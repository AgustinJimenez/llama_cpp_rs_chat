use llama_chat_db::SharedDatabase;
use serde_json::Value;

pub(super) fn ensure_conversation_row(db: &SharedDatabase, conv_id: &str, provider_id: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let conn = db.connection();
    let _ = conn.execute(
        "INSERT OR IGNORE INTO conversations (id, created_at, updated_at, provider_id) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![conv_id, now, now, provider_id],
    );
}

fn next_sequence(db: &SharedDatabase, conv_id: &str) -> i32 {
    db.get_messages(conv_id)
        .map(|msgs| msgs.len() as i32 + 1)
        .unwrap_or(1)
}

pub(super) fn save_message_now(db: &SharedDatabase, conv_id: &str, role: &str, content: &str) {
    save_message_now_returning_seq(db, conv_id, role, content);
}

/// Like save_message_now but returns the sequence_order used, so callers can update parts later.
pub(super) fn save_message_now_returning_seq(
    db: &SharedDatabase,
    conv_id: &str,
    role: &str,
    content: &str,
) -> i32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let seq = next_sequence(db, conv_id);
    let _ = db.insert_message(conv_id, role, content, now, seq);
    seq
}

pub(super) fn generate_title_via_provider(
    base_url: &str,
    api_key: &str,
    model: &str,
    user_message: &str,
    assistant_snippet: &str,
) -> Option<String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let snippet = if assistant_snippet.len() > 500 {
        &assistant_snippet[..500]
    } else {
        assistant_snippet
    };
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "Generate a concise title (3-6 words) for this conversation. Respond with ONLY the title, no quotes, no punctuation, no explanation."},
            {"role": "user", "content": format!("User: {}\nAssistant: {}", &user_message[..user_message.len().min(300)], snippet)},
        ],
        "max_tokens": 20,
        "temperature": 0.3,
        "stream": false,
    });
    let resp = ureq::post(&url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .ok()?;
    let json: Value = serde_json::from_str(&resp.into_string().ok()?).ok()?;
    let title = json["choices"][0]["message"]["content"]
        .as_str()?
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    if title.is_empty() || title.len() > 100 {
        None
    } else {
        Some(title)
    }
}

pub(super) fn provider_log(conv_id: &Option<String>, event_type: &str, message: &str) {
    eprintln!("[OPENAI_COMPAT] [{event_type}] {message}");
    if let Some(cid) = conv_id {
        llama_chat_db::event_log::log_event(cid, event_type, message);
    }
}

pub(super) fn maybe_generate_title_after_response(
    conv_id: &str,
    db: &SharedDatabase,
    messages: &[Value],
    prompt: &str,
    url: &str,
    api_key: &str,
    model: &str,
    conv_id_owned: &Option<String>,
) {
    if db.get_conversation_title(conv_id).ok().flatten().is_some() {
        return;
    }
    let first_user = messages
        .iter()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .unwrap_or(prompt);
    let assistant_text = messages
        .iter()
        .filter(|m| m["role"] == "assistant")
        .find_map(|m| {
            let c = m["content"].as_str().unwrap_or("");
            if !c.is_empty() && !c.starts_with('{') {
                Some(c)
            } else {
                None
            }
        })
        .unwrap_or("");
    let base_url_clean = url.trim_end_matches("/chat/completions").to_string();
    if let Some(title) = generate_title_via_provider(
        &base_url_clean,
        api_key,
        model,
        first_user,
        assistant_text,
    ) {
        provider_log(conv_id_owned, "title_generated", &title);
        let _ = db.update_conversation_title(conv_id, &title);
    }
}
