use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, patch, post},
};
#[cfg(feature = "bundled-frontend")]
use axum::http::header;
use renderdag::{Ancestor, GraphRow, GraphRowRenderer, Renderer};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use crate::jj::Jj;
use crate::review::{Author, Review, ReviewStore, ThreadStatus};
use crate::session::{SessionStatus, SessionStore};
use crate::timeline::TimelineStore;
use crate::todo::TodoStore;
#[cfg(feature = "bundled-frontend")]
mod embedded {
    use rust_embed::Embed;

    #[derive(Embed)]
    #[folder = "web/dist"]
    pub struct Assets;
}

#[cfg(feature = "bundled-frontend")]
async fn static_handler(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // Try to serve the exact file
    if let Some(content) = embedded::Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            content.data.into_owned(),
        )
            .into_response();
    }

    // For SPA routing: serve index.html for non-file paths
    if let Some(content) = embedded::Assets::get("index.html") {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html")],
            content.data.into_owned(),
        )
            .into_response();
    }

    (StatusCode::NOT_FOUND, "Not found").into_response()
}

struct AppState {
    jj: Jj,
    store: ReviewStore,
    todos: TodoStore,
    timeline: TimelineStore,
    sessions: SessionStore,
}

/// Resolve the port to bind to:
/// 1. If explicit port given, use it
/// 2. Else try .aipair/port file
/// 3. Fall back to port 0 (OS assigns)
fn resolve_port(explicit: Option<u16>) -> u16 {
    if let Some(p) = explicit {
        return p;
    }
    let port_file = std::path::Path::new(".aipair/port");
    if let Ok(contents) = std::fs::read_to_string(port_file) {
        if let Ok(p) = contents.trim().parse::<u16>() {
            return p;
        }
    }
    0
}

pub async fn serve(port: Option<u16>) -> anyhow::Result<()> {
    let jj = Jj::discover()?;
    let store = ReviewStore::new(jj.repo_path());
    store.init()?;

    let todos = TodoStore::new(jj.repo_path());
    let timeline = TimelineStore::new(jj.repo_path());
    let sessions = SessionStore::new(jj.repo_path());
    let state = Arc::new(AppState { jj, store, todos, timeline, sessions });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/changes", get(list_changes))
        .route("/api/changes/{change_id}/diff", get(get_diff))
        .route("/api/changes/{change_id}/review", get(get_review))
        .route("/api/changes/{change_id}/review", post(create_review))
        .route("/api/changes/{change_id}/comments", post(add_comment))
        .route("/api/changes/{change_id}/threads/{thread_id}/reply", post(reply_to_thread))
        .route("/api/changes/{change_id}/threads/{thread_id}/resolve", post(resolve_thread))
        .route("/api/changes/{change_id}/threads/{thread_id}/reopen", post(reopen_thread))
        .route("/api/changes/{change_id}/merge", post(merge_change))
        .route("/api/todos", get(get_todos))
        .route("/api/todos", post(create_todo))
        .route("/api/todos/{id}", patch(update_todo))
        .route("/api/todos/{id}", delete(delete_todo))
        .route("/api/timeline", get(get_timeline))
        .route("/api/sessions/{name}/merge", post(merge_session))
        .route("/api/sessions/{name}/changes", get(get_session_changes))
        .with_state(state);

    // Add static file serving for bundled frontend
    #[cfg(feature = "bundled-frontend")]
    let app = app.fallback(static_handler);

    let app = app.layer(cors).layer(TraceLayer::new_for_http());

    let is_auto = port.is_none();
    let bind_port = resolve_port(port);
    let addr = format!("0.0.0.0:{}", bind_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let actual_port = listener.local_addr()?.port();

    // Write port file only when auto-allocated (not explicit --port)
    if is_auto {
        let port_dir = std::path::Path::new(".aipair");
        if !port_dir.exists() {
            std::fs::create_dir_all(port_dir)?;
        }
        std::fs::write(port_dir.join("port"), actual_port.to_string())?;
    }

    info!("Starting server on http://localhost:{}", actual_port);
    #[cfg(feature = "bundled-frontend")]
    info!("Web UI available at http://localhost:{}", actual_port);
    #[cfg(not(feature = "bundled-frontend"))]
    info!("Web UI not bundled - run 'npm run dev' in web/ for development");

    // Watch for binary changes and re-exec on rebuild
    tokio::spawn(watch_binary());

    axum::serve(listener, app).await?;

    Ok(())
}

async fn watch_binary() {
    use std::os::unix::process::CommandExt;
    use std::time::SystemTime;

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            warn!("Cannot determine current exe for self-watch: {}", e);
            return;
        }
    };

    let initial_mtime = match std::fs::metadata(&exe).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(e) => {
            warn!("Cannot stat binary for self-watch: {}", e);
            return;
        }
    };

    let mut last_mtime = initial_mtime;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let mtime = match std::fs::metadata(&exe).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if mtime != last_mtime {
            last_mtime = mtime;
            info!("Binary changed, restarting in 500ms...");
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            // Re-check in case build was still in progress
            let final_mtime = std::fs::metadata(&exe)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            if final_mtime != mtime {
                info!("Binary still changing, waiting another cycle");
                continue;
            }
            let args: Vec<String> = std::env::args().collect();
            let err = std::process::Command::new(&exe).args(&args[1..]).exec();
            warn!("Failed to exec: {}", err);
        }
    }
}

