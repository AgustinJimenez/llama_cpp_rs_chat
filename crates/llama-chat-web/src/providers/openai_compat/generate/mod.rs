//! OpenAI-compatible provider — works with any API that follows the OpenAI chat completions format.
//!
//! Supports: Groq, Gemini, SambaNova, Cerebras, OpenRouter, Together, Fireworks,
//! DeepSeek, local vLLM, Ollama, and any other OpenAI-compatible endpoint.
//!
//! Protocol: HTTP SSE streaming from `POST {base_url}/chat/completions`
//!
//! **Agentic loop**: When tool definitions are included, the provider will execute
//! tool calls locally and loop back to the API until the model produces a final text
//! response (or hits the 20-iteration safety limit).
//!
use crate::providers::openai_compat_request::{
    MAX_AGENTIC_ITERATIONS, MAX_INPUT_TOKENS,
    apply_user_param, budget_trimmed_messages, execute_openai_tool,
    get_agentic_tools, get_cloud_system_prompt, provider_default_params, resolve_model,
    summarize_tool_output,
};
use crate::providers::openai_compat_streaming::stream_sse_response;
use crate::providers::{
    set_remote_generating, set_remote_status, CliTokenData,
};
use super::db::{
    provider_log, save_message_now, save_message_now_returning_seq,
};

use serde_json::{json, Value};
use tokio::sync::mpsc;

mod finalize;
mod tests;

use finalize::{finalize_generation, save_initial_messages, LoopCounters};

// ─── Main generate function with agentic loop ─────────────────────────────

