use llama_cpp_2::{llama_batch::LlamaBatch, model::AddBos};
use regex::Regex;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::super::command::execute_command_streaming;
use super::super::models::*;
use super::super::native_tools;
use super::tool_tags::ToolTags;
use crate::log_info;

// Default SYSTEM.EXEC regex (always tried as fallback)
// (?s) enables DOTALL mode so . matches newlines (multi-line commands)
// Closing tag: models may emit <SYSTEM.EXEC||> (correct) or <||SYSTEM.EXEC||> (mirrored opening)
lazy_static::lazy_static! {
    pub static ref EXEC_PATTERN: Regex = Regex::new(
        r"(?s)SYSTEM\.EXEC>(.+?)<(?:\|{1,2})?SYSTEM\.EXEC\|{1,2}>"
    ).unwrap();

    // Llama3/Hermes XML format: <function=tool_name> ... </function>
    // Some models (Qwen3-Coder) output this without a <tool_call> wrapper.
    static ref LLAMA3_FUNC_PATTERN: Regex = Regex::new(
        r"(?s)(<function=[a-z_]+>.*?</function>)"
    ).unwrap();

    // Harmony format (gpt-oss-20b): to= tool_name ... code<|message|>{...}<|call|>
    // Note: model may emit space after "to=" (e.g. "to= list_directory")
    static ref HARMONY_CALL_PATTERN: Regex = Regex::new(
        r"(?s)to=\s*(\w+)[\s\S]*?code<\|message\|>(.*?)<\|call\|>"
    ).unwrap();

    // Mistral v2 bracket format (Devstral-Small-2-2512):
    // [TOOL_CALLS]tool_name[ARGS]{"arg":"val"}
    // Only matches the prefix — JSON body is extracted via balanced-brace scanner
    // because non-greedy \{.*?\} fails on nested JSON (e.g. write_file with JSON content).
    static ref MISTRAL_BRACKET_PREFIX: Regex = Regex::new(
        r"\[TOOL_CALLS\](\w+)\[ARGS\]"
    ).unwrap();

    // Mistral JSON format (Magistral-Small-2509):
    // [TOOL_CALLS]{"name":"tool_name","arguments":{...}}
    // The [TOOL_CALLS] tag is followed directly by a JSON object (no name[ARGS] separator).
    static ref MISTRAL_JSON_PREFIX: Regex = Regex::new(
        r"\[TOOL_CALLS\]\s*\{"
    ).unwrap();
}

