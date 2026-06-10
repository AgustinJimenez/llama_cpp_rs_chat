use std::num::NonZeroU32;

use llama_chat_types::*;
use crate::generation::create_fresh_context;
use super::summarize::{
    SUMMARY_CTX_SIZE, SUMMARY_MAX_TOKENS,
    run_summary_pass_with_system, run_summary_reusing_ctx_with_system,
};

/// Generate a compact one-line summary of a tool execution result.
pub fn tool_use_one_liner(tool_name: &str, args_hint: &str, output: &str, duration_ms: u64) -> String {
    let status = if output.contains("Error") || output.contains("error:") || output.contains("FAILED") {
        "FAILED"
    } else {
        "OK"
    };

    let detail = match tool_name {
        "execute_command" => {
            if let Some(line) = output.lines().rev().find(|l| l.contains("exit code")) {
                line.trim().to_string()
            } else {
                format!("{} chars output", output.len())
            }
        }
        "write_file" => {
            if let Some(bytes) = output.split("wrote ").nth(1).and_then(|s| s.split(' ').next()) {
                format!("wrote {}", bytes)
            } else {
                output.lines().next().unwrap_or("done").to_string()
            }
        }
        "read_file" => {
            let lines = output.lines().count();
            format!("{} lines", lines)
        }
        "browser_search" => {
            let results = output.matches("URL:").count().max(output.matches("http").count().min(10));
            format!("{} results", results)
        }
        _ => {
            let first_line = output.lines().next().unwrap_or("done");
            if first_line.chars().count() > 80 {
                let truncated: String = first_line.chars().take(77).collect();
                format!("{truncated}...")
            } else {
                first_line.to_string()
            }
        }
    };

    let args_part = if args_hint.is_empty() {
        String::new()
    } else {
        let hint = if args_hint.chars().count() > 60 {
            let truncated: String = args_hint.chars().take(57).collect();
            format!("{truncated}...")
        } else {
            args_hint.to_string()
        };
        format!(" {}", hint)
    };

    if duration_ms > 0 {
        format!("[{}{} -> {} {} ({}ms)]", tool_name, args_part, status, detail, duration_ms)
    } else {
        format!("[{}{} -> {} {}]", tool_name, args_part, status, detail)
    }
}

/// Threshold above which tool output gets smart-truncated before context injection.
const TOOL_OUTPUT_TOKEN_THRESHOLD: usize = 2000;
const TOOL_OUTPUT_TRUNCATION_THRESHOLD: usize = TOOL_OUTPUT_TOKEN_THRESHOLD * 4; // ~8000 chars

/// Smart-truncate large tool output, preserving start and end.
pub fn maybe_truncate_tool_output(output: &str, tool_name: &str, conversation_id: &str) -> String {
    if output.len() <= TOOL_OUTPUT_TRUNCATION_THRESHOLD {
        return output.to_string();
    }

    match tool_name {
        "write_file" | "edit_file" | "read_file" => return output.to_string(),
        _ => {}
    }

    llama_chat_db::event_log::log_event(
        conversation_id, "tool_truncate",
        &format!("{}: truncated {} -> {} chars", tool_name, output.len(), TOOL_OUTPUT_TRUNCATION_THRESHOLD),
    );
    log_info!(
        conversation_id,
        "✂️ Truncating {} output: {} -> {} chars",
        tool_name, output.len(), TOOL_OUTPUT_TRUNCATION_THRESHOLD
    );

    let one_liner = tool_use_one_liner(tool_name, "", output, 0);
    let mut head = (TOOL_OUTPUT_TRUNCATION_THRESHOLD * 3 / 4).min(output.len());
    let mut tail_start = output.len().saturating_sub(TOOL_OUTPUT_TRUNCATION_THRESHOLD / 4);
    while head > 0 && !output.is_char_boundary(head) { head -= 1; }
    while tail_start < output.len() && !output.is_char_boundary(tail_start) { tail_start += 1; }
    let truncated = output.len().saturating_sub(head).saturating_sub(output.len() - tail_start);
    format!(
        "{}\n{}\n\n[...{} chars truncated — {} total. Key info may be at the end.]\n\n{}",
        one_liner,
        &output[..head],
        truncated,
        output.len(),
        &output[tail_start..]
    )
}

