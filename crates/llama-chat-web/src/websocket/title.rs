//! Title generation helpers for WebSocket chat completions.

use llama_chat_db::SharedDatabase;
use llama_chat_worker::worker::worker_bridge::SharedWorkerBridge;

/// Strip tool call/response blocks and thinking blocks from assistant content
/// so they don't pollute the title generation prompt.
pub fn strip_tool_tags(content: &str) -> String {
    // Remove <tool_call>...</tool_call> blocks (and unclosed ones)
    let re_tc = regex::Regex::new(r"<tool_call>[\s\S]*?(?:</tool_call>|$)").unwrap();
    // Remove <tool_response>...</tool_response> blocks
    let re_tr = regex::Regex::new(r"<tool_response>[\s\S]*?(?:</tool_response>|$)").unwrap();
    // Remove <think>...</think> blocks
    let re_think = regex::Regex::new(r"<think>[\s\S]*?(?:</think>|$)").unwrap();
    let s = re_tc.replace_all(content, "");
    let s = re_tr.replace_all(&s, "");
    let s = re_think.replace_all(&s, "");
    s.trim().to_string()
}

/// Clean up model-generated title: strip quotes, markdown, "Title:" prefix, truncate.
pub fn sanitize_title(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    // Strip leading "Title:" or "title:" prefix
    if let Some(rest) = s.strip_prefix("Title:").or_else(|| s.strip_prefix("title:")) {
        s = rest.trim().to_string();
    }
    // Strip surrounding quotes
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s = s[1..s.len() - 1].to_string();
    }
    // Strip markdown bold/italic
    s = s.replace("**", "").replace("__", "");
    // Take first line only
    if let Some(pos) = s.find('\n') {
        s.truncate(pos);
    }
    // Truncate to 60 chars
    let s = s.trim().to_string();
    // Reject if the model hallucinated a raw tag as the title
    if s.starts_with('<') {
        return String::new();
    }
    if s.chars().count() > 60 {
        s.chars().take(60).collect::<String>().trim_end().to_string()
    } else {
        s
    }
}

/// Spawn a background task that generates/updates the conversation title.
pub fn spawn_title_generation(
    conv_id: String,
    db: SharedDatabase,
    bridge: SharedWorkerBridge,
) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let messages = match db.get_messages(&conv_id) {
            Ok(m) => m,
            Err(_) => return,
        };
        let first_user = messages.iter().find(|m| m.role == "user");
        let first_assistant = messages.iter().find(|m| m.role == "assistant");
        if first_user.is_none() || first_assistant.is_none() {
            return;
        }
        let last_user = messages.iter().rev().find(|m| m.role == "user");
        let last_assistant = messages.iter().rev().find(|m| m.role == "assistant");
        let mut prompt = String::new();
        let fu = first_user.unwrap();
        let fa = first_assistant.unwrap();
        let fu_trunc: String = fu.content.chars().take(200).collect();
        let fa_trunc: String = strip_tool_tags(&fa.content).chars().take(200).collect();
        prompt.push_str(&format!("User: {fu_trunc}\nAssistant: {fa_trunc}"));
        if let (Some(lu), Some(la)) = (last_user, last_assistant) {
            if lu.content != fu.content {
                let lu_trunc: String = lu.content.chars().take(200).collect();
                let la_trunc: String = strip_tool_tags(&la.content).chars().take(200).collect();
                prompt.push_str(&format!("\n\n[Latest]\nUser: {lu_trunc}\nAssistant: {la_trunc}"));
            }
        }
        eprintln!("[WS_TITLE] Requesting title for {conv_id}");
        match bridge.generate_title(&conv_id, &prompt).await {
            Ok(raw_title) => {
                let title = sanitize_title(&raw_title);
                eprintln!("[WS_TITLE] Generated: '{title}' (raw: '{raw_title}')");
                if !title.is_empty() {
                    if let Err(e) = db.update_conversation_title(&conv_id, &title) {
                        eprintln!("[WS_TITLE] Failed to store: {e}");
                    }
                }
            }
            Err(e) => eprintln!("[WS_TITLE] Title generation FAILED: {e}"),
        }
    });
}
