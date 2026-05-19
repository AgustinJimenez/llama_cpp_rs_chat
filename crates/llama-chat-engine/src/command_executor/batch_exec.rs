use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

use llama_chat_types::*;

use crate::tool_dispatch::{
    run_native_tool_with_timeout,
    execute_single_tool,
    is_read_only_tool,
    MAX_PARALLEL_TOOLS,
};
use super::output_assembly::maybe_summarize_or_truncate;

/// Result of executing one tool within a batch: (text_output, image_bytes)
pub type ToolResult = (String, Vec<Vec<u8>>);

/// Execute a batch of tool calls (len > 1).
///
/// Returns the combined output string and collected image bytes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_batch_tools(
    all_calls: &[(String, serde_json::Value)],
    conversation_id: &str,
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
    token_pos: i32,
    context_size: u32,
    cancel: Option<Arc<AtomicBool>>,
    use_htmd: bool,
    browser_backend: &crate::browser::BrowserBackend,
    mcp_manager: Option<Arc<dyn llama_chat_tools::McpManagerOps>>,
    db: llama_chat_db::SharedDatabase,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    tags: &crate::tool_tags::ToolTags,
) -> (String, Vec<Vec<u8>>) {
    let mut combined_output = String::new();
    let mut all_response_images: Vec<Vec<u8>> = Vec::new();

    // Group consecutive tool calls by read-only vs write classification.
    let mut groups: Vec<(bool, Vec<usize>)> = Vec::new();
    for (i, (name, _args)) in all_calls.iter().enumerate() {
        let is_ro = is_read_only_tool(name);
        if let Some(last) = groups.last_mut() {
            if last.0 == is_ro {
                last.1.push(i);
                continue;
            }
        }
        groups.push((is_ro, vec![i]));
    }

    // Log the execution plan
    for (is_ro, indices) in &groups {
        let names: Vec<&str> = indices.iter().map(|&i| all_calls[i].0.as_str()).collect();
        if *is_ro && indices.len() > 1 {
            log_info!(conversation_id, "⚡ Parallel group ({} read-only): {:?}", indices.len(), names);
        } else {
            log_info!(conversation_id, "🔄 Serial group ({}): {:?}", indices.len(), names);
        }
    }

    // Pre-allocate result slots
    let mut results: Vec<Option<ToolResult>> = vec![None; all_calls.len()];
    let mut all_durations: Vec<u64> = vec![0u64; all_calls.len()];

    for (is_read_only, indices) in &groups {
        if *is_read_only && indices.len() > 1 {
            let parallel_count = indices.len().min(MAX_PARALLEL_TOOLS);
            log_info!(
                conversation_id,
                "[BATCH] Executing {} read-only tools in parallel",
                parallel_count
            );

            let thread_data: Vec<(usize, String, String)> = indices
                .iter()
                .take(MAX_PARALLEL_TOOLS)
                .map(|&i| {
                    let (name, args) = &all_calls[i];
                    let single_json = serde_json::json!({"name": name, "arguments": args}).to_string();
                    let conv_id = conversation_id.to_string();
                    (i, single_json, conv_id)
                })
                .collect();

            std::thread::scope(|s| {
                let handles: Vec<_> = thread_data
                    .iter()
                    .map(|(idx, json, conv_id)| {
                        let idx = *idx;
                        let tool_name = all_calls[idx].0.clone();
                        let mcp_clone = mcp_manager.clone();
                        let backend_clone = browser_backend.clone();
                        let db_clone = db.clone();
                        s.spawn(move || {
                            let tool_start = std::time::Instant::now();
                            let result = run_native_tool_with_timeout(
                                json,
                                conv_id,
                                use_htmd,
                                backend_clone,
                                mcp_clone,
                                db_clone,
                            );
                            let duration_ms = tool_start.elapsed().as_millis() as u64;
                            let native_result = result.unwrap_or_else(|| {
                                llama_chat_tools::NativeToolResult::text_only(
                                    format!("Error: Tool '{}' returned no output", tool_name)
                                )
                            });
                            (idx, native_result.text, native_result.images, duration_ms)
                        })
                    })
                    .collect();

                for handle in handles {
                    if let Ok((idx, text, images, duration_ms)) = handle.join() {
                        all_durations[idx] = duration_ms;
                        results[idx] = Some((text, images));
                    }
                }
            });

            // Execute any overflow beyond MAX_PARALLEL_TOOLS serially
            for &i in indices.iter().skip(MAX_PARALLEL_TOOLS) {
                let (name, args) = &all_calls[i];
                let single_json = serde_json::json!({"name": name, "arguments": args}).to_string();
                let (tool_output, tool_images, dur) = execute_single_tool(
                    name, args, &single_json,
                    conversation_id,
                    token_sender,
                    token_pos,
                    context_size,
                    cancel.clone(),
                    use_htmd,
                    browser_backend,
                    mcp_manager.clone(),
                    db.clone(),
                    model, backend, chat_template_string, tags,
                );
                all_durations[i] = dur;
                results[i] = Some((tool_output, tool_images));
            }
        } else {
            // Execute serially
            for &i in indices {
                let (name, args) = &all_calls[i];
                let single_json = serde_json::json!({"name": name, "arguments": args}).to_string();
                let (tool_output, tool_images, dur) = execute_single_tool(
                    name, args, &single_json,
                    conversation_id,
                    token_sender,
                    token_pos,
                    context_size,
                    cancel.clone(),
                    use_htmd,
                    browser_backend,
                    mcp_manager.clone(),
                    db.clone(),
                    model, backend, chat_template_string, tags,
                );
                all_durations[i] = dur;
                results[i] = Some((tool_output, tool_images));
            }
        }
    }

    // Merge results in original order, streaming to frontend
    for (i, (name, _args)) in all_calls.iter().enumerate() {
        let dur = all_durations[i];
        llama_chat_db::event_log::log_event(
            conversation_id,
            "tool_timing",
            &format!("{{\"name\":\"{}\",\"duration_ms\":{}}}", name, dur),
        );
        if let Some(ref sender) = token_sender {
            let _ = sender.send(TokenData {
                tool_timing: Some(ToolTimingLive { name: name.clone(), duration_ms: dur }),
                ..Default::default()
            });
        }
        let header = format!("[Tool {}: {}]\n", i + 1, name);
        if let Some(ref sender) = token_sender {
            let _ = sender.send(TokenData {
                token: header.clone(),
                tokens_used: token_pos,
                max_tokens: context_size as i32,
                status: None,
                ..Default::default()
            });
        }
        combined_output.push_str(&header);

        let (tool_output, tool_images) = results[i].take().unwrap_or_default();
        all_response_images.extend(tool_images);

        let summary_val = _args.get("summary");
        let summary_opt_out = summary_val
            .map(|v| v.as_bool() == Some(false) || v.as_str() == Some("false"))
            .unwrap_or(false);

        let tool_output = maybe_summarize_or_truncate(
            &tool_output, name, model, backend, chat_template_string, conversation_id, summary_opt_out,
        );

        log_info!(
            conversation_id,
            "📤 Tool {} ({}) output: {} chars",
            i + 1,
            name,
            tool_output.len()
        );

        if let Some(ref sender) = token_sender {
            let _ = sender.send(TokenData {
                token: tool_output.trim().to_string(),
                tokens_used: token_pos,
                max_tokens: context_size as i32,
                status: None,
                ..Default::default()
            });
        }
        combined_output.push_str(tool_output.trim());
        if i < all_calls.len() - 1 {
            combined_output.push_str("\n\n");
            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    token: "\n\n".to_string(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32,
                    status: None,
                    ..Default::default()
                });
            }
        }
    }

    (combined_output, all_response_images)
}
