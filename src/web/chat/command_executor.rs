use llama_cpp_2::{llama_batch::LlamaBatch, model::AddBos};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::super::background::execute_command_background;
use super::super::command::{execute_command_streaming, sanitize_command_output, strip_ansi_codes};
use super::super::models::*;
use super::super::native_tools;
use super::loop_detection::{self, LoopCheckResult};
use super::sub_agent::{run_sub_agent, try_extract_spawn_agent};
use super::tool_parser::FORMAT_PRIORITY;
use super::tool_tags::ToolTags;
use crate::{log_info, log_debug, log_warn};

pub(crate) use super::tool_output::{
    run_summary_pass_public,
    run_summary_reusing_ctx,
    tool_use_one_liner,
    maybe_truncate_tool_output,
    maybe_summarize_tool_output,
    summarize_tool_output_with_prompt,
    wrap_output_for_model,
};

use super::tool_output::{
    SUMMARIZE_THRESHOLD,
    summarize_tool_output,
};

use super::tool_dispatch::{
    maybe_rtk_prefix,
    detect_destructive_command,
    detect_command_injection,
    run_native_tool_with_timeout,
    execute_single_tool,
    is_read_only_tool,
    MAX_PARALLEL_TOOLS,
};

/// Result of command execution
pub struct CommandExecutionResult {
    /// Display block for frontend/logging (just the output tags, no chat template wrapping)
    pub output_block: String,
    /// Tokens for model context injection (wrapped in chat template turn structure)
    pub model_tokens: Vec<i32>,
    /// The template-wrapped text used for model context injection.
    /// Needed by the vision path to re-tokenize with `<__media__>` markers via MtmdContext.
    #[allow(dead_code)]
    pub model_block: String,
    /// Raw image bytes from tool responses (e.g., screenshots) for vision pipeline injection.
    /// When non-empty AND the model has vision capability, these are fed as image embeddings
    /// instead of (or alongside) the text tokens.
    #[allow(dead_code)]
    pub response_images: Vec<Vec<u8>>,
}

