// Configuration route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;

use llama_chat_config::{db_config_to_sampler_config, load_config_for_conversation, sampler_config_to_db};
use llama_chat_db::SharedDatabase;
use llama_chat_types::logger::LOGGER;
use llama_chat_types::models::SamplerConfig;
use crate::request_parsing::parse_json_body;
use crate::response_helpers::{json_error, json_raw};

pub async fn handle_get_config(
    #[cfg(not(feature = "mock"))] _bridge: llama_chat_worker::worker::worker_bridge::SharedWorkerBridge,
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
    #[cfg(not(feature = "mock"))] _bridge: llama_chat_worker::worker::worker_bridge::SharedWorkerBridge,
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
    #[cfg(not(feature = "mock"))] _bridge: llama_chat_worker::worker::worker_bridge::SharedWorkerBridge,
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
    #[cfg(not(feature = "mock"))] _bridge: llama_chat_worker::worker::worker_bridge::SharedWorkerBridge,
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

/// GET /api/config/provider-keys — get configured provider API keys (masked)
pub async fn handle_get_provider_keys(
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let conn = db.connection();
    let keys_json: String = conn
        .query_row("SELECT provider_api_keys FROM config LIMIT 1", [], |row| row.get(0))
        .unwrap_or_else(|_| "{}".to_string());

    // Mask API key values for security (show first 4 + last 4 chars)
    let mut result = serde_json::Map::new();
    if let Ok(map) = serde_json::from_str::<serde_json::Value>(&keys_json) {
        if let Some(obj) = map.as_object() {
            for (provider, val) in obj {
                let mut entry = serde_json::Map::new();
                if let Some(key) = val.get("api_key").and_then(|k| k.as_str()) {
                    if key.len() > 12 {
                        entry.insert("api_key".into(), serde_json::json!(format!("{}...{}", &key[..4], &key[key.len()-4..])));
                    } else if !key.is_empty() {
                        entry.insert("api_key".into(), serde_json::json!("****"));
                    }
                    entry.insert("configured".into(), serde_json::json!(!key.is_empty()));
                } else if let Some(key) = val.as_str() {
                    entry.insert("configured".into(), serde_json::json!(!key.is_empty()));
                    if key.len() > 12 {
                        entry.insert("api_key".into(), serde_json::json!(format!("{}...{}", &key[..4], &key[key.len()-4..])));
                    }
                }
                if let Some(url) = val.get("base_url").and_then(|u| u.as_str()) {
                    entry.insert("base_url".into(), serde_json::json!(url));
                }
                result.insert(provider.clone(), serde_json::json!(entry));
            }
        }
    }
    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&serde_json::json!(result)).unwrap(),
    ))
}

/// POST /api/config/provider-keys — set a provider API key
pub async fn handle_set_provider_key(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let body = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };
    let json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid JSON")),
    };

    let provider = match json.get("provider").and_then(|p| p.as_str()) {
        Some(p) => p.to_string(),
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "provider is required")),
    };
    let api_key = json.get("api_key").and_then(|k| k.as_str()).unwrap_or("");
    let base_url = json.get("base_url").and_then(|u| u.as_str());

    // Load existing keys
    let conn = db.connection();
    let existing: String = conn
        .query_row("SELECT provider_api_keys FROM config LIMIT 1", [], |row| row.get(0))
        .unwrap_or_else(|_| "{}".to_string());

    let mut keys: serde_json::Value = serde_json::from_str(&existing).unwrap_or(serde_json::json!({}));

    if api_key.is_empty() && base_url.is_none() {
        // Remove provider
        if let Some(obj) = keys.as_object_mut() {
            obj.remove(&provider);
        }
    } else {
        let mut entry = serde_json::json!({"api_key": api_key});
        if let Some(url) = base_url {
            entry["base_url"] = serde_json::json!(url);
        }
        keys[&provider] = entry;
    }

    let updated = serde_json::to_string(&keys).unwrap_or_else(|_| "{}".to_string());
    match conn.execute("UPDATE config SET provider_api_keys = ?1", [&updated]) {
        Ok(_) => Ok(json_raw(
            StatusCode::OK,
            serde_json::to_string(&serde_json::json!({"success": true, "provider": provider})).unwrap(),
        )),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed: {e}"))),
    }
}

/// GET /api/config/active-provider — get active provider and model
pub async fn handle_get_active_provider(
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let conn = db.connection();
    let result: (String, Option<String>) = conn
        .query_row(
            "SELECT COALESCE(active_provider, 'local'), active_provider_model FROM config LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or(("local".to_string(), None));

    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&serde_json::json!({
            "provider": result.0,
            "model": result.1,
        })).unwrap(),
    ))
}

/// POST /api/config/active-provider — set active provider and model
pub async fn handle_set_active_provider(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let body = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };
    let json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid JSON")),
    };

    let provider = json.get("provider").and_then(|p| p.as_str()).unwrap_or("local");
    let model = json.get("model").and_then(|m| m.as_str());

    let conn = db.connection();
    match conn.execute(
        "UPDATE config SET active_provider = ?1, active_provider_model = ?2",
        rusqlite::params![provider, model],
    ) {
        Ok(_) => Ok(json_raw(
            StatusCode::OK,
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "provider": provider,
                "model": model,
            })).unwrap(),
        )),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed: {e}"))),
    }
}