async fn health() -> &'static str {
    "ok"
}

/// Change with merged status and review info for API response
#[derive(Serialize)]
struct ChangeWithStatus {
    #[serde(flatten)]
    change: crate::jj::Change,
    merged: bool,
    open_thread_count: usize,
    revision_count: usize,
    has_pending_changes: bool,

    session_name: Option<String>,
}

/// Serializable graph row for the DAG visualization.
/// We re-serialize from renderdag's GraphRow to control the JSON format
/// (especially LinkLine which bitflags serializes as strings, not numbers).
#[derive(Serialize)]
struct DagRow {
    node: String,
    glyph: String,
    merge: bool,
    node_line: Vec<renderdag::NodeLine>,
    link_line: Option<Vec<u16>>,
    term_line: Option<Vec<bool>>,
    pad_lines: Vec<renderdag::PadLine>,
}

impl DagRow {
    fn from_graph_row(row: GraphRow<String>) -> Self {
        DagRow {
            node: row.node,
            glyph: row.glyph,
            merge: row.merge,
            node_line: row.node_line,
            link_line: row.link_line.map(|v| v.into_iter().map(|l| l.bits()).collect()),
            term_line: row.term_line,
            pad_lines: row.pad_lines,
        }
    }
}

#[derive(Serialize)]
struct PushChangeSnapshot {
    change_id: String,
    commit_id: String,
}

#[derive(Serialize)]
struct SessionPush {
    summary: String,
    commit_id: String,
    timestamp: String,
    change_count: usize,
    changes: Vec<PushChangeSnapshot>,
}

#[derive(Serialize)]
struct SessionSummary {
    name: String,
    status: String,
    push_count: usize,
    last_push: Option<String>,
    base_bookmark: String,
    open_thread_count: usize,
    change_count: usize,
    pushes: Vec<SessionPush>,
    /// Whether the live bookmark state matches the latest push snapshot.
    /// True = safe to merge; false = unpushed changes exist.
    pushed_clean: bool,
}

#[derive(Serialize)]
struct ChangesResponse {
    changes: Vec<ChangeWithStatus>,
    main_change_id: Option<String>,
    graph: Vec<DagRow>,
    sessions: Vec<SessionSummary>,
}

