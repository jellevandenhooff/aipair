use axum::{
    Json, Router,
    extract::{Path, State},
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
use tracing::info;

use crate::jj::Jj;
use crate::review::{Author, Review, ReviewStore, ThreadStatus};
use crate::timeline::TimelineStore;
use crate::todo::TodoStore;
use crate::topic::TopicStore;

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
    topics: TopicStore,
    todos: TodoStore,
    timeline: TimelineStore,
}

pub async fn serve(port: u16) -> anyhow::Result<()> {
    let jj = Jj::discover()?;
    let store = ReviewStore::new(jj.repo_path());
    store.init()?;
    let topics = TopicStore::new(jj.repo_path());
    topics.init()?;

    let todos = TodoStore::new(jj.repo_path());
    let timeline = TimelineStore::new(jj.repo_path());
    let state = Arc::new(AppState { jj, store, topics, todos, timeline });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // MCP server over HTTP (stateless, merged before adding app state)
    let mcp_router = crate::mcp::create_mcp_router();

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
        .route("/api/topics", get(list_topics))
        .route("/api/topics/{topic_id}", get(get_topic))
        .route("/api/topics/{topic_id}/finish", post(finish_topic))
        .route("/api/todos", get(get_todos))
        .route("/api/todos", post(create_todo))
        .route("/api/todos/{id}", patch(update_todo))
        .route("/api/todos/{id}", delete(delete_todo))
        .route("/api/timeline", get(get_timeline))
        .with_state(state)
        .merge(mcp_router);

    // Add static file serving for bundled frontend
    #[cfg(feature = "bundled-frontend")]
    let app = app.fallback(static_handler);

    let app = app.layer(cors).layer(TraceLayer::new_for_http());

    let addr = format!("0.0.0.0:{}", port);
    info!("Starting server on http://localhost:{}", port);
    info!("MCP endpoint available at http://localhost:{}/mcp", port);

    #[cfg(feature = "bundled-frontend")]
    info!("Web UI available at http://localhost:{}", port);
    #[cfg(not(feature = "bundled-frontend"))]
    info!("Web UI not bundled - run 'npm run dev' in web/ for development");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
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
    #[serde(skip_serializing_if = "Option::is_none")]
    topic_id: Option<String>,
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
struct ChangesResponse {
    changes: Vec<ChangeWithStatus>,
    main_change_id: Option<String>,
    graph: Vec<DagRow>,
}

async fn list_changes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let changes = match state.jj.log(100) {
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

    // Load all topics to map changes to topic IDs
    let topics = state.topics.list().unwrap_or_default();
    let change_to_topic: std::collections::HashMap<String, String> = topics
        .iter()
        .flat_map(|t| t.changes.iter().map(move |c| (c.clone(), t.id.clone())))
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
            let topic_id = change_to_topic.get(&change.change_id).cloned();
            ChangeWithStatus {
                change,
                merged,
                open_thread_count,
                revision_count,
                has_pending_changes,
                topic_id,
            }
        })
        .collect();

    Json(ChangesResponse {
        changes: changes_with_status,
        main_change_id,
        graph,
    })
    .into_response()
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
    #[serde(skip_serializing_if = "Option::is_none")]
    target_message: Option<String>,
    /// Line-by-line diff of commit messages (if comparing revisions with different messages)
    #[serde(skip_serializing_if = "Option::is_none")]
    message_diff: Option<Vec<DiffChunk>>,
}

#[derive(Deserialize)]
struct DiffQuery {
    /// Optional commit ID to view diff at (defaults to current working copy)
    commit: Option<String>,
    /// Optional base commit to compare from (defaults to parent)
    base: Option<String>,
}

