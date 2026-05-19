//! SSE/streaming response parsing for the OpenAI-compatible provider.
//!
//! Reads the `text/event-stream` response from `POST /chat/completions`
//! and accumulates tokens, tool-call deltas, and usage info into a
//! `StreamResult`.

use tokio::sync::mpsc;

use super::CliTokenData;
use super::openai_compat_types::{
    AccumulatedToolCall, ChatCompletionChunk, StreamResult,
};

/// Stream one SSE response from the API, sending text tokens to `tx` as they arrive.
/// Returns the accumulated result (content, tool calls, usage info).
pub(super) fn stream_sse_response(
    url: &str,
    api_key: &str,
    body: &serde_json::Value,
    tx: &mpsc::UnboundedSender<CliTokenData>,
    model_hint: &Option<String>,
) -> Result<StreamResult, String> {
    let body_str = serde_json::to_string(body)
        .map_err(|e| format!("Failed to serialize request: {e}"))?;

    // TLS retry: on transport/connection errors (NOT HTTP 4xx/5xx), retry once after 1s
    let resp = {
        #[allow(unused_assignments)]
        let mut last_err = String::new();
        let mut attempts = 0;
        loop {
            attempts += 1;
            match ureq::AgentBuilder::new()
                .timeout_connect(std::time::Duration::from_secs(30))
                .build()
                .post(url)
                .set("Content-Type", "application/json")
                .set("Authorization", &format!("Bearer {api_key}"))
                .set("Accept", "text/event-stream")
                .send_string(&body_str)
            {
                Ok(r) => break Ok(r),
                Err(ureq::Error::Status(code, resp)) => {
                    // HTTP error — don't retry
                    let body = resp.into_string().unwrap_or_default();
                    break Err(format!("HTTP {code}: {body}"));
                }
                Err(other) => {
                    last_err = format!("{other}");
                    if attempts >= 2 {
                        break Err(format!("Request failed: {last_err}"));
                    }
                    eprintln!("[OPENAI_COMPAT] Connection error (attempt {}/2): {}, retrying in 1s...", attempts, last_err);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
    };

    let reader = match resp {
        Ok(r) => r.into_reader(),
        Err(error_msg) => return Err(error_msg),
    };

    let buf_reader = std::io::BufReader::new(reader);
    use std::io::BufRead;

    let mut actual_model: Option<String> = model_hint.clone();
    let mut input_tokens: Option<u64> = None;
    let mut output_tokens: Option<u64> = None;
    let mut cached_tokens: Option<u64> = None;
    let mut reasoning_tokens: Option<u64> = None;
    let mut finish_reason: Option<String> = None;
    let mut content = String::new();
    let mut reasoning_content = String::new();

    // Track streaming tool calls — supports multiple parallel tool calls.
    // Each tool call is identified by its index in the delta stream.
    let mut tool_calls_map: std::collections::BTreeMap<u32, AccumulatedToolCall> =
        std::collections::BTreeMap::new();

    for line_result in buf_reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[OPENAI_COMPAT] Read error: {e}");
                break;
            }
        };

        if !line.starts_with("data: ") {
            continue;
        }

        let data = &line[6..];
        if data == "[DONE]" {
            break;
        }

        let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "[OPENAI_COMPAT] Parse error: {e} | data: {}",
                    &data[..data.len().min(200)]
                );
                continue;
            }
        };

        // Track model ID
        if actual_model.is_none() {
            if let Some(ref m) = chunk.model {
                actual_model = Some(m.clone());
            }
        }

        // Track usage (often in the last chunk)
        if let Some(ref usage) = chunk.usage {
            if let Some(pt) = usage.prompt_tokens {
                input_tokens = Some(pt);
            }
            if let Some(ct) = usage.completion_tokens {
                output_tokens = Some(ct);
            }
            // DeepSeek cache tokens
            if let Some(ch) = usage.prompt_cache_hit_tokens {
                cached_tokens = Some(ch);
            }
            // OpenAI-style cached tokens (nested)
            if cached_tokens.is_none() {
                if let Some(ref details) = usage.prompt_tokens_details {
                    if let Some(ct) = details.get("cached_tokens").and_then(|v| v.as_u64()) {
                        cached_tokens = Some(ct);
                    }
                }
            }
            // Reasoning tokens
            if let Some(ref details) = usage.completion_tokens_details {
                if let Some(rt) = details.reasoning_tokens {
                    reasoning_tokens = Some(rt);
                }
            }
        }

        for choice in &chunk.choices {
            if let Some(ref reason) = choice.finish_reason {
                finish_reason = Some(reason.clone());
            }

            if let Some(ref delta) = choice.delta {
                // Accumulate reasoning content (not streamed to frontend)
                if let Some(ref rc) = delta.reasoning_content {
                    if !rc.is_empty() {
                        reasoning_content.push_str(rc);
                    }
                }

                // Stream text content to frontend
                if let Some(ref text) = delta.content {
                    if !text.is_empty() {
                        content.push_str(text);
                        let _ = tx.send(CliTokenData {
                            token: text.clone(),
                            is_done: false,
                            session_id: None,
                            stop_reason: None,
                            cost_usd: None,
                            duration_ms: None,
                            model_id: actual_model.clone(),
                            input_tokens: None,
                            output_tokens: None,
                            cached_tokens: None,
                        });
                    }
                }

                // Accumulate tool calls by index
                if let Some(ref tcs) = delta.tool_calls {
                    for tc in tcs {
                        let idx = tc.index.unwrap_or(0);
                        let entry = tool_calls_map.entry(idx).or_insert_with(|| {
                            AccumulatedToolCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                            }
                        });
                        if let Some(ref id) = tc.id {
                            entry.id = id.clone();
                        }
                        if let Some(ref func) = tc.function {
                            if let Some(ref name) = func.name {
                                entry.name = name.clone();
                            }
                            if let Some(ref args) = func.arguments {
                                entry.arguments.push_str(args);
                            }
                        }
                    }
                }
            }
        }
    }

    let tool_calls: Vec<AccumulatedToolCall> = tool_calls_map.into_values().collect();

    Ok(StreamResult {
        content,
        reasoning_content: if reasoning_content.is_empty() { None } else { Some(reasoning_content) },
        tool_calls,
        actual_model,
        finish_reason,
        input_tokens,
        output_tokens,
        cached_tokens,
        reasoning_tokens,
    })
}