/// Extract balanced JSON starting at position `start` in `text`.
/// `text[start]` must be `{`. Respects string quoting so nested `{}`
/// inside JSON strings don't break the match.
/// Returns `(end_exclusive, json_slice)` on success.
fn extract_balanced_json(text: &str, start: usize) -> Option<(usize, String)> {
    let bytes = text.as_bytes();
    if start >= bytes.len() || bytes[start] != b'{' {
        return None;
    }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut prev_backslash = false;
    for (i, &b) in bytes[start..].iter().enumerate() {
        if in_string {
            if b == b'"' && !prev_backslash {
                in_string = false;
            }
            prev_backslash = b == b'\\' && !prev_backslash;
        } else {
            match b {
                b'"' => {
                    in_string = true;
                    prev_backslash = false;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = start + i + 1;
                        return Some((end, text[start..end].to_string()));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Build a regex that matches the model-specific exec tags.
/// Returns None if the tags are already covered by the default EXEC_PATTERN.
fn build_model_exec_regex(tags: &ToolTags) -> Option<Regex> {
    // Skip if using default SYSTEM.EXEC tags (already handled by EXEC_PATTERN)
    if tags.exec_open.contains("SYSTEM.EXEC") {
        return None;
    }

    // Escape special regex characters in the tags
    let open = regex::escape(&tags.exec_open);
    let close = regex::escape(&tags.exec_close);

    // Build pattern: open_tag(.+?)close_tag
    // (?s) enables DOTALL mode so . matches newlines (multi-line commands like python -c)
    let pattern = format!(r"(?s){open}(.+?){close}");
    Regex::new(&pattern).ok()
}

// --- Format detectors: each returns the extracted command text or None ---

type FormatDetector = fn(&str, &ToolTags) -> Option<String>;

/// Detector priority order. First match wins.
const FORMAT_PRIORITY: &[(&str, FormatDetector)] = &[
    ("model_specific", detect_model_specific),
    ("exec", detect_exec),
    ("llama3", detect_llama3),
    ("harmony", detect_harmony),
    ("mistral_bracket", detect_mistral_bracket),
    ("mistral_json", detect_mistral_json),
];

fn detect_model_specific(text: &str, tags: &ToolTags) -> Option<String> {
    let re = build_model_exec_regex(tags)?;
    re.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn detect_exec(text: &str, _tags: &ToolTags) -> Option<String> {
    EXEC_PATTERN.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn detect_llama3(text: &str, _tags: &ToolTags) -> Option<String> {
    LLAMA3_FUNC_PATTERN.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn detect_harmony(text: &str, _tags: &ToolTags) -> Option<String> {
    let caps = HARMONY_CALL_PATTERN.captures(text)?;
    let (tool_name, args_json) = (caps.get(1)?, caps.get(2)?);
    Some(format!(
        r#"{{"name":"{}","arguments":{}}}"#,
        tool_name.as_str(),
        args_json.as_str().trim()
    ))
}

fn detect_mistral_bracket(text: &str, _tags: &ToolTags) -> Option<String> {
    let caps = MISTRAL_BRACKET_PREFIX.captures(text)?;
    let tool_name = caps.get(1)?;
    let json_start = caps.get(0)?.end();
    let (_end, args_json) = extract_balanced_json(text, json_start)?;
    Some(format!(
        r#"{{"name":"{}","arguments":{}}}"#,
        tool_name.as_str(),
        args_json.trim()
    ))
}

fn detect_mistral_json(text: &str, _tags: &ToolTags) -> Option<String> {
    let m = MISTRAL_JSON_PREFIX.find(text)?;
    // The `{` is at the end of the match, so JSON starts at match.end() - 1
    let json_start = m.end() - 1;
    let (_end, json) = extract_balanced_json(text, json_start)?;
    // Validate it has the expected "name" and "arguments" fields
    let parsed: serde_json::Value = serde_json::from_str(&json).ok()?;
    if parsed.get("name").is_some() && parsed.get("arguments").is_some() {
        Some(json)
    } else {
        None
    }
}

/// Result of command execution
pub struct CommandExecutionResult {
    /// Display block for frontend/logging (just the output tags, no chat template wrapping)
    pub output_block: String,
    /// Tokens for model context injection (wrapped in chat template turn structure)
    pub model_tokens: Vec<i32>,
}

/// Maximum number of times the same command can be repeated before blocking.
const MAX_COMMAND_REPEATS: usize = 2;

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
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
    token_pos: i32,
    context_size: u32,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Option<CommandExecutionResult>, String> {
    // Only scan new content since last command execution
    let response_to_scan = if last_scan_pos < response.len() {
        &response[last_scan_pos..]
    } else {
        return Ok(None);
    };

    // Fast path: skip expensive regex checks unless we see a closing tag character.
    // Command blocks always end with '>' (SYSTEM.EXEC, </tool_call>, </function>)
    // or ']' ([/TOOL_CALLS], [ARGS]{...}) or '}' (JSON tool calls).
    // This avoids running 6 regex patterns on every single token.
    if !response_to_scan.contains('>')
        && !response_to_scan.contains(']')
        && !response_to_scan.ends_with('}')
    {
        return Ok(None);
    }

    // Try each format detector in priority order (first match wins)
    let command_text = {
        let mut found: Option<String> = None;
        for &(_name, detect) in FORMAT_PRIORITY {
            if let Some(cmd) = detect(response_to_scan, tags) {
                found = Some(cmd);
                break;
            }
        }
        match found {
            Some(cmd) => cmd,
            None => return Ok(None),
        }
    };

    log_info!(conversation_id, "🔧 Command detected: {}", command_text);

    // Loop detection: check if this command was recently executed
    let normalized_cmd = command_text.trim().to_string();
    let repeat_count = recent_commands.iter().filter(|c| *c == &normalized_cmd).count();
    if repeat_count >= MAX_COMMAND_REPEATS {
        log_info!(
            conversation_id,
            "🔁 Loop detected! Command repeated {} times: {}",
            repeat_count + 1,
            normalized_cmd
        );
        let output = format!(
            "LOOP DETECTED: You have already run this exact command {} times with the same result. \
             STOP repeating it. Try a COMPLETELY DIFFERENT approach, or explain to the user what is blocking you.",
            repeat_count
        );
        let output_open = format!("\n{}\n", tags.output_open);
        let output_close = format!("\n{}\n", tags.output_close);
        let output_block = format!("{}{}{}", output_open, output.trim(), output_close);
        let model_block = wrap_output_for_model(&output_block, template_type);
        let model_tokens = model
            .str_to_token(&model_block, AddBos::Never)
            .map_err(|e| format!("Tokenization of loop detection block failed: {e}"))?;
        return Ok(Some(CommandExecutionResult {
            output_block,
            model_tokens: model_tokens.iter().map(|t| t.0).collect(),
        }));
    }
    recent_commands.push(normalized_cmd);

    // Stream the output_open tag to frontend immediately so the UI shows the block
    let output_open = format!("\n{}\n", tags.output_open);
    let output_close = format!("\n{}\n", tags.output_close);

    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: output_open.clone(),
            tokens_used: token_pos,
            max_tokens: context_size as i32,
        });
    }

    // Check if this is an `execute_command` tool call — route through streaming path
    // so the UI shows line-by-line output for long-running commands (composer, npm, etc.)
    let output = if let Some(cmd) = native_tools::extract_execute_command(&command_text) {
        log_info!(conversation_id, "🐚 Streaming execute_command: {}", cmd);
        let sender_clone = token_sender.clone();
        execute_command_streaming(&cmd, cancel.clone(), |line| {
            if let Some(ref sender) = sender_clone {
                let _ = sender.send(TokenData {
                    token: format!("{line}\n"),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32,
                });
            }
        })
    } else if let Some(native_output) = native_tools::dispatch_native_tool(
        &command_text,
        web_search_provider,
        web_search_api_key,
    ) {
        log_info!(conversation_id, "📦 Dispatched to native tool handler");
        // Non-execute tools complete quickly, stream their output at once
        if let Some(ref sender) = token_sender {
            let _ = sender.send(TokenData {
                token: native_output.trim().to_string(),
                tokens_used: token_pos,
                max_tokens: context_size as i32,
            });
        }
        native_output
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
                    max_tokens: context_size as i32,
                });
            }
            err_msg
        } else {
            log_info!(conversation_id, "🐚 Falling back to streaming shell execution");
            // Use streaming execution — each line is sent to frontend as it arrives
            let sender_clone = token_sender.clone();
            execute_command_streaming(&command_text, cancel.clone(), |line| {
                if let Some(ref sender) = sender_clone {
                    let _ = sender.send(TokenData {
                        token: format!("{line}\n"),
                        tokens_used: token_pos,
                        max_tokens: context_size as i32,
                    });
                }
            })
        }
    };
    log_info!(
        conversation_id,
        "📤 Command output length: {} chars",
        output.len()
    );

    // Stream the output_close tag to frontend
    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: output_close.clone(),
            tokens_used: token_pos,
            max_tokens: context_size as i32,
        });
    }

    // Format the full output block for model injection and logging
    let output_block = format!("{}{}{}", output_open, output.trim(), output_close);

    // Build model injection block with chat template turn wrapping.
    // The model needs proper turn structure to know the tool response is from
    // a different role and that it should continue as assistant.
    let model_block = wrap_output_for_model(&output_block, template_type);
    log_info!(
        conversation_id,
        "🔄 Model injection block (template={:?}):\n{}",
        template_type,
        model_block
    );

    let model_tokens = model
        .str_to_token(&model_block, AddBos::Never)
        .map_err(|e| format!("Tokenization of model injection block failed: {e}"))?;

    Ok(Some(CommandExecutionResult {
        output_block,
        model_tokens: model_tokens.iter().map(|t| t.0).collect(),
    }))
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

        context
            .decode(batch)
            .map_err(|e| format!("Decode failed for command output: {e}"))?;

        *token_pos += chunk.len() as i32;
    }

    Ok(())
}

