use hyper::{Body, Request, Response, StatusCode};
use serde::Deserialize;
use std::convert::Infallible;
use std::io::Write;

use crate::web::{request_parsing::parse_json_body, response_helpers::json_error};
use crate::{sys_debug, sys_warn};

#[derive(Debug, Deserialize)]
struct FrontendLogBatch {
    logs: Vec<FrontendLogEntry>,
}

#[derive(Debug, Deserialize)]
struct FrontendLogEntry {
    level: String,
    message: String,
    #[serde(default)]
    timestamp: Option<String>,
}

fn sanitize_level(level: &str) -> &str {
    match level.to_ascii_lowercase().as_str() {
        "info" => "INFO",
        "warn" | "warning" => "WARN",
        "error" => "ERROR",
        "debug" => "DEBUG",
        _ => "INFO",
    }
}

fn truncate_message(mut message: String) -> String {
    const MAX_LEN: usize = 20_000;
    if message.len() > MAX_LEN {
        message.truncate(MAX_LEN);
        message.push_str("…[truncated]");
    }
    message
}

pub async fn handle_post_frontend_logs(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let batch: FrontendLogBatch = match parse_json_body(req.into_body()).await {
        Ok(batch) => batch,
        Err(error_response) => return Ok(error_response),
    };

    if batch.logs.is_empty() {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "logs must be non-empty",
        ));
    }

    // Prevent huge payloads from causing unbounded file writes.
    const MAX_ENTRIES: usize = 200;
    if batch.logs.len() > MAX_ENTRIES {
        return Ok(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too many log entries",
        ));
    }

    let entry_count = batch.logs.len();

    // Ensure log directory exists.
    let log_dir = "logs/frontend";
    if let Err(e) = std::fs::create_dir_all(log_dir) {
        sys_warn!("[FRONTEND LOGS] Failed to create log dir: {}", e);
        return Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create log directory",
        ));
    }

    // Filename format requested: year-month-day-hour_minute.log
    let file_stamp = chrono::Local::now().format("%Y-%m-%d-%H_%M").to_string();
    let log_path = format!("{}/{}.log", log_dir, file_stamp);

    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => file,
        Err(e) => {
            sys_warn!("[FRONTEND LOGS] Failed to open log file: {}", e);
            return Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to open log file",
            ));
        }
    };

    for entry in batch.logs {
        let level = sanitize_level(&entry.level);
        let msg = truncate_message(entry.message);
        let ts = entry
            .timestamp
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| chrono::Local::now().to_rfc3339());
        let line = format!("[{}] [{}] {}\n", ts, level, msg.replace('\n', "\\n"));
        if let Err(e) = file.write_all(line.as_bytes()) {
            sys_warn!("[FRONTEND LOGS] Write failed: {}", e);
            return Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to write log file",
            ));
        }
    }

    if let Err(e) = file.flush() {
        sys_warn!("[FRONTEND LOGS] Flush failed: {}", e);
    }

    sys_debug!(
        "[FRONTEND LOGS] Wrote {} entries to {}",
        entry_count,
        log_path
    );

    Ok(crate::web::response_helpers::json_raw(
        StatusCode::OK,
        r#"{"success":true}"#.to_string(),
    ))
}
