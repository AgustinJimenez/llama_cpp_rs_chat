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
    get_agentic_tools, get_cloud_system_prompt, provider_cost_per_million,
    provider_default_params, resolve_model, summarize_tool_output,
};
use crate::providers::openai_compat_streaming::stream_sse_response;
use crate::providers::{
    clear_remote_generating, set_remote_generating, set_remote_status, CliTokenData,
};
use super::db::{
    ensure_conversation_row, maybe_generate_title_after_response, provider_log,
    save_message_now,
};

use serde_json::{json, Value};
use tokio::sync::mpsc;

mod tests;

// ─── Main generate function with agentic loop ─────────────────────────────

/// Generate a response using an OpenAI-compatible API.
///
/// Streams SSE tokens from `POST {base_url}/chat/completions` and converts them
/// to `CliTokenData` events on the returned channel.
///
/// When the model returns tool calls, they are executed locally and the results
/// are fed back into the conversation for another API round-trip.
pub async fn generate(
    provider_id: &str,
    prompt: &str,
    model: Option<&str>,
    base_url: &str,
    api_key: &str,
    conversation_id: Option<&str>,
    db: Option<&llama_chat_db::SharedDatabase>,
    user_params: Option<&serde_json::Value>,
) -> Result<mpsc::UnboundedReceiver<CliTokenData>, String> {
    let (tx, rx) = mpsc::unbounded_channel();
    let model_name = resolve_model(provider_id, model);
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    eprintln!(
        "[OPENAI_COMPAT] generate() provider={} model={} url={}",
        provider_id, model_name, url
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

    // Use ureq in a blocking task for the streaming HTTP request + agentic loop
    tokio::task::spawn_blocking(move || {
        let start = std::time::Instant::now();

        // Track this generation so frontend can reconnect after refresh
        if let Some(ref cid) = conv_id_owned {
            set_remote_generating(cid, &provider_id_owned);
        }

        // Get tool definitions for the agentic loop
        let tools = get_agentic_tools();
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
            ensure_conversation_row(db, conv_id, &provider_id_owned);
            // Save system prompt on first turn so frontend can display it
            if db.get_messages(conv_id).map(|m| m.is_empty()).unwrap_or(true) {
                save_message_now(db, conv_id, "system", get_cloud_system_prompt());
            }
            save_message_now(db, conv_id, "user", &prompt_owned);
        }

        // Add current user message
        messages.push(json!({"role": "user", "content": prompt_owned}));

        // Track total tokens across all iterations
        let mut total_input_tokens: u64 = 0;
        let mut total_output_tokens: u64 = 0;
        let mut total_cached_tokens: u64 = 0;
        let mut total_reasoning_tokens: u64 = 0;
        let mut actual_model: Option<String> = None;
        let mut final_stop_reason = "end_turn".to_string();
        let mut last_tool_name = String::new();
        let mut same_tool_count = 0u32;

        for iteration in 0..max_iterations {
            provider_log(&conv_id_owned, "provider_iteration",
                &format!("iteration {}/{} messages={}", iteration + 1, max_iterations, messages.len()));

            // Update status for frontend polling (after reconnect)
            if iteration > 0 {
                set_remote_status(Some(format!(
                    "Agent loop #{} · {}in/{}out tokens",
                    iteration + 1, total_input_tokens, total_output_tokens
                )));
            }

            // Note: we intentionally do NOT break when tx.is_closed().
            // The frontend may have disconnected (page refresh, conversation switch)
            // but the agentic loop should continue — messages are saved to DB
            // incrementally, so the user can reconnect and see the results.

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
                        duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                    });
                }
            }

            // Build request body — use budget-trimmed copy if approaching token limit
            // to avoid mutating the original messages array (preserves prompt cache)
            let api_messages = if total_input_tokens > MAX_INPUT_TOKENS * 80 / 100 {
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

            // Apply user-configured parameters (override defaults)
            if let Some(ref params) = user_params_owned {
                if let Some(obj) = params.as_object() {
                    for (key, val) in obj {
                        apply_user_param(&provider_id_owned, &mut body, key, val);
                    }
                }
            }

            // Include tools only if we have them
            if has_tools {
                body["tools"] = json!(tools);
            }

            // Make the API call
            let result = match stream_sse_response(
                &url,
                &api_key_owned,
                &body,
                &tx,
                &actual_model,
            ) {
                Ok(r) => r,
                Err(error_msg) => {
                    provider_log(&conv_id_owned, "provider_error",
                        &format!("iteration {}: {error_msg}", iteration + 1));

                    // DeepSeek reasoning models require reasoning_content from previous turns.
                    // Strip only reasoning_content fields (not entire history) to preserve cache.
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
                        match stream_sse_response(&url, &api_key_owned, &body_retry, &tx, &actual_model) {
                            Ok(r) => {
                                if let Some(m) = r.actual_model { actual_model = Some(m); }
                                if let Some(it) = r.input_tokens { total_input_tokens += it; }
                                if let Some(ot) = r.output_tokens { total_output_tokens += ot; }
                                final_stop_reason = r.finish_reason.unwrap_or_else(|| "stop".to_string());
                            }
                            Err(retry_err) => {
                                let _ = tx.send(CliTokenData {
                                    token: format!("\n**Error:** {retry_err}"),
                                    is_done: false, session_id: None, stop_reason: None,
                                    cost_usd: None, duration_ms: None,
                                    model_id: Some(model_name_clone.clone()),
                                    input_tokens: None, output_tokens: None, cached_tokens: None,
                                });
                                final_stop_reason = "error".to_string();
                            }
                        }
                        break;
                    }

                    let hint = if error_msg.contains("429") || error_msg.contains("rate_limit") {
                        "\nHint: Rate limit reached. Wait a moment and try again, or use a different provider."
                    } else if error_msg.contains("401") || error_msg.contains("403") {
                        "\nHint: Authentication failed. Check your API key in Settings."
                    } else if error_msg.contains("404") {
                        "\nHint: Model not found. The model ID may have changed — try a different model."
                    } else if error_msg.contains("Connection") || error_msg.contains("tls") {
                        "\nHint: Connection failed. The provider may be down or unreachable."
                    } else {
                        ""
                    };
                    let _ = tx.send(CliTokenData {
                        token: format!("\n**Error:** {error_msg}{hint}"),
                        is_done: false,
                        session_id: None,
                        stop_reason: None,
                        cost_usd: None,
                        duration_ms: None,
                        model_id: Some(model_name_clone.clone()),
                        input_tokens: None,
                        output_tokens: None, cached_tokens: None,
                    });
                    final_stop_reason = "error".to_string();
                    break;
                }
            };

            // Update tracking
            if result.actual_model.is_some() {
                actual_model = result.actual_model;
            }
            if let Some(it) = result.input_tokens {
                total_input_tokens += it;
            }
            if let Some(ot) = result.output_tokens {
                total_output_tokens += ot;
            }
            if let Some(ct) = result.cached_tokens {
                total_cached_tokens = ct; // Last value (cumulative from API)
            }
            if let Some(rt) = result.reasoning_tokens {
                total_reasoning_tokens += rt;
            }
            if let Some(ref reason) = result.finish_reason {
                final_stop_reason = reason.clone();
                // Handle provider-specific finish reasons
                if reason == "insufficient_system_resource" {
                    let _ = tx.send(CliTokenData {
                        token: "\n\n**[Provider ran out of resources. Try again later or use a smaller model.]**".to_string(),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                    });
                    break;
                }
                if reason == "content_filter" {
                    let _ = tx.send(CliTokenData {
                        token: "\n\n**[Response filtered by provider's content policy.]**".to_string(),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                    });
                    break;
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
                model_id: actual_model.clone(),
                input_tokens: Some(total_input_tokens),
                output_tokens: Some(total_output_tokens),
                cached_tokens: if total_cached_tokens > 0 { Some(total_cached_tokens) } else { None },
            });

            // If no tool calls, we're done — save final assistant response and exit
            if result.tool_calls.is_empty() {
                if !result.content.is_empty() {
                    if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                        save_message_now(db, conv_id, "assistant", &result.content);
                    }
                }
                provider_log(&conv_id_owned, "provider_done",
                    &format!("no tool calls, finish_reason={:?} after iteration {}", result.finish_reason, iteration + 1));
                break;
            }

            // --- Tool calls detected: execute them and loop ---
            let tc_names: Vec<&str> = result.tool_calls.iter().map(|tc| tc.name.as_str()).collect();
            provider_log(&conv_id_owned, "tool_call",
                &format!("{} tool call(s): {} finish_reason={:?}", result.tool_calls.len(), tc_names.join(", "), result.finish_reason));

            // Loop detection: if same tool+args called 3+ times in a row, stop
            let current_tool = result.tool_calls.iter()
                .map(|tc| {
                    let args_short: String = tc.arguments.chars().take(100).collect();
                    format!("{}({})", tc.name, args_short)
                })
                .collect::<Vec<_>>()
                .join(",");
            if current_tool == last_tool_name {
                same_tool_count += 1;
                // Warning at n-1: inject system message so model knows
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
                        &format!("Loop detected: {} called {} times in a row, stopping", current_tool, same_tool_count));
                    let _ = tx.send(CliTokenData {
                        token: format!("\n\n*Loop detected: tool called {} times in a row. Send a message to continue with a different approach.*", same_tool_count),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                    });
                    final_stop_reason = "infinite_loop".to_string();
                    break;
                }
            } else {
                last_tool_name = current_tool;
                same_tool_count = 1;
            }

            // Build the assistant message with tool_calls for the conversation
            let tc_json: Vec<Value> = result
                .tool_calls
                .iter()
                .map(|tc| {
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments,
                        }
                    })
                })
                .collect();

            // Add assistant message (with content if any, plus tool_calls, plus reasoning_content for thinking models)
            let mut assistant_msg = if result.content.is_empty() {
                json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": tc_json,
                })
            } else {
                json!({
                    "role": "assistant",
                    "content": result.content,
                    "tool_calls": tc_json,
                })
            };
            // DeepSeek reasoning models require reasoning_content to be passed back in multi-turn
            if let Some(ref rc) = result.reasoning_content {
                assistant_msg["reasoning_content"] = json!(rc);
            }
            messages.push(assistant_msg.clone());

            // Save assistant tool_call message to DB (preserve reasoning_content for cache)
            if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                let mut stored = json!({
                    "tool_calls": assistant_msg.get("tool_calls"),
                    "content": assistant_msg.get("content"),
                });
                if let Some(rc) = assistant_msg.get("reasoning_content") {
                    stored["reasoning_content"] = rc.clone();
                }
                save_message_now(db, conv_id, "assistant", &stored.to_string());
            }

            // Execute tool calls — parallel if multiple, sequential if single
            let tool_results: Vec<(String, String, String, u64)> = result.tool_calls.iter().map(|tc| {
                let args_display = if tc.arguments.is_empty() { "{}".to_string() } else { tc.arguments.clone() };
                let _ = tx.send(CliTokenData {
                    token: format!("\n<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>\n", tc.name, args_display),
                    is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                    duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                });
                eprintln!(
                    "[OPENAI_COMPAT] Executing tool: {} args={}",
                    tc.name,
                    &tc.arguments.chars().take(200).collect::<String>()
                );
                let tool_start = std::time::Instant::now();
                let result_text = execute_openai_tool(&tc.name, &tc.arguments, db_owned.as_ref());
                let tool_duration_ms = tool_start.elapsed().as_millis() as u64;
                // Smart truncation: keep head + tail for large outputs, 50KB safety net
                let safe = if result_text.len() > 50_000 {
                    format!("{}\n\n[... truncated at 50KB, total {} bytes]", &result_text[..50_000], result_text.len())
                } else {
                    result_text
                };
                let summarized = summarize_tool_output(&safe, &tc.name, &url, &api_key_owned, &model_name_clone);
                (tc.id.clone(), tc.name.clone(), summarized, tool_duration_ms)
            }).collect();

            // Display results, save to DB, and add to messages
            for (id, name, truncated, duration_ms) in &tool_results {
                let response_display = format!(
                    "\n<tool_response>{}</tool_response>\n",
                    &truncated[..truncated.len().min(2000)]
                );
                let _ = tx.send(CliTokenData {
                    token: response_display,
                    is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                    duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
                });
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": truncated,
                }));
                // Save tool result and timing to DB
                if let (Some(conv_id), Some(ref _db)) = (&conv_id_owned, &db_owned) {
                    save_message_now(_db, conv_id, "tool", &format!("{id}\n\n{truncated}"));
                    llama_chat_db::event_log::log_event(
                        conv_id,
                        "tool_timing",
                        &format!("{{\"name\":\"{}\",\"duration_ms\":{}}}", name, duration_ms),
                    );
                }
            }

            provider_log(&conv_id_owned, "tool_results",
                &format!("{} tool result(s) added, continuing loop", tool_results.len()));

            // Check if frontend disconnected after tool execution
            if tx.is_closed() {
                provider_log(&conv_id_owned, "provider_abort", "frontend disconnected after tool execution");
                break;
            }

            // Note: budget trimming is applied at request time (line above),
            // never mutating the original messages array (preserves prompt cache).
        }

        // Notify user if iteration limit was hit
        if final_stop_reason == "tool_calls" {
            let _ = tx.send(CliTokenData {
                token: "\n\n*Tool call safe limit reached. Send another message to continue.*".to_string(),
                is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None, cached_tokens: None,
            });
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        provider_log(&conv_id_owned, "provider_complete",
            &format!("model={:?} stop={} duration={}ms tokens={}in/{}out",
                actual_model, final_stop_reason, duration_ms, total_input_tokens, total_output_tokens));

        // Compute cost estimate (cache-aware: cached tokens are discounted)
        let cost_usd = provider_cost_per_million(&provider_id_owned, &model_name_clone)
            .map(|(ic, oc, cache_discount)| {
                let uncached = total_input_tokens.saturating_sub(total_cached_tokens) as f64;
                let cached = total_cached_tokens as f64;
                let input_cost = (uncached * ic + cached * ic * cache_discount) / 1_000_000.0;
                let output_cost = total_output_tokens as f64 * oc / 1_000_000.0;
                input_cost + output_cost
            });
        if total_cached_tokens > 0 {
            provider_log(&conv_id_owned, "cache_stats",
                &format!("cached={} uncached={} reasoning={}", total_cached_tokens,
                    total_input_tokens.saturating_sub(total_cached_tokens), total_reasoning_tokens));
        }

        // Save timings on the last assistant message so stats persist after refresh
        if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
            let conn = db.connection();
            let last_asst_id: Option<String> = conn.query_row(
                "SELECT id FROM messages WHERE conversation_id = ?1 AND role = 'assistant' ORDER BY sequence_order DESC LIMIT 1",
                [conv_id],
                |row| row.get(0),
            ).ok();
            if let Some(msg_id) = last_asst_id {
                let gen_tok_per_sec = if duration_ms > 0 {
                    Some(total_output_tokens as f64 / (duration_ms as f64 / 1000.0))
                } else {
                    None
                };
                let _ = db.update_message_timings(
                    &msg_id,
                    None,
                    gen_tok_per_sec,
                    Some(duration_ms as f64),
                    Some(total_output_tokens as i32),
                    None,
                    Some(total_input_tokens as i32),
                );
            }
        }

        // Send done event
        let _ = tx.send(CliTokenData {
            token: String::new(),
            is_done: true,
            session_id: None,
            stop_reason: Some(final_stop_reason),
            cost_usd,
            duration_ms: Some(duration_ms),
            model_id: actual_model,
            input_tokens: if total_input_tokens > 0 {
                Some(total_input_tokens)
            } else {
                None
            },
            output_tokens: if total_output_tokens > 0 {
                Some(total_output_tokens)
            } else {
                None
            },
            cached_tokens: if total_cached_tokens > 0 { Some(total_cached_tokens) } else { None },
        });

        // Clear remote generation tracker
        clear_remote_generating();

        // Generate title after response is done (non-blocking, cheap API call)
        if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
            maybe_generate_title_after_response(
                conv_id,
                db,
                &messages,
                &prompt_owned,
                &url,
                &api_key_owned,
                &model_name_clone,
                &conv_id_owned,
            );
        }
    });

    Ok(rx)
}