async fn get_diff(
    State(state): State<Arc<AppState>>,
    Path(change_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<DiffQuery>,
) -> impl IntoResponse {
    // If a specific commit is requested, use it as the "to" revision
    let to_rev = query.commit.as_deref().unwrap_or(&change_id);

    let diff = match state.jj.diff(to_rev, query.base.as_deref()) {
        Ok(diff) => diff,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Get target message when viewing a specific revision
    let target_message = query.commit.as_ref().and_then(|commit| {
        state.jj.get_change(commit).ok().map(|c| c.description)
    });

    // Compute message diff when comparing revisions
    let message_diff = match (query.base.as_ref(), query.commit.as_ref()) {
        (Some(base), Some(_commit)) => {
            let base_msg = state.jj.get_change(base).ok().map(|c| c.description).unwrap_or_default();
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
                topic_id: None,
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
                topic_id: None,
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

// --- Topic endpoints ---

#[derive(Serialize)]
struct TopicChangeInfo {
    change_id: String,
    description: String,
    open_thread_count: usize,
}

#[derive(Serialize)]
struct TopicResponse {
    id: String,
    name: String,
    status: crate::topic::TopicStatus,
    change_count: usize,
    changes: Vec<TopicChangeInfo>,
    notes: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
struct TopicsListResponse {
    topics: Vec<TopicResponse>,
}

async fn list_topics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let topics = match state.topics.list() {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Get all changes for ordering
    let all_changes = state.jj.log(200).unwrap_or_default();

    let mut topic_responses = Vec::new();
    for topic in &topics {
        // Sort topic's changes in topological order (jj log returns newest first, reverse for base-to-tip)
        let mut ordered: Vec<_> = all_changes
            .iter()
            .filter(|c| topic.changes.contains(&c.change_id))
            .collect();
        ordered.reverse();

        let changes: Vec<TopicChangeInfo> = ordered
            .iter()
            .map(|c| {
                let open_threads = state
                    .store
                    .get(&c.change_id)
                    .ok()
                    .flatten()
                    .map(|r| {
                        r.threads
                            .iter()
                            .filter(|t| t.status == ThreadStatus::Open)
                            .count()
                    })
                    .unwrap_or(0);
                TopicChangeInfo {
                    change_id: c.change_id.clone(),
                    description: c.description.clone(),
                    open_thread_count: open_threads,
                }
            })
            .collect();

        let notes = state.topics.get_notes(&topic.id).ok().filter(|n| !n.is_empty());

        topic_responses.push(TopicResponse {
            id: topic.id.clone(),
            name: topic.name.clone(),
            status: topic.status.clone(),
            change_count: topic.changes.len(),
            changes,
            notes,
            created_at: topic.created_at,
        });
    }

    Json(TopicsListResponse {
        topics: topic_responses,
    })
    .into_response()
}

async fn get_topic(
    State(state): State<Arc<AppState>>,
    Path(topic_id): Path<String>,
) -> impl IntoResponse {
    let topic = match state.topics.get(&topic_id) {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::NOT_FOUND, "Topic not found").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let all_changes = state.jj.log(200).unwrap_or_default();
    let mut ordered: Vec<_> = all_changes
        .iter()
        .filter(|c| topic.changes.contains(&c.change_id))
        .collect();
    ordered.reverse();

    let changes: Vec<TopicChangeInfo> = ordered
        .iter()
        .map(|c| {
            let open_threads = state
                .store
                .get(&c.change_id)
                .ok()
                .flatten()
                .map(|r| {
                    r.threads
                        .iter()
                        .filter(|t| t.status == ThreadStatus::Open)
                        .count()
                })
                .unwrap_or(0);
            TopicChangeInfo {
                change_id: c.change_id.clone(),
                description: c.description.clone(),
                open_thread_count: open_threads,
            }
        })
        .collect();

    let notes = state.topics.get_notes(&topic.id).ok().filter(|n| !n.is_empty());

    Json(TopicResponse {
        id: topic.id.clone(),
        name: topic.name.clone(),
        status: topic.status.clone(),
        change_count: topic.changes.len(),
        changes,
        notes,
        created_at: topic.created_at,
    })
    .into_response()
}

#[derive(Deserialize)]
struct FinishTopicRequest {
    #[serde(default)]
    force: bool,
}

async fn finish_topic(
    State(state): State<Arc<AppState>>,
    Path(topic_id): Path<String>,
    Json(req): Json<FinishTopicRequest>,
) -> impl IntoResponse {
    let topic = match state.topics.get(&topic_id) {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::NOT_FOUND, "Topic not found".to_string()).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    if topic.changes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "Topic has no changes".to_string(),
        )
            .into_response();
    }

    let all_changes = match state.jj.log(200) {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let ordered: Vec<_> = all_changes
        .iter()
        .filter(|c| topic.changes.contains(&c.change_id))
        .collect();

    if ordered.len() != topic.changes.len() {
        return (
            StatusCode::BAD_REQUEST,
            "Some topic changes not found in jj log".to_string(),
        )
            .into_response();
    }

    // Validate descriptions
    let empty: Vec<_> = ordered
        .iter()
        .filter(|c| c.description.trim().is_empty())
        .map(|c| c.change_id[..8.min(c.change_id.len())].to_string())
        .collect();
    if !empty.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            format!("Changes with empty descriptions: {}", empty.join(", ")),
        )
            .into_response();
    }

    // Validate open threads
    if !req.force {
        let mut total_open = 0;
        for change in &ordered {
            if let Ok(Some(review)) = state.store.get(&change.change_id) {
                total_open += review
                    .threads
                    .iter()
                    .filter(|t| t.status == ThreadStatus::Open)
                    .count();
            }
        }
        if total_open > 0 {
            return (
                StatusCode::BAD_REQUEST,
                format!("{} open review thread(s). Use force to override.", total_open),
            )
                .into_response();
        }
    }

    // Tip is first in ordered (newest first from jj log)
    let tip = &ordered[0];

    if let Err(e) = state.jj.move_bookmark("main", &tip.change_id) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    if let Err(e) = state.topics.finish(&topic_id) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    Json(serde_json::json!({
        "success": true,
        "message": format!("Finished topic '{}'. main moved to {}.", topic.name, &tip.change_id[..8.min(tip.change_id.len())])
    }))
    .into_response()
}

// --- Todo endpoints ---

async fn get_todos(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut tree = match state.todos.load() {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Sync with topics
    let topics = state.topics.list().unwrap_or_default();
    if let Err(e) = state.todos.sync_topics(&mut tree, &topics) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health() {
        let response = health().await;
        assert_eq!(response, "ok");
    }
}
