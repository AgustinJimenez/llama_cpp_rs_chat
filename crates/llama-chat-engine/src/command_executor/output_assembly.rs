use llama_chat_command::sanitize_command_output;
use tokio::sync::mpsc;
use llama_chat_types::*;

use crate::tool_output::{
    tool_use_one_liner,
    maybe_truncate_tool_output,
    maybe_summarize_tool_output,
    summarize_tool_output,
    summarize_tool_output_with_prompt,
    wrap_output_for_model,
    SUMMARIZE_THRESHOLD,
};

const DEFAULT_IMAGE_SUMMARY_PROMPT: &str =
    "Describe what is visible on this screen. Include: \
     the application or window title, any visible text content, \
     UI elements (buttons, menus, forms), error or status messages, \
     and the overall layout.";

/// Extract the `summary` parameter from a tool call for image results.
/// Returns `None` for summary=false (inject raw images), `Some(prompt)` otherwise.
pub(crate) fn extract_image_summary_prompt(command_text: &str) -> Option<String> {
    let summary_val = extract_summary_param(command_text);
    let cmd_lower = command_text.to_lowercase();
    let disabled = summary_val.as_deref() == Some("false")
        || cmd_lower.contains("\"summary\": false")
        || cmd_lower.contains("\"summary\":false")
        || cmd_lower.contains("summary>\nfalse")
        || cmd_lower.contains("summary>false");
    if disabled {
        return None;
    }
    // Use a custom prompt if the agent supplied one (long enough, not a boolean)
    if let Some(ref s) = summary_val {
        if s != "true" && s != "false" && s.len() > 3 {
            return Some(s.clone());
        }
    }
    Some(DEFAULT_IMAGE_SUMMARY_PROMPT.to_string())
}

/// Re-export `tool_use_one_liner` for use in sibling modules.
pub(crate) fn tool_use_one_liner_pub(name: &str, args: &str, output: &str, elapsed_ms: u64) -> String {
    tool_use_one_liner(name, args, output, elapsed_ms)
}

/// Summarize or truncate a tool output depending on `summary_opt_out`.
pub(crate) fn maybe_summarize_or_truncate(
    tool_output: &str,
    name: &str,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    summary_opt_out: bool,
) -> String {
    if summary_opt_out {
        maybe_truncate_tool_output(tool_output, name, conversation_id)
    } else {
        maybe_summarize_tool_output(tool_output, name, model, backend, chat_template_string, conversation_id)
    }
}

/// Parameters used to assemble the final output block sent to the model.
pub(crate) struct AssemblyParams<'a> {
    pub command_text: &'a str,
    pub raw_output: &'a str,
    pub tool_name_for_log: &'a str,
    pub conversation_id: &'a str,
    pub output_open: &'a str,
    pub output_close: &'a str,
    pub fuzzy_warning: Option<&'a str>,
    pub template_type: Option<&'a str>,
    pub token_sender: &'a Option<mpsc::UnboundedSender<TokenData>>,
    pub token_pos: i32,
    pub context_size: u32,
    pub model: &'a llama_cpp_2::model::LlamaModel,
    pub backend: &'a llama_cpp_2::llama_backend::LlamaBackend,
    pub chat_template_string: Option<&'a str>,
}

/// Assembled output ready for frontend display and model injection.
pub(crate) struct AssembledOutput {
    pub output_block: String,
    pub model_block: String,
}

/// Extract a `summary` param value from raw command text.
fn extract_summary_param(command_text: &str) -> Option<String> {
    // JSON: "summary": "value"
    let json_pattern = "\"summary\":";
    if let Some(pos) = command_text.find(json_pattern) {
        let rest = command_text[pos + json_pattern.len()..].trim_start();
        if rest.starts_with('"') {
            let inner = &rest[1..];
            let mut end = 0;
            let bytes = inner.as_bytes();
            while end < bytes.len() {
                if bytes[end] == b'"' && (end == 0 || bytes[end - 1] != b'\\') { break; }
                end += 1;
            }
            return Some(inner[..end].to_string());
        }
    }
    // XML: <parameter=summary>value</parameter>
    let xml_open = "=summary>\n";
    if let Some(pos) = command_text.find(xml_open) {
        let rest = &command_text[pos + xml_open.len()..];
        if let Some(end) = rest.find("</parameter>") {
            return Some(rest[..end].trim().to_string());
        }
    }
    let xml_open2 = "=summary>";
    if let Some(pos) = command_text.find(xml_open2) {
        let rest = &command_text[pos + xml_open2.len()..];
        if let Some(end) = rest.find("</") {
            return Some(rest[..end].trim().to_string());
        }
    }
    None
}

