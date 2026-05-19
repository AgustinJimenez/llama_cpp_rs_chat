// ─── Logging Commands ─────────────────────────────────────────────────

use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use crate::web::database::SharedDatabase;

#[derive(Deserialize)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
}

#[derive(Deserialize)]
pub struct AppErrorInput {
    pub level: String,
    pub source: String,
    pub message: String,
    pub details: Option<String>,
    pub timestamp: Option<i64>,
}

#[derive(Serialize)]
pub struct AppErrorEntry {
    pub id: i64,
    pub level: String,
    pub source: String,
    pub message: String,
    pub details: Option<String>,
    pub timestamp: i64,
}

fn truncate_owned(mut text: String, max_len: usize) -> String {
    if text.len() > max_len {
        let mut end = max_len;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

#[tauri::command]
pub fn log_to_file(logs: Vec<LogEntry>) {
    for log in logs {
        match log.level.as_str() {
            "info" => info!("[FRONTEND] {}", log.message),
            "warn" => warn!("[FRONTEND] {}", log.message),
            "error" => error!("[FRONTEND] {}", log.message),
            _ => info!("[FRONTEND] {}", log.message),
        }
    }
}

#[tauri::command]
pub fn record_app_error(
    error: AppErrorInput,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let level = error.level.trim();
    let source = error.source.trim();
    let message = error.message.trim();
    if level.is_empty() || source.is_empty() || message.is_empty() {
        return Err("level, source, and message are required".into());
    }

    let level = truncate_owned(level.to_string(), 32);
    let source = truncate_owned(source.to_string(), 128);
    let message = truncate_owned(message.to_string(), 8_000);
    let details = error.details.map(|value| truncate_owned(value, 32_000));

    db.record_app_error(
        &level,
        &source,
        &message,
        details.as_deref(),
        error.timestamp,
    )?;

    Ok(serde_json::json!({"success": true}))
}

#[tauri::command]
pub fn get_app_errors(
    limit: Option<usize>,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<Vec<AppErrorEntry>, String> {
    db.get_app_errors(limit.unwrap_or(100).clamp(1, 500))?
        .into_iter()
        .map(|row| {
            Ok(AppErrorEntry {
                id: row.id,
                level: row.level,
                source: row.source,
                message: row.message,
                details: row.details,
                timestamp: row.timestamp,
            })
        })
        .collect()
}

#[tauri::command]
pub fn clear_app_errors(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let deleted = db.clear_app_errors()?;
    Ok(serde_json::json!({"success": true, "deleted": deleted}))
}