/// Summarize large tool output using recursive map-reduce (sub-agent approach).
/// Falls back to truncation if summarization fails.
pub fn maybe_summarize_tool_output(
    output: &str,
    tool_name: &str,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> String {
    const PASS_THROUGH_THRESHOLD: usize = 8000;
    const SINGLE_PASS_LIMIT: usize = 10000;
    const MAP_REDUCE_CTX: u32 = 4096;

    if output.len() <= PASS_THROUGH_THRESHOLD {
        return output.to_string();
    }

    let lower_name = tool_name.to_lowercase();
    if lower_name.contains("read_file") || lower_name.contains("write_file") || lower_name.contains("edit_file")
        || lower_name.contains("browser_get_html") || lower_name.contains("browser_eval")
    {
        return maybe_truncate_tool_output(output, tool_name, conversation_id);
    }

    let extra_instructions = if lower_name.contains("browser_get_links") || lower_name.contains("browser_get_html") {
        "\nCRITICAL: Preserve ALL URLs/href values and their associated text. \
         The user needs the actual links to navigate. Never omit or paraphrase URLs."
    } else if lower_name.contains("browser_get_text") {
        "\nPreserve key facts, names, dates, and quotes from the page content. \
         Keep article structure (headings, main points)."
    } else if lower_name.contains("browser_eval") {
        "\nPreserve the complete data structure (JSON arrays, objects). \
         Do not paraphrase structured data — keep it verbatim if possible."
    } else {
        ""
    };

    let system_prompt = format!(
        "Summarize this {} tool output concisely. Extract ONLY:\n\
         - Key results and status\n\
         - Error messages with file paths and line numbers\n\
         - Important warnings\n\
         - Actionable information\n\n\
         Remove verbose logs, progress bars, repeated output, boilerplate.\n\
         Keep under 500 words.{extra_instructions}",
        tool_name
    );

    log_info!(conversation_id, "📝 [TOOL_SUMMARY] Summarizing {} output: {} chars", tool_name, output.len());
    llama_chat_db::event_log::log_event(conversation_id, "tool_summary",
        &format!("{}: {} chars -> summarizing", tool_name, output.len()));

    if output.len() <= SINGLE_PASS_LIMIT {
        match run_summary_pass_with_system(
            model, backend, output, chat_template_string, conversation_id, &system_prompt,
            SUMMARY_CTX_SIZE, SUMMARY_MAX_TOKENS,
        ) {
            Ok(summary) => {
                log_info!(conversation_id, "📝 [TOOL_SUMMARY] Single pass: {} -> {} chars", output.len(), summary.len());
                llama_chat_db::event_log::log_event(conversation_id, "tool_summary",
                    &format!("{}: {} -> {} chars (single)", tool_name, output.len(), summary.len()));
                return format!("[Summarized {} output: {} -> {} chars]\n{}", tool_name, output.len(), summary.len(), summary);
            }
            Err(e) => {
                log_warn!(conversation_id, "[TOOL_SUMMARY] Single pass failed: {}, falling back to truncation", e);
                return maybe_truncate_tool_output(output, tool_name, conversation_id);
            }
        }
    }

    eprintln!("[TOOL_SUMMARY] Map-reduce summarizing {} output: {} chars", tool_name, output.len());

    let n_ctx = NonZeroU32::new(MAP_REDUCE_CTX).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = match create_fresh_context(model, backend, n_ctx, true, &config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[TOOL_SUMMARY] Failed to create summary context: {}", e);
            return maybe_truncate_tool_output(output, tool_name, conversation_id);
        }
    };

    match map_reduce_summarize_tool_output(model, &mut ctx, output, chat_template_string, conversation_id, &system_prompt) {
        Ok(summary) => {
            log_info!(conversation_id, "📝 [TOOL_SUMMARY] Map-reduce: {} -> {} chars", output.len(), summary.len());
            llama_chat_db::event_log::log_event(conversation_id, "tool_summary",
                &format!("{}: {} -> {} chars (map-reduce)", tool_name, output.len(), summary.len()));
            format!("[Summarized {} output: {} -> {} chars]\n{}", tool_name, output.len(), summary.len(), summary)
        }
        Err(e) => {
            log_warn!(conversation_id, "[TOOL_SUMMARY] Map-reduce failed: {}, falling back to truncation", e);
            maybe_truncate_tool_output(output, tool_name, conversation_id)
        }
    }
}

/// Recursive map-reduce summarization for tool output.
fn map_reduce_summarize_tool_output(
    model: &llama_cpp_2::model::LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    system_prompt: &str,
) -> Result<String, String> {
    const CHUNK_SIZE: usize = 10000;

    let mut summaries = Vec::new();
    let mut pos = 0;
    let total_chunks = text.len().div_ceil(CHUNK_SIZE);
    let mut chunk_num = 0;

    while pos < text.len() {
        let end = (pos + CHUNK_SIZE).min(text.len());
        let end = (pos..=end).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(end);
        let chunk = &text[pos..end];
        chunk_num += 1;

        eprintln!("[TOOL_SUMMARY] Map chunk {}/{} ({} chars)", chunk_num, total_chunks, chunk.len());

        match run_summary_reusing_ctx_with_system(model, ctx, chunk, chat_template_string, conversation_id, system_prompt, SUMMARY_CTX_SIZE as usize, SUMMARY_MAX_TOKENS) {
            Ok(summary) => {
                eprintln!("[TOOL_SUMMARY] Chunk {} -> {} chars", chunk_num, summary.len());
                summaries.push(summary);
            }
            Err(e) => {
                eprintln!("[TOOL_SUMMARY] Chunk {} failed: {}, using truncated fallback", chunk_num, e);
                summaries.push(chunk.chars().take(200).collect::<String>() + "...");
            }
        }

        pos = end;
    }

    let combined = summaries.join("\n\n");
    eprintln!("[TOOL_SUMMARY] Reduce: {} summaries ({} chars)", summaries.len(), combined.len());

    if combined.len() <= CHUNK_SIZE {
        run_summary_reusing_ctx_with_system(model, ctx, &combined, chat_template_string, conversation_id, system_prompt, SUMMARY_CTX_SIZE as usize, SUMMARY_MAX_TOKENS)
    } else {
        map_reduce_summarize_tool_output(model, ctx, &combined, chat_template_string, conversation_id, system_prompt)
    }
}