async fn list_changes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Scope to main's ancestors — sessions get their own per-session query
    let changes = match state.jj.log_revset("ancestors(main, 100)") {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let main_change_id = match state.jj.get_bookmark("main") {
        Ok(id) => id,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Load all reviews to get thread counts
    let reviews = state.store.list().unwrap_or_default();
    let review_map: std::collections::HashMap<_, _> = reviews
        .into_iter()
        .map(|r| (r.change_id.clone(), r))
        .collect();

    // Load sessions — build rich summaries with thread counts and change counts
    let all_sessions = state.sessions.list().unwrap_or_default();
    let session_summaries: Vec<SessionSummary> = all_sessions
        .iter()
        .map(|s| {
            // Count open threads across this session's changes
            let open_threads: usize = s.changes.iter()
                .filter_map(|cid| review_map.get(cid))
                .map(|r| r.threads.iter().filter(|t| t.status == ThreadStatus::Open).count())
                .sum();
            // Count changes from jj (if bookmark exists)
            let revset = format!("{}..{}", s.base_bookmark, s.bookmark);
            let current_changes = state.jj.log_revset(&revset).ok();
            let change_count = current_changes.as_ref()
                .map(|c| c.len())
                .unwrap_or(s.changes.len());

            // pushed_clean: clone's live state matches latest push snapshot.
            // Check the clone (what the user sees), not the main repo bookmark.
            let pushed_clean = if let Some(last_push) = s.pushes.last() {
                if last_push.changes.is_empty() {
                    false
                } else {
                    let clone_path = state.jj.repo_path().join(&s.clone_path);
                    let clone_changes = if clone_path.exists() {
                        let clone_jj = Jj::new(&clone_path);
                        let revset = format!("{}@origin..visible_heads()", s.base_bookmark);
                        clone_jj.log_revset(&revset).ok()
                    } else {
                        // No clone — fall back to main repo bookmark state
                        current_changes.clone()
                    };
                    match clone_changes {
                        Some(live) => {
                            let push_set: std::collections::HashSet<(&str, &str)> = last_push.changes.iter()
                                .map(|c| (c.change_id.as_str(), c.commit_id.as_str()))
                                .collect();
                            let live_set: std::collections::HashSet<(&str, &str)> = live.iter()
                                .map(|c| (c.change_id.as_str(), c.commit_id.as_str()))
                                .collect();
                            push_set == live_set
                        }
                        None => false,
                    }
                }
            } else {
                false
            };

            let pushes = s.pushes.iter().map(|p| SessionPush {
                summary: p.summary.clone(),
                commit_id: p.commit_id.clone(),
                timestamp: p.timestamp.to_rfc3339(),
                change_count: p.changes.len(),
                changes: p.changes.iter().map(|c| PushChangeSnapshot {
                    change_id: c.change_id.clone(),
                    commit_id: c.commit_id.clone(),
                }).collect(),
            }).collect();
            SessionSummary {
                name: s.name.clone(),
                status: match s.status {
                    SessionStatus::Active => "active".to_string(),
                    SessionStatus::Merged => "merged".to_string(),
                },
                push_count: s.pushes.len(),
                last_push: s.pushes.last().map(|p| p.summary.clone()),
                base_bookmark: s.base_bookmark.clone(),
                open_thread_count: open_threads,
                change_count,
                pushes,
                pushed_clean,
            }
        })
        .collect();

    // A change is merged if it appears at or after the main bookmark in the ancestor list
    // The list is ordered newest to oldest, so we find main's position and mark everything from there
    let main_idx = main_change_id
        .as_ref()
        .and_then(|main_id| changes.iter().position(|c| &c.change_id == main_id));

    // Compute DAG graph layout using sapling-renderdag.
    // Changes come from jj log in topological order (newest first).
    let change_ids: std::collections::HashSet<&str> = changes
        .iter()
        .map(|c| c.change_id.as_str())
        .collect();

    let mut renderer = GraphRowRenderer::new();
    let mut graph: Vec<DagRow> = Vec::new();
    for change in &changes {
        let parents: Vec<Ancestor<String>> = change
            .parent_change_ids
            .iter()
            .filter(|p| change_ids.contains(p.as_str()))
            .map(|p| Ancestor::Parent(p.clone()))
            .collect();
        let glyph = if change.empty { "o" } else { "@" };
        let row = renderer.next_row(change.change_id.clone(), parents, glyph.to_string(), String::new());
        graph.push(DagRow::from_graph_row(row));
    }

    let changes_with_status: Vec<ChangeWithStatus> = changes
        .into_iter()
        .enumerate()
        .map(|(idx, change)| {
            let merged = main_idx.map(|mi| idx >= mi).unwrap_or(false);
            let (open_thread_count, revision_count, has_pending_changes) = review_map
                .get(&change.change_id)
                .map(|r| {
                    let open = r
                        .threads
                        .iter()
                        .filter(|t| t.status == ThreadStatus::Open)
                        .count();
                    // Pending if working_commit differs from last revision's commit
                    let pending = match (r.working_commit_id.as_ref(), r.revisions.last()) {
                        (Some(working), Some(last_rev)) => working != &last_rev.commit_id,
                        (Some(_), None) => true, // Has working commit but no revisions
                        _ => false,
                    };
                    (open, r.revisions.len(), pending)
                })
                .unwrap_or((0, 0, false));
            // Also pending if current jj commit differs from working_commit_id
            let has_pending_changes = has_pending_changes || review_map
                .get(&change.change_id)
                .map(|r| {
                    r.working_commit_id.as_ref().map(|w| w != &change.commit_id).unwrap_or(false)
                })
                .unwrap_or(false);
            ChangeWithStatus {
                change,
                merged,
                open_thread_count,
                revision_count,
                has_pending_changes,
                session_name: None,
            }
        })
        .collect();

    Json(ChangesResponse {
        changes: changes_with_status,
        main_change_id,
        graph,
        sessions: session_summaries,
    })
    .into_response()
}

/// Resolve which jj repo path to use. If a session name is given, return its
/// clone's Jj (error if session not found or clone missing). Otherwise return
/// the main repo's Jj.
fn resolve_jj_for_session(state: &AppState, session_name: Option<&str>) -> Result<Jj, (StatusCode, String)> {
    if let Some(name) = session_name {
        let session = state.sessions.get(name)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Session '{name}' not found")))?;
        let clone_path = state.jj.repo_path().join(&session.clone_path);
        if !clone_path.exists() {
            return Err((StatusCode::NOT_FOUND, format!("Clone for session '{name}' not found")));
        }
        return Ok(Jj::new(&clone_path));
    }
    Ok(Jj::new(state.jj.repo_path()))
}

/// A single chunk in a text diff
#[derive(Serialize)]
struct DiffChunk {
    /// "equal", "delete", or "insert"
    tag: &'static str,
    text: String,
}

/// Compute a line-based diff between two strings
fn compute_text_diff(old: &str, new: &str) -> Vec<DiffChunk> {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(old, new);
    diff.iter_all_changes()
        .map(|change| {
            let tag = match change.tag() {
                ChangeTag::Equal => "equal",
                ChangeTag::Delete => "delete",
                ChangeTag::Insert => "insert",
            };
            DiffChunk {
                tag,
                text: change.value().to_string(),
            }
        })
        .collect()
}

#[derive(Serialize)]
struct DiffResponse {
    diff: crate::jj::Diff,
    /// Commit message for the target revision (when viewing a specific revision)

    target_message: Option<String>,
    /// Line-by-line diff of commit messages (if comparing revisions with different messages)

    message_diff: Option<Vec<DiffChunk>>,
}

#[derive(Deserialize)]
struct DiffQuery {
    /// Optional commit ID to view diff at (defaults to current working copy)
    commit: Option<String>,
    /// Optional base commit to compare from (defaults to parent)
    base: Option<String>,
    /// Optional session name — when set, queries the session's clone
    session: Option<String>,
}

async fn get_diff(
    State(state): State<Arc<AppState>>,
    Path(change_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<DiffQuery>,
) -> impl IntoResponse {
    // Resolve which jj instance to use: clone (for session) or main repo
    let jj = match resolve_jj_for_session(&state, query.session.as_deref()) {
        Ok(jj) => jj,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    // If a specific commit is requested, use it as the "to" revision
    let to_rev = query.commit.as_deref().unwrap_or(&change_id);

    let diff = match jj.diff(to_rev, query.base.as_deref()) {
        Ok(diff) => diff,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Get target message when viewing a specific revision
    let target_message = query.commit.as_ref().and_then(|commit| {
        jj.get_change(commit).ok().map(|c| c.description)
    });

    // Compute message diff when comparing revisions
    let message_diff = match (query.base.as_ref(), query.commit.as_ref()) {
        (Some(base), Some(_commit)) => {
            let base_msg = jj.get_change(base).ok().map(|c| c.description).unwrap_or_default();
            let target_msg = target_message.clone().unwrap_or_default();
            if base_msg != target_msg {
                Some(compute_text_diff(&base_msg, &target_msg))
            } else {
                None
            }
        }
        _ => None,
    };

    Json(DiffResponse { diff, target_message, message_diff }).into_response()
}

#[derive(Serialize)]
struct ReviewResponse {
    review: Option<Review>,
}

/// Add a virtual pending revision if the current commit differs from the last recorded revision
fn add_pending_revision_if_needed(mut review: Review, current_commit_id: &str) -> Review {
    let has_pending = match review.revisions.last() {
        Some(last_rev) => last_rev.commit_id != current_commit_id,
        None => true, // No revisions yet, so current state is "pending"
    };

    if has_pending {
        let next_number = review.revisions.last().map(|r| r.number + 1).unwrap_or(1);
        review.revisions.push(crate::review::Revision {
            number: next_number,
            commit_id: current_commit_id.to_string(),
            created_at: chrono::Utc::now(),
            description: None,
            is_pending: true,
        });
    }

    review
}

/// Populate display positions on threads by mapping through diffs
fn populate_display_positions(jj: &crate::jj::Jj, review: &mut Review, current_commit_id: &str) {
    if current_commit_id.is_empty() {
        return;
    }

    let mapped = crate::line_mapper::map_all_threads(jj, &review.threads, current_commit_id);

    for thread in &mut review.threads {
        if let Some(pos) = mapped.get(&thread.id) {
            thread.display_line_start = Some(pos.line_start);
            thread.display_line_end = Some(pos.line_end);
            thread.is_deleted = pos.is_deleted;
            thread.is_displaced = pos.line_start != thread.line_start
                || pos.line_end != thread.line_end;
        }
    }
}

async fn get_review(
    State(state): State<Arc<AppState>>,
    Path(change_id): Path<String>,
) -> impl IntoResponse {
    // Get current commit_id for this change
    let current_commit_id = state.jj.get_change(&change_id)
        .map(|c| c.commit_id)
        .unwrap_or_default();

    match state.store.get(&change_id) {
        Ok(Some(review)) => {
            let mut review = add_pending_revision_if_needed(review, &current_commit_id);
            populate_display_positions(&state.jj, &mut review, &current_commit_id);
            Json(ReviewResponse { review: Some(review) }).into_response()
        }
        Ok(None) => Json(ReviewResponse { review: None }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateReviewRequest {
    base: Option<String>,
}

async fn create_review(
    State(state): State<Arc<AppState>>,
    Path(change_id): Path<String>,
    Json(req): Json<CreateReviewRequest>,
) -> impl IntoResponse {
    let base = req.base.as_deref().unwrap_or("@-");

    // Get commit_id for this change
    let current_commit_id = state.jj.get_change(&change_id)
        .map(|c| c.commit_id)
        .unwrap_or_default();

    match state.store.get_or_create(&change_id, base, &current_commit_id) {
        Ok(review) => {
            let review = add_pending_revision_if_needed(review, &current_commit_id);
            Json(ReviewResponse {
                review: Some(review),
            })
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct AddCommentRequest {
    file: String,
    line_start: usize,
    line_end: usize,
    text: String,
}

#[derive(Serialize)]
struct AddCommentResponse {
    review: Review,
    thread_id: String,
}

async fn add_comment(
    State(state): State<Arc<AppState>>,
    Path(change_id): Path<String>,
    Json(req): Json<AddCommentRequest>,
) -> impl IntoResponse {
    // Get commit_id for this change
    let commit_id = match state.jj.log(100) {
        Ok(changes) => changes
            .iter()
            .find(|c| c.change_id == change_id)
            .map(|c| c.commit_id.clone())
            .unwrap_or_default(),
        Err(_) => String::new(),
    };

    match state.store.add_comment(
        &change_id,
        &req.file,
        req.line_start,
        req.line_end,
        Author::User,
        &req.text,
        &commit_id,
    ) {
        Ok((review, thread_id)) => {
            let _ = state.timeline.append(&crate::timeline::TimelineEntry {
                timestamp: chrono::Utc::now(),
                data: crate::timeline::TimelineEventData::ReviewComment {
                    change_id: change_id.clone(),
                    thread_id: thread_id.clone(),
                    file: req.file.clone(),
                    line_start: req.line_start,
                    line_end: req.line_end,
                    text: req.text.clone(),
                },
            });
            Json(AddCommentResponse { review, thread_id }).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct ReplyRequest {
    text: String,
}

async fn reply_to_thread(
    State(state): State<Arc<AppState>>,
    Path((change_id, thread_id)): Path<(String, String)>,
    Json(req): Json<ReplyRequest>,
) -> impl IntoResponse {
    match state.store.reply_to_thread(&change_id, &thread_id, Author::User, &req.text) {
        Ok(review) => {
            let _ = state.timeline.append(&crate::timeline::TimelineEntry {
                timestamp: chrono::Utc::now(),
                data: crate::timeline::TimelineEventData::ReviewReply {
                    change_id: change_id.clone(),
                    thread_id: thread_id.clone(),
                    author: "user".to_string(),
                    text: req.text.clone(),
                },
            });
            Json(ReviewResponse { review: Some(review) }).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn resolve_thread(
    State(state): State<Arc<AppState>>,
    Path((change_id, thread_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match state.store.resolve_thread(&change_id, &thread_id) {
        Ok(review) => Json(ReviewResponse { review: Some(review) }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn reopen_thread(
    State(state): State<Arc<AppState>>,
    Path((change_id, thread_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match state.store.reopen_thread(&change_id, &thread_id) {
        Ok(review) => Json(ReviewResponse { review: Some(review) }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct MergeRequest {
    #[serde(default)]
    force: bool,
}

#[derive(Serialize)]
struct MergeResponse {
    success: bool,
    message: String,
}

async fn merge_change(
    State(state): State<Arc<AppState>>,
    Path(change_id): Path<String>,
    Json(req): Json<MergeRequest>,
) -> impl IntoResponse {
    // Check if already merged
    let main_change_id = match state.jj.get_bookmark("main") {
        Ok(id) => id,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    if main_change_id.as_ref() == Some(&change_id) {
        return Json(MergeResponse {
            success: false,
            message: "Change is already at main".to_string(),
        })
        .into_response();
    }

    // Get current change info
    let change = match state.jj.get_change(&change_id) {
        Ok(change) => change,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MergeResponse {
                    success: false,
                    message: format!("Failed to get change info: {}", e),
                }),
            )
                .into_response();
        }
    };
    let current_commit_id = change.commit_id.clone();

    // Check for empty commit message (unless force)
    if !req.force && change.description.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(MergeResponse {
                success: false,
                message: "Cannot merge: commit message is empty. Set a description with `jj describe -m \"...\"`".to_string(),
            }),
        )
            .into_response();
    }

    // Check for pending changes and open threads (unless force)
    if !req.force {
        let review = state.store.get(&change_id).ok().flatten();

        // Check for pending changes: either no review/revisions, or current commit differs from last revision
        let has_pending = match review.as_ref().and_then(|r| r.revisions.last()) {
            Some(last_rev) => current_commit_id != last_rev.commit_id,
            None => true, // No review or no revisions recorded = pending
        };

        if has_pending {
            return (
                StatusCode::BAD_REQUEST,
                Json(MergeResponse {
                    success: false,
                    message: "Cannot merge: pending changes not yet recorded as a revision. \
                              Use force=true to override."
                        .to_string(),
                }),
            )
            .into_response();
        }

        if let Some(review) = review {
            let open_threads: Vec<_> = review
                .threads
                .iter()
                .filter(|t| t.status == ThreadStatus::Open)
                .collect();

            if !open_threads.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(MergeResponse {
                        success: false,
                        message: format!(
                            "Cannot merge: {} open thread(s). Use force=true to override.",
                            open_threads.len()
                        ),
                    }),
                )
                    .into_response();
            }
        }
    }

    // Move the bookmark
    match state.jj.move_bookmark("main", &change_id) {
        Ok(()) => Json(MergeResponse {
            success: true,
            message: format!("Merged: main now at {}", &change_id[..8.min(change_id.len())]),
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// --- Todo endpoints ---

async fn get_todos(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let tree = match state.todos.load() {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    Json(tree).into_response()
}

#[derive(Deserialize)]
struct CreateTodoRequest {
    text: String,
    parent_id: Option<String>,
    after_id: Option<String>,
}

async fn create_todo(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTodoRequest>,
) -> impl IntoResponse {
    let mut tree = match state.todos.load() {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    match state.todos.add_item(
        &mut tree,
        req.text,
        req.parent_id.as_deref(),
        req.after_id.as_deref(),
    ) {
        Ok(_id) => Json(tree).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct UpdateTodoRequest {
    text: Option<String>,
    checked: Option<bool>,
    parent_id: Option<String>,
    after_id: Option<String>,
}

async fn update_todo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateTodoRequest>,
) -> impl IntoResponse {
    let mut tree = match state.todos.load() {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Update text/checked if provided
    if req.text.is_some() || req.checked.is_some() {
        if let Err(e) = state.todos.update_item(&mut tree, &id, req.text, req.checked) {
            return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
        }
    }

    // Move if parent_id is provided (including explicit null for "move to root")
    if req.parent_id.is_some() {
        let parent = req.parent_id.as_deref().filter(|s| !s.is_empty());
        if let Err(e) = state.todos.move_item(&mut tree, &id, parent, req.after_id.as_deref()) {
            return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
        }
    }

    Json(tree).into_response()
}

async fn delete_todo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut tree = match state.todos.load() {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    if let Err(e) = state.todos.delete_item(&mut tree, &id) {
        return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }

    Json(tree).into_response()
}

// --- Timeline endpoint ---

#[derive(Deserialize)]
struct TimelineQuery {
    since: Option<String>,
    until: Option<String>,
    change_id: Option<String>,
    #[serde(rename = "type")]
    event_type: Option<String>,
}

async fn get_timeline(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<TimelineQuery>,
) -> impl IntoResponse {
    // Auto-import new Claude Code session content
    let repo_path = state.jj.repo_path().to_path_buf();
    if let Err(e) = state.timeline.import_claude_sessions(&repo_path) {
        tracing::warn!("Failed to import Claude sessions: {}", e);
    }

    let filter = crate::timeline::TimelineFilter {
        since: query.since.as_deref().and_then(|s| s.parse().ok()),
        until: query.until.as_deref().and_then(|s| s.parse().ok()),
        change_id: query.change_id,
        event_type: query.event_type,
    };

    let has_filter = filter.since.is_some()
        || filter.until.is_some()
        || filter.change_id.is_some()
        || filter.event_type.is_some();

    match state.timeline.read(if has_filter { Some(&filter) } else { None }) {
        Ok(entries) => Json(entries).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// --- Session endpoints ---

#[derive(Serialize)]
struct SessionChangesResponse {
    changes: Vec<ChangeWithStatus>,
    graph: Vec<DagRow>,
    /// Commit ID that these changes are actually based on
    base_commit_id: Option<String>,
    /// What the base bookmark currently resolves to in the main repo
    base_current_commit_id: Option<String>,
}

#[derive(Deserialize)]
struct SessionChangesQuery {
    /// "live", "latest", or a push index (0 = oldest)
    #[serde(default = "default_version")]
    version: String,
}

fn default_version() -> String {
    "live".to_string()
}

/// Compute DAG graph from a list of changes.
fn compute_graph(changes: &[ChangeWithStatus]) -> Vec<DagRow> {
    let change_ids: std::collections::HashSet<&str> = changes
        .iter()
        .map(|c| c.change.change_id.as_str())
        .collect();
    let mut renderer = GraphRowRenderer::new();
    let mut graph: Vec<DagRow> = Vec::new();
    for c in changes {
        let parents: Vec<Ancestor<String>> = c.change
            .parent_change_ids
            .iter()
            .filter(|p| change_ids.contains(p.as_str()))
            .map(|p| Ancestor::Parent(p.clone()))
            .collect();
        let glyph = if c.change.empty { "o" } else { "@" };
        let row = renderer.next_row(c.change.change_id.clone(), parents, glyph.to_string(), String::new());
        graph.push(DagRow::from_graph_row(row));
    }
    graph
}

/// Convert raw jj changes to ChangeWithStatus (no review data).
fn changes_to_status(changes: Vec<crate::jj::Change>, session_name: &str) -> Vec<ChangeWithStatus> {
    changes.into_iter().map(|change| ChangeWithStatus {
        change,
        merged: false,
        open_thread_count: 0,
        revision_count: 0,
        has_pending_changes: false,
        session_name: Some(session_name.to_string()),
    }).collect()
}

async fn get_session_changes(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(query): Query<SessionChangesQuery>,
) -> impl IntoResponse {
    let session = match state.sessions.get(&name) {
        Ok(Some(s)) => s,
        Ok(None) => return (StatusCode::NOT_FOUND, format!("Session '{name}' not found")).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // What the base bookmark currently resolves to in the main repo
    let base_current_commit_id = state.jj.get_change(&session.base_bookmark)
        .ok().map(|c| c.commit_id);

    let (changes_with_status, base_commit_id) = if query.version == "live" {
        // Query the clone directory
        let clone_path = state.jj.repo_path().join(&session.clone_path);
        if !clone_path.exists() {
            // No clone — fall back to latest pushed state
            return get_session_changes_latest(&state, &session, &name).into_response();
        }
        let clone_jj = Jj::new(&clone_path);
        let revset = format!("{}@origin..visible_heads()", session.base_bookmark);
        // Base in the clone: what base_bookmark@origin resolves to
        let base = clone_jj.get_change(&format!("{}@origin", session.base_bookmark))
            .ok().map(|c| c.commit_id);
        match clone_jj.log_revset(&revset) {
            Ok(changes) => (changes_to_status(changes, &name), base),
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else if query.version == "latest" {
        return get_session_changes_latest(&state, &session, &name).into_response();
    } else if let Ok(push_idx) = query.version.parse::<usize>() {
        // Historical push — reconstruct from stored commit_ids
        if push_idx >= session.pushes.len() {
            return (StatusCode::NOT_FOUND, format!("Push index {push_idx} out of range")).into_response();
        }
        let push = &session.pushes[push_idx];
        if push.changes.is_empty() {
            // Old push without snapshot data — fall back to latest
            return get_session_changes_latest(&state, &session, &name).into_response();
        }
        // Query jj by commit_ids to get full change info (including parents)
        let commit_ids: Vec<&str> = push.changes.iter().map(|c| c.commit_id.as_str()).collect();
        let revset = commit_ids.join(" | ");
        match state.jj.log_revset(&revset) {
            // For historical pushes, base is unknown (could derive from parents but not critical)
            Ok(changes) => (changes_to_status(changes, &name), None),
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else {
        return (StatusCode::BAD_REQUEST, format!("Invalid version: {}", query.version)).into_response();
    };

    let graph = compute_graph(&changes_with_status);
    Json(SessionChangesResponse {
        changes: changes_with_status,
        graph,
        base_commit_id,
        base_current_commit_id,
    }).into_response()
}

/// Helper: get changes from the current pushed state (main repo bookmark).
fn get_session_changes_latest(
    state: &AppState,
    session: &crate::session::Session,
    name: &str,
) -> Json<SessionChangesResponse> {
    let revset = format!("{}..{}", session.base_bookmark, session.bookmark);
    let changes = state.jj.log_revset(&revset).unwrap_or_default();

    // For "latest", base is the current base bookmark in the main repo
    let base_commit_id = state.jj.get_change(&session.base_bookmark)
        .ok().map(|c| c.commit_id);

    // Load reviews for thread/revision info
    let reviews = state.store.list().unwrap_or_default();
    let review_map: std::collections::HashMap<_, _> = reviews
        .into_iter()
        .map(|r| (r.change_id.clone(), r))
        .collect();

    let main_change_id = state.jj.get_bookmark("main").ok().flatten();

    let changes_with_status: Vec<ChangeWithStatus> = changes
        .into_iter()
        .map(|change| {
            let merged = main_change_id.as_ref() == Some(&change.change_id);
            let (open_thread_count, revision_count, has_pending_changes) = review_map
                .get(&change.change_id)
                .map(|r| {
                    let open = r.threads.iter().filter(|t| t.status == ThreadStatus::Open).count();
                    let pending = match (r.working_commit_id.as_ref(), r.revisions.last()) {
                        (Some(working), Some(last_rev)) => working != &last_rev.commit_id,
                        (Some(_), None) => true,
                        _ => false,
                    };
                    (open, r.revisions.len(), pending)
                })
                .unwrap_or((0, 0, false));
            let has_pending_changes = has_pending_changes || review_map
                .get(&change.change_id)
                .map(|r| r.working_commit_id.as_ref().map(|w| w != &change.commit_id).unwrap_or(false))
                .unwrap_or(false);
            ChangeWithStatus {
                change,
                merged,
                open_thread_count,
                revision_count,
                has_pending_changes,
                session_name: Some(name.to_string()),
            }
        })
        .collect();

    let graph = compute_graph(&changes_with_status);
    Json(SessionChangesResponse {
        changes: changes_with_status,
        graph,
        base_commit_id: base_commit_id.clone(),
        base_current_commit_id: base_commit_id, // latest is always current
    })
}

async fn merge_session(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let mut session = match state.sessions.get(&name) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Json(MergeResponse {
                success: false,
                message: format!("Session '{name}' not found"),
            })
            .into_response()
        }
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    if session.status != SessionStatus::Active {
        return Json(MergeResponse {
            success: false,
            message: format!("Session '{name}' is not active"),
        })
        .into_response();
    }

    // Fetch to get latest from clone's pushes
    let _ = state.jj.git_fetch();

    // Find session bookmark tip
    let bookmark = &session.bookmark;
    let session_tip = match state.jj.get_bookmark(bookmark) {
        Ok(Some(id)) => id,
        Ok(None) => {
            return Json(MergeResponse {
                success: false,
                message: format!("Bookmark '{bookmark}' not found — was it pushed?"),
            })
            .into_response()
        }
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Move main bookmark to session tip
    if let Err(e) = state.jj.move_bookmark("main", &session_tip) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    // Delete session bookmark
    let _ = state.jj.bookmark_delete(bookmark);

    // Update status
    session.status = SessionStatus::Merged;
    if let Err(e) = state.sessions.save(&session) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    Json(MergeResponse {
        success: true,
        message: format!(
            "Session '{name}' merged into main at {}",
            &session_tip[..12.min(session_tip.len())]
        ),
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health() {
        let response = health().await;
        assert_eq!(response, "ok");
    }
}