/// Generate a response using an OpenAI-compatible API.
///
/// Streams SSE tokens from `POST {base_url}/chat/completions` and converts them
/// to `CliTokenData` events on the returned channel.
///
/// When the model returns tool calls, they are executed locally and the results
/// are fed back into the conversation for another API round-trip.
#[allow(clippy::too_many_arguments)]
pub async fn generate(
    provider_id: &str,
    prompt: &str,
    model: Option<&str>,
    base_url: &str,
    api_key: &str,
    conversation_id: Option<&str>,
    db: Option<&llama_chat_db::SharedDatabase>,
    user_params: Option<&serde_json::Value>,
    image_data: Option<&[String]>,
    mcp_bridge: Option<llama_chat_worker::worker::worker_bridge::SharedWorkerBridge>,
) -> Result<mpsc::UnboundedReceiver<CliTokenData>, String> {
    let (tx, rx) = mpsc::unbounded_channel();
    let model_name = resolve_model(provider_id, model);
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    eprintln!(
        "[OPENAI_COMPAT] generate() provider={provider_id} model={model_name} url={url}"
    );

    // Read max_tool_calls from config (default 2000)
    let config = db.map(|d| d.load_config());
    let max_iterations = config.as_ref()
        .map(|c| c.max_tool_calls as usize)
        .unwrap_or(MAX_AGENTIC_ITERATIONS);
    let loop_limit = config.as_ref()
        .map(|c| c.loop_detection_limit as u32)
        .unwrap_or(15);

    let api_key_owned = api_key.to_string();
    let provider_id_owned = provider_id.to_string();
    let model_name_clone = model_name.clone();
    let prompt_owned = prompt.to_string();
    let conv_id_owned = conversation_id.map(|s| s.to_string());
    let db_owned = db.cloned();
    let user_params_owned = user_params.cloned();
    let image_data_owned: Vec<String> = image_data.unwrap_or(&[]).to_vec();

    // Use ureq in a blocking task for the streaming HTTP request + agentic loop
    tokio::task::spawn_blocking(move || {
        let start = std::time::Instant::now();

        // Track this generation so frontend can reconnect after refresh
        if let Some(ref cid) = conv_id_owned {
            set_remote_generating(cid, &provider_id_owned);
        }

        // Build MCP proxy if a bridge was provided and MCP tools are available.
        let mcp_proxy: Option<crate::providers::bridge_mcp_proxy::BridgeMcpProxy> =
            mcp_bridge.map(crate::providers::bridge_mcp_proxy::BridgeMcpProxy::new_blocking);
        let mcp_ops: Option<&dyn llama_chat_tools::McpManagerOps> =
            mcp_proxy.as_ref().map(|p| p as &dyn llama_chat_tools::McpManagerOps);

        // Get tool definitions for the agentic loop
        let tools = get_agentic_tools(mcp_ops);
        let has_tools = !tools.is_empty();

        provider_log(&conv_id_owned, "provider_start",
            &format!("provider={} model={} url={} tools={}", provider_id_owned, model_name_clone, url, tools.len()));

        // Build initial messages array with system prompt
        let mut messages: Vec<Value> = vec![
            json!({"role": "system", "content": get_cloud_system_prompt()}),
        ];

        // Add prior conversation turns from DB (reconstruct tool messages)
        if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
            if let Ok(msgs) = db.get_messages(conv_id) {
                for m in &msgs {
                    if m.compacted || m.role == "system" { continue; }
                    if m.role == "tool" {
                        let (tc_id, content) = m.content.split_once("\n\n")
                            .unwrap_or(("unknown", &m.content));
                        messages.push(json!({"role": "tool", "tool_call_id": tc_id, "content": content}));
                    } else if m.role == "assistant" && m.content.contains("\"tool_calls\":") && m.content.starts_with("{") {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&m.content) {
                            let mut msg = json!({"role": "assistant"});
                            if let Some(tc) = parsed.get("tool_calls") { msg["tool_calls"] = tc.clone(); }
                            if let Some(c) = parsed.get("content") { msg["content"] = c.clone(); }
                            else { msg["content"] = Value::Null; }
                            // Preserve reasoning_content for DeepSeek cache
                            if let Some(rc) = parsed.get("reasoning_content") { msg["reasoning_content"] = rc.clone(); }
                            messages.push(msg);
                        } else {
                            messages.push(json!({"role": m.role, "content": m.content}));
                        }
                    } else {
                        messages.push(json!({"role": m.role, "content": m.content}));
                    }
                }
            }
        }

        // Ensure conversation exists in DB and save system prompt + user message
        if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
            save_initial_messages(db, conv_id, &provider_id_owned, &prompt_owned, get_cloud_system_prompt());
        }

        // Add current user message (multimodal if images are present)
        if image_data_owned.is_empty() {
            messages.push(json!({"role": "user", "content": prompt_owned}));
        } else {
            let mut parts: Vec<Value> = vec![json!({"type": "text", "text": prompt_owned})];
            for b64 in &image_data_owned {
                // Detect image format from base64 prefix (data URLs) or default to jpeg
                let url = if b64.starts_with("data:") {
                    b64.clone()
                } else {
                    format!("data:image/jpeg;base64,{b64}")
                };
                parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
            }
            messages.push(json!({"role": "user", "content": parts}));
        }

        // Track total tokens across all iterations
        let mut counters = LoopCounters {
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cached_tokens: 0,
            total_reasoning_tokens: 0,
            actual_model: None,
            final_stop_reason: "end_turn".to_string(),
        };
        let mut last_tool_name = String::new();
        let mut same_tool_count = 0u32;

        'agent_loop: for iteration in 0..max_iterations {
            provider_log(&conv_id_owned, "provider_iteration",
                &format!("iteration {}/{} messages={}", iteration + 1, max_iterations, messages.len()));

            // Update status for frontend polling (after reconnect)
            if iteration > 0 {
                set_remote_status(Some(format!(
                    "Agent loop #{} · {}in/{}out tokens",
                    iteration + 1, counters.total_input_tokens, counters.total_output_tokens
                )));
            }

            // Check for queued user messages (injected mid-generation via UI)
            if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                let queued = db.pop_queued_messages(conv_id);
                for msg in &queued {
                    provider_log(&conv_id_owned, "queued_message", &format!("injecting user message: {}...", &msg[..msg.len().min(80)]));
                    messages.push(json!({"role": "user", "content": msg}));
                    save_message_now(db, conv_id, "user", msg);
                    let _ = tx.send(CliTokenData {
                        token: format!("\n\n**[User message injected]**: {msg}\n\n"),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: counters.actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                    });
                }
            }

            // Build request body — use budget-trimmed copy if approaching token limit
            let api_messages = if counters.total_input_tokens > MAX_INPUT_TOKENS * 80 / 100 {
                budget_trimmed_messages(&messages)
            } else {
                messages.clone()
            };
            let mut body = json!({
                "model": model_name_clone,
                "messages": api_messages,
                "stream": true,
                "stream_options": {"include_usage": true},
            });

            // Apply provider-specific default parameters, then user overrides
            let defaults = provider_default_params(&provider_id_owned);
            if let Some(temp) = defaults.get("temperature") {
                body["temperature"] = temp.clone();
            }
            if let Some(max) = defaults.get("max_tokens") {
                body["max_tokens"] = max.clone();
            }
            if let Some(ref params) = user_params_owned {
                if let Some(obj) = params.as_object() {
                    for (key, val) in obj {
                        apply_user_param(&provider_id_owned, &mut body, key, val);
                    }
                }
            }

            if has_tools {
                body["tools"] = json!(tools);
            }

            // Make the API call — retry on 429 / 5xx with exponential backoff
            let call_result = {
                let mut attempt = 0u32;
                const MAX_RETRIES: u32 = 2; // up to 3 total attempts
                loop {
                    match stream_sse_response(&url, &api_key_owned, &body, &tx, &counters.actual_model) {
                        Ok(r) => break Ok(r),
                        Err(ref e) if attempt < MAX_RETRIES && is_retryable_http_error(e) => {
                            let wait_secs = 2u64 << attempt; // 2 s, 4 s
                            eprintln!(
                                "[OPENAI_COMPAT] Retryable error (attempt {}/{MAX_RETRIES}): {e}, retrying in {wait_secs}s",
                                attempt + 1
                            );
                            provider_log(
                                &conv_id_owned,
                                "provider_retry",
                                &format!("attempt {}/{MAX_RETRIES}: {e}, waiting {wait_secs}s", attempt + 1),
                            );
                            let _ = tx.send(CliTokenData {
                                token: format!(
                                    "\n*[Provider error — retrying in {wait_secs}s… attempt {}/{}]*\n",
                                    attempt + 2,
                                    MAX_RETRIES + 1
                                ),
                                is_done: false, session_id: None, stop_reason: None,
                                cost_usd: None, duration_ms: None,
                                model_id: Some(model_name_clone.clone()),
                                input_tokens: None, output_tokens: None, cached_tokens: None,
                            });
                            std::thread::sleep(std::time::Duration::from_secs(wait_secs));
                            attempt += 1;
                        }
                        Err(e) => break Err(e),
                    }
                }
            };
            let result = match call_result {
                Ok(r) => r,
                Err(error_msg) => {
                    provider_log(&conv_id_owned, "provider_error",
                        &format!("iteration {}: {error_msg}", iteration + 1));

                    if iteration == 0 && error_msg.contains("reasoning_content") {
                        eprintln!("[OPENAI_COMPAT] reasoning_content error — retrying with fields stripped");
                        let mut body_retry = body.clone();
                        if let Some(msgs) = body_retry["messages"].as_array_mut() {
                            for m in msgs.iter_mut() {
                                if m.get("reasoning_content").is_some() {
                                    m.as_object_mut().map(|o| o.remove("reasoning_content"));
                                }
                            }
                        }
                        match stream_sse_response(&url, &api_key_owned, &body_retry, &tx, &counters.actual_model) {
                            Ok(r) => {
                                if let Some(m) = r.actual_model { counters.actual_model = Some(m); }
                                if let Some(it) = r.input_tokens { counters.total_input_tokens += it; }
                                if let Some(ot) = r.output_tokens { counters.total_output_tokens += ot; }
                                counters.final_stop_reason = r.finish_reason.unwrap_or_else(|| "stop".to_string());
                            }
                            Err(retry_err) => {
                                let _ = tx.send(CliTokenData {
                                    token: format!("\n**Error:** {retry_err}"),
                                    is_done: false, session_id: None, stop_reason: None,
                                    cost_usd: None, duration_ms: None,
                                    model_id: Some(model_name_clone.clone()),
                                    input_tokens: None, output_tokens: None, cached_tokens: None,
                                });
                                counters.final_stop_reason = "error".to_string();
                            }
                        }
                        break 'agent_loop;
                    }

                    let hint = http_error_hint(&error_msg);
                    let _ = tx.send(CliTokenData {
                        token: format!("\n**Error:** {error_msg}{hint}"),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: Some(model_name_clone.clone()),
                        input_tokens: None, output_tokens: None, cached_tokens: None,
                    });
                    counters.final_stop_reason = "error".to_string();
                    break 'agent_loop;
                }
            };

            // Update token tracking
            if result.actual_model.is_some() { counters.actual_model = result.actual_model; }
            if let Some(it) = result.input_tokens { counters.total_input_tokens += it; }
            if let Some(ot) = result.output_tokens { counters.total_output_tokens += ot; }
            if let Some(ct) = result.cached_tokens { counters.total_cached_tokens = ct; }
            if let Some(rt) = result.reasoning_tokens { counters.total_reasoning_tokens += rt; }

            if let Some(ref reason) = result.finish_reason {
                counters.final_stop_reason = reason.clone();
                if reason == "insufficient_system_resource" {
                    let _ = tx.send(CliTokenData {
                        token: "\n\n**[Provider ran out of resources. Try again later or use a smaller model.]**".to_string(),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: counters.actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                    });
                    break 'agent_loop;
                }
                if reason == "content_filter" {
                    let _ = tx.send(CliTokenData {
                        token: "\n\n**[Response filtered by provider's content policy.]**".to_string(),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: counters.actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                    });
                    break 'agent_loop;
                }
            }

            // Send cumulative token tracking status after each iteration
            let _ = tx.send(CliTokenData {
                token: String::new(),
                is_done: false,
                session_id: None,
                stop_reason: None,
                cost_usd: None,
                duration_ms: Some(start.elapsed().as_millis() as u64),
                model_id: counters.actual_model.clone(),
                input_tokens: Some(counters.total_input_tokens),
                output_tokens: Some(counters.total_output_tokens),
                cached_tokens: if counters.total_cached_tokens > 0 { Some(counters.total_cached_tokens) } else { None },
            });

            // If no tool calls, save final assistant response and exit
            if result.tool_calls.is_empty() {
                if !result.content.is_empty() {
                    if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                        let seq = save_message_now_returning_seq(db, conv_id, "assistant", &result.content);
                        // Write a single text part for this message
                        let parts_json = serde_json::json!([{"type": "text", "content": result.content}]).to_string();
                        let _ = db.update_message_parts(conv_id, seq, &parts_json);
                    }
                }
                provider_log(&conv_id_owned, "provider_done",
                    &format!("no tool calls, finish_reason={:?} after iteration {}", result.finish_reason, iteration + 1));
                break 'agent_loop;
            }

            // --- Tool calls detected: execute them and loop ---
            let tc_names: Vec<&str> = result.tool_calls.iter().map(|tc| tc.name.as_str()).collect();
            provider_log(&conv_id_owned, "tool_call",
                &format!("{} tool call(s): {} finish_reason={:?}", result.tool_calls.len(), tc_names.join(", "), result.finish_reason));

            // Loop detection: if same tool+args called N times in a row, stop
            let current_tool = result.tool_calls.iter()
                .map(|tc| format!("{}({})", tc.name, tc.arguments.chars().take(100).collect::<String>()))
                .collect::<Vec<_>>()
                .join(",");
            if current_tool == last_tool_name {
                same_tool_count += 1;
                if same_tool_count == loop_limit.saturating_sub(1) && loop_limit > 2 {
                    messages.push(json!({
                        "role": "system",
                        "content": format!("WARNING: You have called the same tool ({}) {} times in a row. You will be stopped after one more identical call. Try a completely different approach.",
                            result.tool_calls.iter().map(|tc| tc.name.as_str()).collect::<Vec<_>>().join(", "),
                            same_tool_count)
                    }));
                }
                if same_tool_count >= loop_limit {
                    provider_log(&conv_id_owned, "provider_error",
                        &format!("Loop detected: {current_tool} called {same_tool_count} times in a row, stopping"));
                    let _ = tx.send(CliTokenData {
                        token: format!("\n\n*Loop detected: tool called {same_tool_count} times in a row. Send a message to continue with a different approach.*"),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: counters.actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                    });
                    counters.final_stop_reason = "infinite_loop".to_string();
                    break 'agent_loop;
                }
            } else {
                last_tool_name = current_tool;
                same_tool_count = 1;
            }

            // Build assistant message with tool_calls
            let tc_json: Vec<Value> = result.tool_calls.iter()
                .map(|tc| json!({"id": tc.id, "type": "function", "function": {"name": tc.name, "arguments": tc.arguments}}))
                .collect();
            let mut assistant_msg = if result.content.is_empty() {
                json!({"role": "assistant", "content": null, "tool_calls": tc_json})
            } else {
                json!({"role": "assistant", "content": result.content, "tool_calls": tc_json})
            };
            if let Some(ref rc) = result.reasoning_content {
                assistant_msg["reasoning_content"] = json!(rc);
            }
            messages.push(assistant_msg.clone());

            // Save assistant tool_call message to DB, capture seq for parts update
            let assistant_seq: Option<(String, i32)> =
                if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                    let mut stored = json!({"tool_calls": assistant_msg.get("tool_calls"), "content": assistant_msg.get("content")});
                    if let Some(rc) = assistant_msg.get("reasoning_content") { stored["reasoning_content"] = rc.clone(); }
                    let seq = save_message_now_returning_seq(db, conv_id, "assistant", &stored.to_string());
                    Some((conv_id.clone(), seq))
                } else {
                    None
                };

            // Execute tool calls
            let tool_results: Vec<(String, String, String, u64)> = result.tool_calls.iter().map(|tc| {
                let args_display = if tc.arguments.is_empty() { "{}".to_string() } else { tc.arguments.clone() };
                let _ = tx.send(CliTokenData {
                    token: format!("\n<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>\n", tc.name, args_display),
                    is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                    duration_ms: None, model_id: counters.actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                });
                eprintln!("[OPENAI_COMPAT] Executing tool: {} args={}", tc.name, &tc.arguments.chars().take(200).collect::<String>());
                let tool_start = std::time::Instant::now();
                let result_text = execute_openai_tool(&tc.name, &tc.arguments, db_owned.as_ref(), mcp_ops);
                let tool_duration_ms = tool_start.elapsed().as_millis() as u64;
                let safe = if result_text.len() > 50_000 {
                    format!("{}\n\n[... truncated at 50KB, total {} bytes]", &result_text[..50_000], result_text.len())
                } else {
                    result_text
                };
                let summarized = summarize_tool_output(&safe, &tc.name, &url, &api_key_owned, &model_name_clone);
                (tc.id.clone(), tc.name.clone(), summarized, tool_duration_ms)
            }).collect();

            // Display results, save to DB, add to messages
            for (id, name, truncated, duration_ms) in &tool_results {
                let _ = tx.send(CliTokenData {
                    token: format!("\n<tool_response>{}</tool_response>\n", &truncated[..truncated.len().min(2000)]),
                    is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                    duration_ms: None, model_id: counters.actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                });
                messages.push(json!({"role": "tool", "tool_call_id": id, "content": truncated}));
                if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                    save_message_now(db, conv_id, "tool", &format!("{id}\n\n{truncated}"));
                    llama_chat_db::event_log::log_event(
                        conv_id,
                        "tool_timing",
                        &format!("{{\"name\":\"{name}\",\"duration_ms\":{duration_ms}}}"),
                    );
                }
            }

            // Write structured parts onto the assistant message now that we have tool results
            if let (Some((conv_id, seq)), Some(ref db)) = (&assistant_seq, &db_owned) {
                let mut parts: Vec<serde_json::Value> = Vec::new();
                if !result.content.is_empty() {
                    parts.push(json!({"type": "text", "content": result.content}));
                }
                for tc in &result.tool_calls {
                    parts.push(json!({"type": "tool_call", "content": "", "tool_name": tc.name, "tool_args": tc.arguments}));
                }
                for (_, name, truncated, _) in &tool_results {
                    parts.push(json!({"type": "tool_result", "content": &truncated[..truncated.len().min(4000)], "tool_name": name}));
                }
                if !parts.is_empty() {
                    if let Ok(parts_json) = serde_json::to_string(&parts) {
                        let _ = db.update_message_parts(conv_id, *seq, &parts_json);
                    }
                }
            }

            provider_log(&conv_id_owned, "tool_results",
                &format!("{} tool result(s) added, continuing loop", tool_results.len()));

            if tx.is_closed() {
                provider_log(&conv_id_owned, "provider_abort", "frontend disconnected after tool execution");
                break 'agent_loop;
            }
        }

        // Notify user if iteration limit was hit
        if counters.final_stop_reason == "tool_calls" {
            let _ = tx.send(CliTokenData {
                token: "\n\n*Tool call safe limit reached. Send another message to continue.*".to_string(),
                is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                duration_ms: None, model_id: counters.actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
            });
        }

        finalize_generation(&tx, &counters, start, &provider_id_owned, &model_name_clone,
            &url, &api_key_owned, &prompt_owned, &conv_id_owned, &db_owned, &messages);
    });

    Ok(rx)
}

/// Returns true for errors that are safe to retry (rate-limit or server errors).
/// Only matches errors that occur before any streaming starts (i.e. HTTP status errors),
/// so we never duplicate tokens that were already sent to the frontend.
fn is_retryable_http_error(msg: &str) -> bool {
    msg.starts_with("HTTP 429") || (msg.starts_with("HTTP 5") && msg.len() > 7)
}

/// Map HTTP error codes to user-friendly hints.
fn http_error_hint(error_msg: &str) -> &'static str {
    if error_msg.contains("429") || error_msg.contains("rate_limit") {
        "\nHint: Rate limit reached. Wait a moment and try again, or use a different provider."
    } else if error_msg.contains("401") || error_msg.contains("403") {
        "\nHint: Authentication failed. Check your API key in Settings."
    } else if error_msg.contains("404") {
        "\nHint: Model not found. The model ID may have changed — try a different model."
    } else if error_msg.contains("Connection") || error_msg.contains("tls") {
        "\nHint: Connection failed. The provider may be down or unreachable."
    } else {
        ""
    }
}
