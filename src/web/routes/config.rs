// Configuration route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;

use crate::web::{
    config::{db_config_to_sampler_config, load_config_for_conversation, sampler_config_to_db},
    database::SharedDatabase,
    logger::LOGGER,
    models::SamplerConfig,
    request_parsing::parse_json_body,
    response_helpers::{json_error, json_raw},
};

pub async fn handle_get_config(
    #[cfg(not(feature = "mock"))] _bridge: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let db_config = db.load_config();
    let config = db_config_to_sampler_config(&db_config);

    match serde_json::to_string(&config) {
        Ok(config_json) => Ok(json_raw(StatusCode::OK, config_json)),
        Err(_) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to serialize configuration",
        )),
    }
}

pub async fn handle_post_config(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] _bridge: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Parse request body
    let incoming_config: SamplerConfig = match parse_json_body(req.into_body()).await {
        Ok(config) => config,
        Err(error_response) => return Ok(error_response),
    };

    // Basic validation
    if !(0.0..=2.0).contains(&incoming_config.temperature) {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "temperature must be between 0.0 and 2.0",
        ));
    }
    if !(0.0..=1.0).contains(&incoming_config.top_p) {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "top_p must be between 0.0 and 1.0",
        ));
    }
    if incoming_config.context_size.unwrap_or(0) == 0 {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "context_size must be positive",
        ));
    }

    // Load existing config to preserve model_history
    let existing_db_config = db.load_config();

    // Merge: take incoming values but keep existing model_history
    let mut merged = sampler_config_to_db(&incoming_config);
    merged.model_history = existing_db_config.model_history;

    match db.save_config(&merged) {
        Ok(_) => {
            // Sync file logging toggle at runtime
            LOGGER.set_enabled(!merged.disable_file_logging);
            Ok(json_raw(StatusCode::OK, r#"{"success":true}"#.to_string()))
        }
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to save configuration: {e}"),
        )),
    }
}

/// Extract conversation ID from path like /api/conversations/{id}/config
fn extract_conversation_id_from_config_path(path: &str) -> Option<String> {
    let stripped = path.strip_prefix("/api/conversations/")?;
    let id = stripped.strip_suffix("/config")?;
    let id = id.trim_end_matches(".txt");
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

/// GET /api/conversations/:id/config
pub async fn handle_get_conversation_config(
    path: &str,
    #[cfg(not(feature = "mock"))] _bridge: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let conversation_id = match extract_conversation_id_from_config_path(path) {
        Some(id) => id,
        None => {
            return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid conversation ID"));
        }
    };

    let config = load_config_for_conversation(&db, &conversation_id);

    match serde_json::to_string(&config) {
        Ok(json) => Ok(json_raw(StatusCode::OK, json)),
        Err(_) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to serialize configuration",
        )),
    }
}

/// POST /api/conversations/:id/config
pub async fn handle_post_conversation_config(
    req: Request<Body>,
    path: &str,
    #[cfg(not(feature = "mock"))] _bridge: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let conversation_id = match extract_conversation_id_from_config_path(path) {
        Some(id) => id,
        None => {
            return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid conversation ID"));
        }
    };

    let incoming: SamplerConfig = match parse_json_body(req.into_body()).await {
        Ok(config) => config,
        Err(error_response) => return Ok(error_response),
    };

    // Same validation as global config
    if !(0.0..=2.0).contains(&incoming.temperature) {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "temperature must be between 0.0 and 2.0",
        ));
    }
    if !(0.0..=1.0).contains(&incoming.top_p) {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "top_p must be between 0.0 and 1.0",
        ));
    }

    let db_config = sampler_config_to_db(&incoming);

    match db.save_conversation_config(&conversation_id, &db_config) {
        Ok(_) => Ok(json_raw(StatusCode::OK, r#"{"success":true}"#.to_string())),
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to save conversation config: {e}"),
        )),
    }
}
