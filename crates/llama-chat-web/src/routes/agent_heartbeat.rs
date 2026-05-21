// Per-conversation agent heartbeat route handlers.
// GET  /api/conversations/:id/heartbeat         — read config + status
// POST /api/conversations/:id/heartbeat         — update config fields
// POST /api/conversations/:id/heartbeat/fire    — trigger one heartbeat immediately
// POST /api/conversations/:id/heartbeat/clear   — mark unread badge as seen

use hyper::{Body, Request, Response, StatusCode};
use serde::Deserialize;
use std::convert::Infallible;

use llama_chat_db::{
    agent_heartbeat::{
        clear_heartbeat_unread, read_heartbeat_config, write_heartbeat_config,
    },
    SharedDatabase,
};
use crate::request_parsing::parse_json_body;
use crate::response_helpers::{json_error, json_response};
use crate::worker_pool::{resolve_bridge_for_conversation, WorkerPool};

// ── GET /api/conversations/:id/heartbeat ─────────────────────────────────────

pub async fn handle_get_heartbeat(
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let cfg = read_heartbeat_config(&db, conversation_id);
    Ok(json_response(StatusCode::OK, &cfg))
}

// ── POST /api/conversations/:id/heartbeat ────────────────────────────────────

#[derive(Deserialize)]
struct HeartbeatUpdate {
    enabled: Option<bool>,
    interval_minutes: Option<u32>,
    prompt: Option<String>,
}

pub async fn handle_post_heartbeat(
    req: Request<Body>,
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let update: HeartbeatUpdate = match parse_json_body(req.into_body()).await {
        Ok(u) => u,
        Err(e) => return Ok(e),
    };

    let mut cfg = read_heartbeat_config(&db, conversation_id);

    if let Some(v) = update.enabled {
        cfg.enabled = v;
    }
    if let Some(v) = update.interval_minutes {
        if v == 0 {
            return Ok(json_error(StatusCode::BAD_REQUEST, "interval_minutes must be >= 1"));
        }
        cfg.interval_minutes = v;
    }
    if let Some(v) = update.prompt {
        cfg.prompt = v;
    }

    match write_heartbeat_config(&db, conversation_id, &cfg) {
        Ok(()) => Ok(json_response(StatusCode::OK, &cfg)),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

// ── POST /api/conversations/:id/heartbeat/fire ───────────────────────────────

pub async fn handle_fire_heartbeat(
    conversation_id: &str,
    #[cfg(not(feature = "mock"))] pool: WorkerPool,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        let bridge = match resolve_bridge_for_conversation(&pool, &db, Some(conversation_id)) {
            Ok(bridge) => bridge,
            Err(e) => return Ok(json_error(StatusCode::SERVICE_UNAVAILABLE, &e)),
        };
        let conv_id = conversation_id.to_string();
        tokio::spawn(crate::agent_heartbeat_runner::fire_one(bridge, db, conv_id));
        Ok(json_response(
            StatusCode::OK,
            &serde_json::json!({ "status": "fired" }),
        ))
    }
    #[cfg(feature = "mock")]
    {
        Ok(json_response(
            StatusCode::OK,
            &serde_json::json!({ "status": "mock" }),
        ))
    }
}

// ── POST /api/conversations/:id/heartbeat/clear ──────────────────────────────

pub async fn handle_clear_heartbeat(
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    clear_heartbeat_unread(&db, conversation_id);
    Ok(json_response(
        StatusCode::OK,
        &serde_json::json!({ "status": "cleared" }),
    ))
}
