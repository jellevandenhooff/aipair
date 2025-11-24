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

#[derive(Serialize)]
struct ChangesResponse {
    changes: Vec<crate::jj::Change>,
}

async fn list_changes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.jj.log(20) {
        Ok(changes) => Json(ChangesResponse { changes }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Serialize)]
struct DiffResponse {
    diff: crate::jj::Diff,
}

async fn get_diff(
    State(state): State<Arc<AppState>>,
    Path(change_id): Path<String>,
) -> impl IntoResponse {
    match state.jj.diff(&change_id, None) {
        Ok(diff) => Json(DiffResponse { diff }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Serialize)]
struct ReviewResponse {
    review: Option<Review>,
}

async fn get_review(
    State(state): State<Arc<AppState>>,
    Path(change_id): Path<String>,
) -> impl IntoResponse {
    match state.store.get(&change_id) {
        Ok(review) => Json(ReviewResponse { review }).into_response(),
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
    match state.store.get_or_create(&change_id, base) {
        Ok(review) => Json(ReviewResponse {
            review: Some(review),
        })
        .into_response(),
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
    match state.store.add_comment(
        &change_id,
        &req.file,
        req.line_start,
        req.line_end,
        Author::User,
        &req.text,
    ) {
        Ok((review, thread_id)) => Json(AddCommentResponse { review, thread_id }).into_response(),
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