/// Extract a string parameter from raw tool call text (JSON or XML format).
fn extract_param_string(command_text: &str, param_name: &str) -> Option<String> {
    // JSON: "param_name": "value"
    let json_pattern = format!("\"{}\":", param_name);
    if let Some(pos) = command_text.find(&json_pattern) {
        let rest = &command_text[pos + json_pattern.len()..];
        let rest = rest.trim_start();
        if rest.starts_with('"') {
            // Find closing quote (handle escaped quotes)
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
    // XML: <parameter=param_name>value</parameter>
    let xml_open = format!("={}>\n", param_name);
    if let Some(pos) = command_text.find(&xml_open) {
        let rest = &command_text[pos + xml_open.len()..];
        if let Some(end) = rest.find("</parameter>") {
            return Some(rest[..end].trim().to_string());
        }
    }
    // XML without newline
    let xml_open2 = format!("={}>", param_name);
    if let Some(pos) = command_text.find(&xml_open2) {
        let rest = &command_text[pos + xml_open2.len()..];
        if let Some(end) = rest.find("</") {
            return Some(rest[..end].trim().to_string());
        }
    }
    None
}

/// Check for and execute commands using model-specific tool tags.
pub fn check_and_execute_command_with_tags(
    response: &str,
    last_scan_pos: usize,
    conversation_id: &str,
    model: &llama_cpp_2::model::LlamaModel,
    tags: &ToolTags,
    template_type: Option<&str>,
    web_search_provider: Option<&str>,
    web_search_api_key: Option<&str>,
    recent_commands: &mut Vec<String>,
    consecutive_loop_blocks: &mut usize,
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
    token_pos: i32,
    context_size: u32,
    cancel: Option<Arc<AtomicBool>>,
    use_rtk: bool,
    use_htmd: bool,
    browser_backend: &crate::web::browser::BrowserBackend,
    mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
    db: crate::web::database::SharedDatabase,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
) -> Result<Option<CommandExecutionResult>, String> {
    // Only scan new content since last command execution
    let response_to_scan = if last_scan_pos < response.len() {
        // Adjust to char boundary to avoid panicking on multi-byte UTF-8
        let mut pos = last_scan_pos;
        while pos < response.len() && !response.is_char_boundary(pos) {
            pos += 1;
        }
        &response[pos..]
    } else {
        return Ok(None);
    };

    // Fast path: skip expensive regex checks unless we see a closing tag character.
    // Command blocks always end with '>' (SYSTEM.EXEC, </tool_call>, </function>)
    // or ']' ([/TOOL_CALLS], [ARGS]{...}) or '}' (JSON tool calls).
    // This avoids running 6 regex patterns on every single token.
    let has_gt = response_to_scan.contains('>');
    let has_bracket = response_to_scan.contains(']');
    let ends_brace = response_to_scan.ends_with('}');
    if !has_gt && !has_bracket && !ends_brace {
        return Ok(None);
    }

    // Try each format detector in priority order (first match wins)
    let command_text = {
        let mut found: Option<(&str, String)> = None;
        for &(name, detect) in FORMAT_PRIORITY {
            if let Some(cmd) = detect(response_to_scan, tags) {
                log_debug!(conversation_id, "tool_detect: format='{}' matched, cmd_len={}", name, cmd.len());
                found = Some((name, cmd));
                break;
            }
        }
        match found {
            Some((_name, cmd)) => cmd,
            None => {
                // Log when we have tag characters but no format matches — throttled to
                // avoid flooding logs (previously caused 12K+ lines during repetition loops).
                let slen = response_to_scan.len();
                if (slen <= 300 || slen % 500 == 0)
                    && (response_to_scan.contains("tool_call") || response_to_scan.contains("SYSTEM.EXEC"))
                {
                    log_debug!(
                        conversation_id,
                        "tool_detect: no format matched but tool-related text found. scan_len={}, tail={:?}",
                        slen,
                        {
                            let mut tail = slen.saturating_sub(200);
                            while tail > 0 && !response_to_scan.is_char_boundary(tail) { tail += 1; }
                            &response_to_scan[tail..]
                        }
                    );
                }
                return Ok(None);
            }
        }
    };

    log_info!(conversation_id, "🔧 Command detected: {}", command_text);

    // Extract tool name for logging
    let tool_name_for_log = {
        let lower = command_text.to_lowercase();
        if let Some(start) = lower.find("\"name\"") {
            let rest = &command_text[start..];
            if let Some(q1) = rest.find(':').and_then(|c| rest[c..].find('"').map(|q| c + q + 1)) {
                if let Some(q2) = rest[q1..].find('"') {
                    rest[q1..q1+q2].to_string()
                } else { "unknown".to_string() }
            } else { "unknown".to_string() }
        } else if let Some(start) = lower.find("<function=") {
            let rest = &command_text[start + 10..];
            rest.split('>').next().unwrap_or("unknown").to_string()
        } else { "unknown".to_string() }
    };
    crate::web::event_log::log_event(conversation_id, "tool_call", &format!("{} (cmd #{})", tool_name_for_log, recent_commands.len() + 1));

    // Loop detection: check if this command was recently executed
    match loop_detection::check_loop(&command_text, recent_commands, consecutive_loop_blocks, tags, template_type, model, conversation_id)? {
        LoopCheckResult::ForceStop(mut result) => {
            crate::web::event_log::log_event(conversation_id, "infinite_loop", &format!("Force-stop after {} consecutive blocks", consecutive_loop_blocks));
            result.output_block.push_str("\n[INFINITE_LOOP_DETECTED]\n");
            return Ok(Some(result));
        }
        LoopCheckResult::Blocked(result) => {
            crate::web::event_log::log_event(conversation_id, "loop_blocked", &format!("{} blocked (consecutive: {})", tool_name_for_log, consecutive_loop_blocks));
            return Ok(Some(result));
        }
        LoopCheckResult::Continue(fuzzy_warning) => {
            // Continue with execution; fuzzy_warning may be Some

            // Parse all tool calls from the command text (supports JSON arrays for batch calls)
            let all_calls = native_tools::try_parse_all_from_raw(&command_text);
            let is_batch = all_calls.len() > 1;

            if is_batch {
                log_info!(
                    conversation_id,
                    "📦 Batch tool call: {} tools detected",
                    all_calls.len()
                );
            }

            // Stream the output_open tag to frontend immediately so the UI shows the block
            let output_open = format!("\n{}\n", tags.output_open);
            let output_close = format!("\n{}\n", tags.output_close);

            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    token: output_open.clone(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32, status: None,
                    ..Default::default()
                });
            }

            // Collect images from tool responses for vision pipeline
            let mut all_response_images: Vec<Vec<u8>> = Vec::new();

            let output = if is_batch {
                // === Batch execution path: group consecutive read-only tools for parallel execution ===
                let mut combined_output = String::new();

                // Group consecutive tool calls by read-only vs write classification.
                // Consecutive read-only tools are executed in parallel; write tools are executed serially.
                let mut groups: Vec<(bool, Vec<usize>)> = Vec::new(); // (is_read_only, indices)
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

                // Pre-allocate result slots: (text, images)
                let mut results: Vec<Option<(String, Vec<Vec<u8>>)>> = vec![None; all_calls.len()];

                for (is_read_only, indices) in &groups {
                    if *is_read_only && indices.len() > 1 {
                        // Execute consecutive read-only tools in parallel via thread::scope
                        let parallel_count = indices.len().min(MAX_PARALLEL_TOOLS);
                        log_info!(
                            conversation_id,
                            "[BATCH] Executing {} read-only tools in parallel",
                            parallel_count
                        );

                        // Prepare owned data for threads
                        let thread_data: Vec<(usize, String, Option<String>, Option<String>, String)> = indices
                            .iter()
                            .take(MAX_PARALLEL_TOOLS)
                            .map(|&i| {
                                let (name, args) = &all_calls[i];
                                let single_json = serde_json::json!({"name": name, "arguments": args}).to_string();
                                let provider = web_search_provider.map(|s| s.to_string());
                                let api_key = web_search_api_key.map(|s| s.to_string());
                                let conv_id = conversation_id.to_string();
                                (i, single_json, provider, api_key, conv_id)
                            })
                            .collect();

                        std::thread::scope(|s| {
                            let handles: Vec<_> = thread_data
                                .iter()
                                .map(|(idx, json, provider, api_key, conv_id)| {
                                    let idx = *idx;
                                    let tool_name = all_calls[idx].0.clone();
                                    let mcp_clone = mcp_manager.clone();
                                    let backend_clone = browser_backend.clone();
                                    let db_clone = db.clone();
                                    s.spawn(move || {
                                        let result = run_native_tool_with_timeout(
                                            json,
                                            provider.as_deref(),
                                            api_key.as_deref(),
                                            conv_id,
                                            use_htmd,
                                            backend_clone,
                                            mcp_clone,
                                            db_clone,
                                        );
                                        let native_result = result.unwrap_or_else(|| {
                                            native_tools::NativeToolResult::text_only(
                                                format!("Error: Tool '{}' returned no output", tool_name)
                                            )
                                        });
                                        (idx, native_result.text, native_result.images)
                                    })
                                })
                                .collect();

                            for handle in handles {
                                if let Ok((idx, text, images)) = handle.join() {
                                    results[idx] = Some((text, images));
                                }
                            }
                        });

                        // Execute any overflow beyond MAX_PARALLEL_TOOLS serially
                        for &i in indices.iter().skip(MAX_PARALLEL_TOOLS) {
                            let (name, args) = &all_calls[i];
                            let single_json = serde_json::json!({"name": name, "arguments": args}).to_string();
                            let (tool_output, tool_images) = execute_single_tool(
                                name, args, &single_json,
                                conversation_id,
                                web_search_provider,
                                web_search_api_key,
                                token_sender,
                                token_pos,
                                context_size,
                                cancel.clone(),
                                use_rtk,
                                use_htmd,
                                browser_backend,
                                mcp_manager.clone(),
                                db.clone(),
                                model, backend, chat_template_string, tags,
                            );
                            results[i] = Some((tool_output, tool_images));
                        }
                    } else {
                        // Execute serially: write tools, single read-only tools, or mixed
                        for &i in indices {
                            let (name, args) = &all_calls[i];
                            let single_json = serde_json::json!({"name": name, "arguments": args}).to_string();
                            let (tool_output, tool_images) = execute_single_tool(
                                name, args, &single_json,
                                conversation_id,
                                web_search_provider,
                                web_search_api_key,
                                token_sender,
                                token_pos,
                                context_size,
                                cancel.clone(),
                                use_rtk,
                                use_htmd,
                                browser_backend,
                                mcp_manager.clone(),
                                db.clone(),
                                model, backend, chat_template_string, tags,
                            );
                            results[i] = Some((tool_output, tool_images));
                        }
                    }
                }

                // Merge results in original order, streaming to frontend
                for (i, (name, _args)) in all_calls.iter().enumerate() {
                    let header = format!("[Tool {}: {}]\n", i + 1, name);
                    if let Some(ref sender) = token_sender {
                        let _ = sender.send(TokenData {
                            token: header.clone(),
                            tokens_used: token_pos,
                            max_tokens: context_size as i32, status: None,
                            ..Default::default()
                        });
                    }
                    combined_output.push_str(&header);

                    let (tool_output, tool_images) = results[i].take().unwrap_or_default();
                    all_response_images.extend(tool_images);
                    // Check summary param: false → skip, string → custom prompt
                    let summary_val = _args.get("summary");
                    let summary_opt_out = summary_val
                        .map(|v| v.as_bool() == Some(false) || v.as_str() == Some("false"))
                        .unwrap_or(false);
                    let _custom_prompt = summary_val
                        .and_then(|v| v.as_str())
                        .filter(|s| *s != "true" && *s != "false" && s.len() > 3)
                        .map(|s| s.to_string());
                    // Summarize (or truncate as fallback) individual tool output
                    let tool_output = if summary_opt_out {
                        maybe_truncate_tool_output(&tool_output, name, conversation_id)
                    } else {
                        maybe_summarize_tool_output(&tool_output, name, model, backend, chat_template_string, conversation_id)
                    };
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
                            max_tokens: context_size as i32, status: None,
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
                                max_tokens: context_size as i32, status: None,
                                ..Default::default()
                            });
                        }
                    }
                }

                combined_output
            } else {
                // === Single execution path (existing logic) ===
                // Check for spawn_agent first — needs model/backend access, can't go through native tool path
                if let Some(agent_result) = try_extract_spawn_agent(&command_text) {
                    let (task, extra_context) = agent_result;
                    if task.is_empty() {
                        "Error: 'task' argument is required for spawn_agent".to_string()
                    } else {
                        match run_sub_agent(
                            model, backend, &task, extra_context.as_deref(), chat_template_string,
                            conversation_id, tags, web_search_provider, web_search_api_key,
                            use_rtk, use_htmd, browser_backend, mcp_manager.clone(), db.clone(),
                            token_sender,
                        ) {
                            Ok(result) => result,
                            Err(e) => format!("Sub-agent error: {}", e),
                        }
                    }
                }
                // Check if this is an `execute_command` tool call — route through streaming or background path
                // so the UI shows line-by-line output for long-running commands (composer, npm, etc.)
                else if let Some((cmd, is_background)) = native_tools::extract_execute_command_with_opts(&command_text) {
                    // Security checks
                    if let Some(injection_msg) = detect_command_injection(&cmd) {
                        injection_msg
                    } else {
                    if let Some(warning) = detect_destructive_command(&cmd) {
                        eprintln!("[SECURITY] {}: {}", warning, &cmd[..cmd.len().min(100)]);
                        crate::web::event_log::log_event(conversation_id, "security_warning", &format!("{}: {}", warning, &cmd[..cmd.len().min(80)]));
                    }

                    let rtk_cmd = maybe_rtk_prefix(&cmd, use_rtk);
                    if is_background {
                        log_info!(conversation_id, "🐚 Background execute_command: {}", rtk_cmd);
                        let sender_clone = token_sender.clone();
                        execute_command_background(&rtk_cmd, |line| {
                            if let Some(ref sender) = sender_clone {
                                let _ = sender.send(TokenData {
                                    token: format!("{}\n", strip_ansi_codes(line)),
                                    tokens_used: token_pos,
                                    max_tokens: context_size as i32, status: None,
                                    ..Default::default()
                                });
                            }
                        })
                    } else {
                        log_info!(conversation_id, "🐚 Streaming execute_command: {}", rtk_cmd);
                        crate::web::event_log::log_event(conversation_id, "tool_exec", &format!("execute_command: {}", &rtk_cmd[..rtk_cmd.len().min(100)]));
                        let exec_start = std::time::Instant::now();
                        let sender_clone = token_sender.clone();
                        let result = execute_command_streaming(&rtk_cmd, cancel.clone(), |line| {
                            if let Some(ref sender) = sender_clone {
                                let _ = sender.send(TokenData {
                                    token: format!("{}\n", strip_ansi_codes(line)),
                                    tokens_used: token_pos,
                                    max_tokens: context_size as i32, status: None,
                                    ..Default::default()
                                });
                            }
                        });
                        let elapsed_ms = exec_start.elapsed().as_millis();
                        let one_liner = tool_use_one_liner("execute_command", &cmd[..cmd.len().min(60)], &result, elapsed_ms as u64);
                        crate::web::event_log::log_event(conversation_id, "tool_done", &one_liner);
                        result
                    }
                    } // end security injection check else block
                } else if let Some(native_result) = run_native_tool_with_timeout(
                    &command_text,
                    web_search_provider,
                    web_search_api_key,
                    conversation_id,
                    use_htmd,
                    browser_backend.clone(),
                    mcp_manager.clone(),
                    db.clone(),
                ) {
                    let one_liner = tool_use_one_liner(&tool_name_for_log, "", &native_result.text, 0);
                    log_info!(conversation_id, "📦 Native tool result: {}", one_liner);
                    crate::web::event_log::log_event(conversation_id, "tool_done", &one_liner);

                    // Check if tool result is successful using sub-agent.
                    // Skip check for action tools that always succeed (navigate, scroll, etc.)
                    let skip_check = matches!(tool_name_for_log.as_str(),
                        "browser_scroll" | "browser_close" | "browser_press_key" | "open_url"
                    );
                    let result_status = if skip_check || native_result.text.len() < 20 {
                        "" // No indicator for action tools or very short output
                    } else {
                        let check_text = if native_result.text.len() > 500 {
                            let mut end = 500;
                            while end < native_result.text.len() && !native_result.text.is_char_boundary(end) { end += 1; }
                            &native_result.text[..end]
                        } else {
                            &native_result.text
                        };
                        let is_ok = super::generation::quick_tool_result_check(
                            model, backend, chat_template_string, conversation_id,
                            &tool_name_for_log, check_text,
                        );
                        if is_ok { "success" } else { "error" }
                    };

                    // Stream output with optional TOOL_RESULT status tag for frontend
                    if let Some(ref sender) = token_sender {
                        let prefix = if result_status.is_empty() {
                            String::new()
                        } else {
                            format!("[TOOL_RESULT:{}]", result_status)
                        };
                        let _ = sender.send(TokenData {
                            token: format!("{}{}", prefix, native_result.text.trim()),
                            tokens_used: token_pos,
                            max_tokens: context_size as i32, status: None,
                            ..Default::default()
                        });
                    }
                    all_response_images.extend(native_result.images);
                    // Prepend status tag so it persists in DB for reloaded conversations
                    if result_status.is_empty() {
                        native_result.text
                    } else {
                        format!("[TOOL_RESULT:{}]{}", result_status, native_result.text)
                    }
                } else {
                    let trimmed_cmd = command_text.trim();
                    if trimmed_cmd.starts_with('{') || trimmed_cmd.starts_with('[') {
                        // Looks like a JSON tool call that failed to parse — don't execute as shell.
                        log_info!(conversation_id, "⚠️ JSON-like tool call failed to parse, returning error to model");
                        let err_msg = "Error: Failed to parse tool call JSON. The JSON may be malformed (check for unescaped backslashes, missing braces, or literal newlines in strings). Please try the execute_command tool to write files instead.".to_string();
                        if let Some(ref sender) = token_sender {
                            let _ = sender.send(TokenData {
                                token: err_msg.clone(),
                                tokens_used: token_pos,
                                max_tokens: context_size as i32, status: None,
                                ..Default::default()
                            });
                        }
                        err_msg
                    } else {
                        log_info!(conversation_id, "🐚 Falling back to streaming shell execution");
                        // Use streaming execution — each line is sent to frontend as it arrives
                        let rtk_cmd = maybe_rtk_prefix(&command_text, use_rtk);
                        let sender_clone = token_sender.clone();
                        execute_command_streaming(&rtk_cmd, cancel.clone(), |line| {
                            if let Some(ref sender) = sender_clone {
                                let _ = sender.send(TokenData {
                                    token: format!("{}\n", strip_ansi_codes(line)),
                                    tokens_used: token_pos,
                                    max_tokens: context_size as i32, status: None,
                                    ..Default::default()
                                });
                            }
                        })
                    }
                }
            };
            log_info!(
                conversation_id,
                "📤 Command output length: {} chars",
                output.len()
            );

            // Sanitize output: strip ANSI codes + truncate long output.
            // Strip [TOOL_RESULT:...] tag before model sees it (tag is for frontend only)
            let output_for_model = if output.starts_with("[TOOL_RESULT:") {
                output.splitn(2, ']').nth(1).unwrap_or(&output).to_string()
            } else {
                output.clone()
            };
            let sanitized = sanitize_command_output(&output_for_model);

            // Smart-truncate large output before LLM summarization to bound context usage.
            let sanitized = maybe_truncate_tool_output(&sanitized, &tool_name_for_log, conversation_id);

            // Summarize large outputs via LLM sub-agent to save context tokens.
            // The user sees the original output (persisted in output_block);
            // the model only receives the summary (injected via model_block/model_tokens).
            // Use original output length to decide summarization — the sanitized version may
            // be heavily truncated but the user still sees the full streamed output.
            // Extract 'summary' param — can be boolean (true/false) or custom prompt string.
            // If false → skip summarization. If string → use as custom prompt.
            let summary_value = extract_param_string(&command_text, "summary");
            let cmd_lower = command_text.to_lowercase();
            let summary_disabled = summary_value.as_deref() == Some("false")
                || cmd_lower.contains("\"summary\": false")
                || cmd_lower.contains("\"summary\":false")
                || cmd_lower.contains("summary>\nfalse")
                || cmd_lower.contains("summary>false");
            let custom_summary_prompt = if summary_disabled {
                None
            } else {
                // If summary is a non-boolean string, use it as the custom prompt
                summary_value.filter(|s| s != "true" && s != "false" && s.len() > 3)
            };
            let (display_text, model_text) = if summary_disabled {
                // Model wants raw output — skip summarization
                (sanitized.clone(), sanitized)
            } else if output.len() > SUMMARIZE_THRESHOLD || sanitized.len() > SUMMARIZE_THRESHOLD {
                match if let Some(ref prompt) = custom_summary_prompt {
                    summarize_tool_output_with_prompt(model, backend, &sanitized, chat_template_string, conversation_id, Some(prompt))
                } else {
                    summarize_tool_output(model, backend, &sanitized, chat_template_string, conversation_id)
                } {
                    Ok(summary) => {
                        log_info!(conversation_id, "📝 Summarized tool output: {} → {} chars", sanitized.len(), summary.len());
                        // Stream summary with actual content to frontend (before output_close)
                        let summary_block = format!(
                            "\n\n📝 Summary for model ({} → {} chars):\n{}",
                            sanitized.len(), summary.len(), summary.trim()
                        );
                        if let Some(ref sender) = token_sender {
                            let _ = sender.send(TokenData {
                                token: summary_block.clone(),
                                tokens_used: token_pos,
                                max_tokens: context_size as i32, status: None,
                                ..Default::default()
                            });
                        }
                        // Display: original output + summary with content
                        // Model: summary with label (so model knows it's not raw output)
                        let display = format!("{}{}", sanitized, summary_block);
                        let model_summary = format!(
                            "[SUMMARIZED: {} → {} chars. Use summary=false to get raw output.]\n{}",
                            sanitized.len(), summary.len(), summary
                        );
                        (display, model_summary)
                    }
                    Err(e) => {
                        log_warn!(conversation_id, "Summarization failed ({}), using raw output", e);
                        (sanitized.clone(), sanitized)
                    }
                }
            } else {
                (sanitized.clone(), sanitized)
            };

            // Stream the output_close tag to frontend (after any summary note)
            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    token: output_close.clone(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32, status: None,
                    ..Default::default()
                });
            }

            // output_block: persisted in conversation — contains original output for user display
            let output_block = format!("{}{}{}", output_open, display_text.trim(), output_close);

            // Detect dead links / HTTP errors and hint the model to search online
            let model_trimmed = model_text.trim();
            let http_error_hint = loop_detection::detect_http_error_hint(model_trimmed);

            // Build model text with warnings
            let mut model_text_with_warning = model_trimmed.to_string();
            if let Some(ref warning) = fuzzy_warning {
                model_text_with_warning = format!("{}\n\n{}", warning, model_text_with_warning);
            }
            if let Some(hint) = http_error_hint {
                model_text_with_warning = format!("{}\n\n{}", model_text_with_warning, hint);
            }

            // model_injection_block: contains only the summary — this is what the LLM sees
            let model_injection_block = format!("{}{}{}", output_open, model_text_with_warning, output_close);

            // Build model injection block with chat template turn wrapping.
            // The model needs proper turn structure to know the tool response is from
            // a different role and that it should continue as assistant.
            let model_block = wrap_output_for_model(&model_injection_block, template_type);
            log_info!(
                conversation_id,
                "🔄 Model injection block (template={:?}):\n{}",
                template_type,
                model_block
            );

            let model_tokens = model
                .str_to_token(&model_block, AddBos::Never)
                .map_err(|e| format!("Tokenization of model injection block failed: {e}"))?;

            if !all_response_images.is_empty() {
                eprintln!(
                    "[TOOL_RESULT] {} image(s) for vision pipeline, sizes: {:?}",
                    all_response_images.len(),
                    all_response_images.iter().map(|img| img.len()).collect::<Vec<_>>()
                );
            }

            // Persist screenshot images to disk and append markdown links for frontend display.
            // Images are saved to assets/images/{conversation_id}/ and served via /api/images/ route.
            let mut output_block = output_block;
            if !all_response_images.is_empty() {
                let images_dir = std::path::PathBuf::from("assets/images").join(conversation_id);
                if let Err(e) = std::fs::create_dir_all(&images_dir) {
                    eprintln!("[IMAGES] Failed to create images dir: {e}");
                } else {
                    for (i, img_bytes) in all_response_images.iter().enumerate() {
                        let uuid = uuid::Uuid::new_v4();
                        let filename = format!("{uuid}.jpg");
                        let filepath = images_dir.join(&filename);
                        match std::fs::write(&filepath, img_bytes) {
                            Ok(()) => {
                                let img_url = format!("/api/images/{}/{}", conversation_id, filename);
                                let size_kb = img_bytes.len() / 1024;
                                eprintln!("[IMAGES] Saved screenshot {}/{}: {} ({}KB)", i + 1, all_response_images.len(), filepath.display(), size_kb);
                                // Append markdown image after the output close tag
                                output_block.push_str(&format!("\n![screenshot]({img_url})"));
                            }
                            Err(e) => {
                                eprintln!("[IMAGES] Failed to save screenshot: {e}");
                            }
                        }
                    }
                }
            }

            Ok(Some(CommandExecutionResult {
                output_block,
                model_tokens: model_tokens.iter().map(|t| t.0).collect(),
                model_block,
                response_images: all_response_images,
            }))
        }
    }
}

