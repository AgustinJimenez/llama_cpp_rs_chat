//! Telegram notification tool.

use serde_json::Value;
use crate::utils::silent_command;

/// Send a message via Telegram Bot API.
///
/// Reads bot token and chat ID from:
/// 1. Environment variables: TELEGRAM_BOT_TOKEN, TELEGRAM_CHAT_ID
/// 2. DB config table: telegram_bot_token, telegram_chat_id
///
/// Uses curl via std::process::Command to avoid requiring an async HTTP client.
pub fn tool_send_telegram(
    args: &Value,
    db: Option<&llama_chat_db::SharedDatabase>,
) -> String {
    let message = match args.get("message").and_then(|v| v.as_str()) {
        Some(m) if !m.is_empty() => m,
        _ => return "Error: 'message' argument is required and must be non-empty".to_string(),
    };

    // Resolve bot token: env var first, then DB config
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN").ok().or_else(|| {
        db.and_then(|d| {
            let config = d.load_config();
            config.telegram_bot_token
        })
    });

    let chat_id = std::env::var("TELEGRAM_CHAT_ID").ok().or_else(|| {
        db.and_then(|d| {
            let config = d.load_config();
            config.telegram_chat_id
        })
    });

    let bot_token = match bot_token {
        Some(t) if !t.is_empty() => t,
        _ => return "Error: Telegram bot token not configured. Set TELEGRAM_BOT_TOKEN env var or configure in app settings.".to_string(),
    };

    let chat_id = match chat_id {
        Some(c) if !c.is_empty() => c,
        _ => return "Error: Telegram chat ID not configured. Set TELEGRAM_CHAT_ID env var or configure in app settings.".to_string(),
    };

    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "text": message,
        "parse_mode": "Markdown"
    });
    let body_str = body.to_string();

    let result = silent_command("curl")
        .args([
            "-s",
            "-X", "POST",
            &url,
            "-H", "Content-Type: application/json",
            "-d", &body_str,
            "--max-time", "10",
        ])
        .output();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if output.status.success() {
                if let Ok(resp) = serde_json::from_str::<Value>(&stdout) {
                    if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                        "Telegram message sent successfully.".to_string()
                    } else {
                        let desc = resp.get("description").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                        format!("Telegram API error: {desc}")
                    }
                } else {
                    "Telegram message sent (could not parse response).".to_string()
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                format!("Failed to send Telegram message: {stderr}")
            }
        }
        Err(e) => format!("Failed to run curl: {e}. Make sure curl is installed."),
    }
}
