use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::jj::Jj;
use crate::review::{Author, Review, ReviewStore};

struct AppState {
    jj: Jj,
    store: ReviewStore,
}

pub async fn serve(port: u16) -> anyhow::Result<()> {
    let jj = Jj::discover()?;
    let store = ReviewStore::new(jj.repo_path());
    store.init()?;

    let state = Arc::new(AppState { jj, store });

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
        .with_state(state)
        .layer(cors);

    let addr = format!("0.0.0.0:{}", port);
    info!("Starting server on http://localhost:{}", port);

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
}

#[derive(Serialize)]
struct ChangesResponse {
    changes: Vec<ChangeWithStatus>,
    main_change_id: Option<String>,
}

async fn list_changes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let changes = match state.jj.log(20) {
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

    // A change is merged if it appears at or after the main bookmark in the ancestor list
    // The list is ordered newest to oldest, so we find main's position and mark everything from there
    let main_idx = main_change_id
        .as_ref()
        .and_then(|main_id| changes.iter().position(|c| &c.change_id == main_id));

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
                        .filter(|t| t.status == crate::review::ThreadStatus::Open)
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
            }
        })
        .collect();

    Json(ChangesResponse {
        changes: changes_with_status,
        main_change_id,
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
        (Some(base), Some(commit)) => {
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
            let review = add_pending_revision_if_needed(review, &current_commit_id);
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
    let commit_id = match state.jj.log(50) {
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
        Ok((review, thread_id)) => Json(AddCommentResponse { review, thread_id }).into_response(),
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
        Ok(review) => Json(ReviewResponse { review: Some(review) }).into_response(),
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

    // Get current commit_id for this change
    let current_commit_id = match state.jj.get_change(&change_id) {
        Ok(change) => change.commit_id,
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

    // Check for pending changes and open threads (unless force)
    if !req.force {
        if let Ok(Some(review)) = state.store.get(&change_id) {
            // Check for pending changes
            let has_pending = match review.revisions.last() {
                Some(last_rev) => current_commit_id != last_rev.commit_id,
                None => true, // Has commit but no revisions recorded
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

            let open_threads: Vec<_> = review
                .threads
                .iter()
                .filter(|t| t.status == crate::review::ThreadStatus::Open)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health() {
        let response = health().await;
        assert_eq!(response, "ok");
    }
}
