use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

use llama_chat_command::{execute_command_streaming, strip_ansi_codes};
use llama_chat_command::background::execute_command_background;
use llama_chat_types::*;

use crate::loop_detection;
use crate::sub_agent::{run_sub_agent, try_extract_spawn_agent};
use crate::tool_dispatch::{
    rtk_prefix, rtk_prefix_for_tool,
    detect_destructive_command,
    detect_command_injection,
    run_native_tool_with_timeout,
};
use crate::tool_tags::ToolTags;
use super::output_assembly::tool_use_one_liner_pub;

/// Result of single tool execution: (text_output, image_bytes, image_summary_prompt)
pub type SingleToolResult = (String, Vec<Vec<u8>>, Option<String>);

/// Execute a single tool call and return (output_text, images).
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_single_call(
    command_text: &str,
    tool_name_for_log: &str,
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
    tags: &ToolTags,
    recent_commands: &mut Vec<String>,
) -> SingleToolResult {
    let mut all_images: Vec<Vec<u8>> = Vec::new();

    // Check for spawn_agent first — needs model/backend access, can't go through native tool path
    let output = if let Some(agent_result) = try_extract_spawn_agent(command_text) {
        let (task, extra_context) = agent_result;
        if task.is_empty() {
            "Error: 'task' argument is required for spawn_agent".to_string()
        } else {
            match run_sub_agent(
                model, backend, &task, extra_context.as_deref(), chat_template_string,
                conversation_id, tags,
                use_htmd, browser_backend, mcp_manager.clone(), db.clone(),
                token_sender,
            ) {
                Ok(result) => result,
                Err(e) => format!("Sub-agent error: {e}"),
            }
        }
    }
    // Check if this is an `execute_command` tool call
    else if let Some((cmd, is_background)) = llama_chat_tools::extract_execute_command_with_opts(command_text) {
        // Security checks
        if let Some(injection_msg) = detect_command_injection(&cmd) {
            injection_msg
        } else {
            if let Some(warning) = detect_destructive_command(&cmd) {
                eprintln!("[SECURITY] {}: {}", warning, &cmd[..cmd.len().min(100)]);
                llama_chat_db::event_log::log_event(conversation_id, "security_warning", &format!("{}: {}", warning, &cmd[..cmd.len().min(80)]));
            }

            let rtk_cmd = rtk_prefix_for_tool(&cmd);
            if is_background {
                log_info!(conversation_id, "🐚 Background execute_command: {}", rtk_cmd);
                let sender_clone = token_sender.clone();
                execute_command_background(&rtk_cmd, |line| {
                    if let Some(ref sender) = sender_clone {
                        let _ = sender.send(TokenData {
                            token: format!("{}\n", strip_ansi_codes(line)),
                            tokens_used: token_pos,
                            max_tokens: context_size as i32,
                            status: None,
                            ..Default::default()
                        });
                    }
                })
            } else {
                log_info!(conversation_id, "🐚 Streaming execute_command: {}", rtk_cmd);
                llama_chat_db::event_log::log_event(conversation_id, "tool_exec", &format!("execute_command: {}", &rtk_cmd[..rtk_cmd.len().min(100)]));
                let exec_start = std::time::Instant::now();
                let sender_clone = token_sender.clone();
                let result = execute_command_streaming(&rtk_cmd, cancel.clone(), |line| {
                    if let Some(ref sender) = sender_clone {
                        let _ = sender.send(TokenData {
                            token: format!("{}\n", strip_ansi_codes(line)),
                            tokens_used: token_pos,
                            max_tokens: context_size as i32,
                            status: None,
                            ..Default::default()
                        });
                    }
                });
                let elapsed_ms = exec_start.elapsed().as_millis();
                let one_liner = tool_use_one_liner_pub("execute_command", &cmd[..cmd.len().min(60)], &result, elapsed_ms as u64);
                llama_chat_db::event_log::log_event(conversation_id, "tool_done", &one_liner);
                llama_chat_db::event_log::log_event(
                    conversation_id,
                    "tool_timing",
                    &format!("{{\"name\":\"execute_command\",\"duration_ms\":{elapsed_ms}}}"),
                );
                if let Some(ref sender) = token_sender {
                    let _ = sender.send(TokenData {
                        tool_timing: Some(ToolTimingLive {
                            name: "execute_command".to_string(),
                            duration_ms: elapsed_ms as u64,
                        }),
                        ..Default::default()
                    });
                }
                result
            }
        }
    } else {
        // Native tool path
        let native_start = std::time::Instant::now();
        let native_option = run_native_tool_with_timeout(
            command_text,
            conversation_id,
            use_htmd,
            browser_backend.clone(),
            mcp_manager.clone(),
            db.clone(),
        );
        let native_duration_ms = native_start.elapsed().as_millis() as u64;
        if let Some(native_result) = native_option {
            let one_liner = tool_use_one_liner_pub(tool_name_for_log, "", &native_result.text, native_duration_ms);
            log_info!(conversation_id, "📦 Native tool result: {}", one_liner);
            llama_chat_db::event_log::log_event(conversation_id, "tool_done", &one_liner);
            llama_chat_db::event_log::log_event(
                conversation_id,
                "tool_timing",
                &format!("{{\"name\":\"{tool_name_for_log}\",\"duration_ms\":{native_duration_ms}}}"),
            );
            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    tool_timing: Some(ToolTimingLive {
                        name: tool_name_for_log.to_string(),
                        duration_ms: native_duration_ms,
                    }),
                    ..Default::default()
                });
            }

            // After a successful file write or edit, clear compile/execute entries from
            // the loop-detection window.
            if matches!(tool_name_for_log, "write_file" | "edit_file") {
                loop_detection::reset_after_write(recent_commands);
            }

            // Check if tool result is successful using sub-agent.
            let skip_check = matches!(tool_name_for_log,
                "browser_scroll" | "browser_close" | "browser_press_key" | "open_url"
            );
            let result_status = if skip_check || native_result.text.len() < 20 {
                ""
            } else {
                let check_text = if native_result.text.len() > 500 {
                    let mut end = 500;
                    while end < native_result.text.len() && !native_result.text.is_char_boundary(end) { end += 1; }
                    &native_result.text[..end]
                } else {
                    &native_result.text
                };
                let is_ok = crate::sub_checks::quick_tool_result_check(
                    model, backend, chat_template_string, conversation_id,
                    tool_name_for_log, check_text,
                );
                if is_ok { "success" } else { "error" }
            };

            if let Some(ref sender) = token_sender {
                let prefix = if result_status.is_empty() {
                    String::new()
                } else {
                    format!("[TOOL_RESULT:{result_status}]")
                };
                let _ = sender.send(TokenData {
                    token: format!("{}{}", prefix, native_result.text.trim()),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32,
                    status: None,
                    ..Default::default()
                });
            }
            all_images.extend(native_result.images);
            if result_status.is_empty() {
                native_result.text
            } else {
                format!("[TOOL_RESULT:{}]{}", result_status, native_result.text)
            }
        } else {
            let trimmed_cmd = command_text.trim();
            if trimmed_cmd.starts_with('{') || trimmed_cmd.starts_with('[') {
                log_info!(conversation_id, "⚠️ JSON-like tool call failed to parse, returning error to model");
                let err_msg = "Error: Failed to parse tool call JSON. The JSON may be malformed (check for unescaped backslashes, missing braces, or literal newlines in strings). Please try the execute_command tool to write files instead.".to_string();
                if let Some(ref sender) = token_sender {
                    let _ = sender.send(TokenData {
                        token: err_msg.clone(),
                        tokens_used: token_pos,
                        max_tokens: context_size as i32,
                        status: None,
                        ..Default::default()
                    });
                }
                err_msg
            } else {
                log_info!(conversation_id, "🐚 Falling back to streaming shell execution");
                let rtk_cmd = rtk_prefix(command_text);
                let sender_clone = token_sender.clone();
                execute_command_streaming(&rtk_cmd, cancel.clone(), |line| {
                    if let Some(ref sender) = sender_clone {
                        let _ = sender.send(TokenData {
                            token: format!("{}\n", strip_ansi_codes(line)),
                            tokens_used: token_pos,
                            max_tokens: context_size as i32,
                            status: None,
                            ..Default::default()
                        });
                    }
                })
            }
        }
    };

    let image_summary_prompt = if all_images.is_empty() {
        None
    } else {
        super::output_assembly::extract_image_summary_prompt(command_text)
    };
    (output, all_images, image_summary_prompt)
}