/// Inject command output tokens into the LLM context.
pub fn inject_output_tokens(
    tokens: &[i32],
    batch: &mut LlamaBatch<'_>,
    context: &mut llama_cpp_2::context::LlamaContext<'_>,
    token_pos: &mut i32,
    conversation_id: &str,
) -> Result<(), String> {
    use crate::log_debug;
    log_debug!(
        conversation_id,
        "Injecting {} output tokens into context",
        tokens.len()
    );

    // Decode in chunks for performance (single-token decode is extremely slow for large outputs)
    const INJECT_CHUNK_SIZE: usize = 512;
    for chunk in tokens.chunks(INJECT_CHUNK_SIZE) {
        batch.clear();

        for (idx, &token) in chunk.iter().enumerate() {
            let is_last = idx == chunk.len() - 1;
            batch
                .add(
                    llama_cpp_2::token::LlamaToken(token),
                    *token_pos + (idx as i32),
                    &[0],
                    is_last,
                )
                .map_err(|e| format!("Batch add failed for command output: {e}"))?;
        }

        if let Err(e) = context.decode(batch) {
            let err_str = format!("{e}");
            if err_str.contains("NoKvCacheSlot") || err_str.contains("no kv cache slot") {
                return Err("CONTEXT_EXHAUSTED".to_string());
            }
            return Err(format!("Decode failed for command output: {e}"));
        }

        *token_pos += chunk.len() as i32;
    }

    // Check if we've consumed too much context after injection
    // (catches recurrent/hybrid models where decode succeeds but context is full)
    let ctx_size = context.n_ctx();
    if *token_pos as u32 >= ctx_size.saturating_sub(ctx_size / 20) {
        eprintln!("[INJECT] Context 95% full after injection ({}/{})", token_pos, ctx_size);
        return Err("CONTEXT_EXHAUSTED".to_string());
    }

    Ok(())
}
