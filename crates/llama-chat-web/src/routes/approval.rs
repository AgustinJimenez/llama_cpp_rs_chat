use std::convert::Infallible;

use hyper::{Body, Response, StatusCode};
use llama_chat_db::SharedDatabase;

/// POST /api/approval/:id/approve  — approve a pending dangerous tool call
pub async fn handle_approve(id: &str, db: SharedDatabase) -> Result<Response<Body>, Infallible> {
    let body = match db.resolve_pending_approval(id, "approved") {
        Ok(()) => "{\"ok\":true}",
        Err(_) => "{\"ok\":false}",
    };
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap())
}

/// POST /api/approval/:id/reject  — reject a pending dangerous tool call
pub async fn handle_reject(id: &str, db: SharedDatabase) -> Result<Response<Body>, Infallible> {
    let body = match db.resolve_pending_approval(id, "rejected") {
        Ok(()) => "{\"ok\":true}",
        Err(_) => "{\"ok\":false}",
    };
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap())
}
