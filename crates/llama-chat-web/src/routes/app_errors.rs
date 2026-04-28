use std::convert::Infallible;

use hyper::{Body, Request, Response, StatusCode};
use serde::Deserialize;

use llama_chat_db::{current_timestamp_millis, SharedDatabase};
use crate::request_parsing::{get_query_param, parse_json_body};
use crate::response_helpers::{json_error, json_raw};

#[derive(Deserialize)]
struct RecordAppErrorRequest {
    level: String,
    source: String,
    message: String,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    timestamp: Option<i64>,
}

fn truncate_text(mut text: String, max_len: usize) -> String {
    if text.len() > max_len {
        let mut end = max_len;
        while end < text.len() && !text.is_char_boundary(end) { end += 1; }
        text.truncate(end);
        text.push_str("…[truncated]");
    }
    text
}

pub async fn handle_record_app_error(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let payload: RecordAppErrorRequest = match parse_json_body(req.into_body()).await {
        Ok(payload) => payload,
        Err(error_response) => return Ok(error_response),
    };

    if payload.level.trim().is_empty()
        || payload.source.trim().is_empty()
        || payload.message.trim().is_empty()
    {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "level, source, and message are required",
        ));
    }

    let level = truncate_text(payload.level, 32);
    let source = truncate_text(payload.source, 128);
    let message = truncate_text(payload.message, 8_000);
    let details = payload.details.map(|value| truncate_text(value, 32_000));

    match db.record_app_error(
        &level,
        &source,
        &message,
        details.as_deref(),
        payload.timestamp.or_else(|| Some(current_timestamp_millis())),
    ) {
        Ok(()) => Ok(json_raw(
            StatusCode::OK,
            r#"{"success":true}"#.to_string(),
        )),
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to store app error: {e}"),
        )),
    }
}

pub async fn handle_get_app_errors(
    req: &Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let limit = get_query_param(req.uri(), "limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, 500);

    match db.get_app_errors(limit) {
        Ok(errors) => Ok(json_raw(
            StatusCode::OK,
            serde_json::to_string(&errors).unwrap_or_else(|_| "[]".to_string()),
        )),
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to load app errors: {e}"),
        )),
    }
}

pub async fn handle_clear_app_errors(
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    match db.clear_app_errors() {
        Ok(deleted) => Ok(json_raw(
            StatusCode::OK,
            format!(r#"{{"success":true,"deleted":{deleted}}}"#),
        )),
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to clear app errors: {e}"),
        )),
    }
}