/// Sanitize, (optionally) summarize, and wrap output text for model injection.
///
/// Returns `(display_text, model_text)`.
pub(crate) fn sanitize_and_summarize(
    p: &AssemblyParams<'_>,
) -> (String, String) {
    // Strip [TOOL_RESULT:...] tag before model sees it (tag is for frontend only)
    let output_for_model = if p.raw_output.starts_with("[TOOL_RESULT:") {
        p.raw_output.splitn(2, ']').nth(1).unwrap_or(p.raw_output).to_string()
    } else {
        p.raw_output.to_string()
    };
    let sanitized = sanitize_command_output(&output_for_model);
    let sanitized = maybe_truncate_tool_output(&sanitized, p.tool_name_for_log, p.conversation_id);

    let summary_value = extract_summary_param(p.command_text);
    let cmd_lower = p.command_text.to_lowercase();
    let summary_disabled = summary_value.as_deref() == Some("false")
        || cmd_lower.contains("\"summary\": false")
        || cmd_lower.contains("\"summary\":false")
        || cmd_lower.contains("summary>\nfalse")
        || cmd_lower.contains("summary>false");
    let custom_summary_prompt = if summary_disabled {
        None
    } else {
        summary_value.filter(|s| s != "true" && s != "false" && s.len() > 3)
    };

    if summary_disabled {
        return (sanitized.clone(), sanitized);
    }

    if p.raw_output.len() > SUMMARIZE_THRESHOLD || sanitized.len() > SUMMARIZE_THRESHOLD {
        let summarize_result = if let Some(ref prompt) = custom_summary_prompt {
            summarize_tool_output_with_prompt(p.model, p.backend, &sanitized, p.chat_template_string, p.conversation_id, Some(prompt))
        } else {
            summarize_tool_output(p.model, p.backend, &sanitized, p.chat_template_string, p.conversation_id)
        };
        match summarize_result {
            Ok(summary) => {
                log_info!(p.conversation_id, "📝 Summarized tool output: {} → {} chars", sanitized.len(), summary.len());
                let summary_block = format!(
                    "\n\n📝 Summary for model ({} → {} chars):\n{}",
                    sanitized.len(), summary.len(), summary.trim()
                );
                if let Some(ref sender) = p.token_sender {
                    let _ = sender.send(TokenData {
                        token: summary_block.clone(),
                        tokens_used: p.token_pos,
                        max_tokens: p.context_size as i32,
                        status: None,
                        ..Default::default()
                    });
                }
                let display = format!("{}{}", sanitized, summary_block);
                let model_summary = format!(
                    "[SUMMARIZED: {} → {} chars. Use summary=false to get raw output.]\n{}",
                    sanitized.len(), summary.len(), summary
                );
                (display, model_summary)
            }
            Err(e) => {
                log_warn!(p.conversation_id, "Summarization failed ({}), using raw output", e);
                (sanitized.clone(), sanitized)
            }
        }
    } else {
        (sanitized.clone(), sanitized)
    }
}

/// Build the final output block and model injection block.
pub(crate) fn assemble_output(
    p: &AssemblyParams<'_>,
    display_text: &str,
    model_text: &str,
    http_error_hint: Option<&str>,
) -> AssembledOutput {
    let output_open = p.output_open;
    let output_close = p.output_close;

    let model_trimmed = model_text.trim();
    let mut model_text_with_warning = model_trimmed.to_string();
    if let Some(warning) = p.fuzzy_warning {
        model_text_with_warning = format!("{}\n\n{}", warning, model_text_with_warning);
    }
    if let Some(hint) = http_error_hint {
        model_text_with_warning = format!("{}\n\n{}", model_text_with_warning, hint);
    }

    let model_injection_block = format!("{}{}{}", output_open, model_text_with_warning, output_close);
    let model_block = wrap_output_for_model(&model_injection_block, p.template_type);

    let output_block = format!("{}{}{}", output_open, display_text.trim(), output_close);

    AssembledOutput { output_block, model_block }
}

/// Save response images to disk and append markdown image links to `output_block`.
pub(crate) fn append_image_links(output_block: &mut String, images: &[Vec<u8>], conversation_id: &str) {
    if images.is_empty() {
        return;
    }
    let images_dir = std::path::PathBuf::from("assets/images").join(conversation_id);
    if let Err(e) = std::fs::create_dir_all(&images_dir) {
        eprintln!("[IMAGES] Failed to create images dir: {e}");
        return;
    }
    for (i, img_bytes) in images.iter().enumerate() {
        let uuid = uuid::Uuid::new_v4();
        let filename = format!("{uuid}.jpg");
        let filepath = images_dir.join(&filename);
        match std::fs::write(&filepath, img_bytes) {
            Ok(()) => {
                let img_url = format!("/api/images/{}/{}", conversation_id, filename);
                let size_kb = img_bytes.len() / 1024;
                eprintln!("[IMAGES] Saved screenshot {}/{}: {} ({}KB)", i + 1, images.len(), filepath.display(), size_kb);
                output_block.push_str(&format!("\n![screenshot]({img_url})"));
            }
            Err(e) => {
                eprintln!("[IMAGES] Failed to save screenshot: {e}");
            }
        }
    }
}