/// Wrap tool output in the model's chat template turn structure.
///
/// After the model generates a tool call, we need to:
/// 1. Close the assistant's turn
/// 2. Present the tool response as a separate turn (role varies by template)
/// 3. Re-open an assistant turn so the model continues naturally
///
/// Without this wrapping, the model sees raw tool output injected mid-turn
/// and gets confused (e.g., Qwen loops on `<|im_start|>` tokens).
///
/// The `output_block` already contains the output tags (e.g. `<tool_response>...</tool_response>`).
/// This function adds the surrounding chat template structure for model injection only.
/// The frontend continues to see the unwrapped `output_block`.
fn wrap_output_for_model(output_block: &str, template_type: Option<&str>) -> String {
    match template_type {
        Some("ChatML") => {
            // Qwen/ChatML: <|im_end|>\n<|im_start|>user\n...output...<|im_end|>\n<|im_start|>assistant\n
            format!(
                "<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                output_block
            )
        }
        Some("Llama3") => {
            // Llama 3: <|eot_id|><|start_header_id|>tool<|end_header_id|>\n\n...output...<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n
            format!(
                "<|eot_id|><|start_header_id|>tool<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n",
                output_block
            )
        }
        Some("Gemma") => {
            // Gemma: <end_of_turn>\n<start_of_turn>user\n...output...<end_of_turn>\n<start_of_turn>model\n
            format!(
                "<end_of_turn>\n<start_of_turn>user\n{}<end_of_turn>\n<start_of_turn>model\n",
                output_block
            )
        }
        Some("Harmony") => {
            // Harmony (gpt-oss-20b): Close assistant turn, inject tool result, re-open assistant analysis turn.
            // Using "analysis" channel (not "final") so the model continues reasoning and can make
            // more tool calls. If we re-open with "final", the model writes a user-facing summary
            // immediately instead of executing further steps.
            // output_block already contains <|start|>tool<|message|>...result...<|end|>
            format!(
                "<|end|>\n{}\n<|start|>assistant<|channel|>analysis<|message|>",
                output_block
            )
        }
        Some("GLM") => {
            // GLM-4 family: output_block contains <|observation|>\n{result}\n
            // Re-open assistant turn so model continues generating
            format!("{}\n<|assistant|>\n", output_block)
        }
        Some("Mistral") | _ => {
            // Mistral and default: output tags are sufficient, no extra turn wrapping needed.
            // Mistral's tool format is inline within the conversation flow.
            output_block.to_string()
        }
    }
}
