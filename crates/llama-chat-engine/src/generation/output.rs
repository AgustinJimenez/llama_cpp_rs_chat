use super::*;

/// Output from a generation run, including timing metrics.
pub struct GenerationOutput {
    #[allow(dead_code)]
    pub response: String,
    pub tokens_used: i32,
    pub max_tokens: i32,
    pub finish_reason: String,
    pub prompt_tok_per_sec: Option<f64>,
    pub gen_tok_per_sec: Option<f64>,
    pub gen_eval_ms: Option<f64>,
    pub gen_tokens: Option<i32>,
    pub prompt_eval_ms: Option<f64>,
    pub prompt_tokens: Option<i32>,
    pub token_breakdown: Option<llama_chat_types::TokenBreakdown>,
}

pub(super) fn strip_incomplete_tool_call_on_cancel(gen: &mut TokenGenState) {
    let last_tool_open = gen.response.rfind("<tool_call>");
    let last_tool_close = gen.response.rfind("</tool_call>");
    let last_fn_close = gen.response.rfind("</function>");
    if let Some(open_pos) = last_tool_open {
        let is_unclosed = match last_tool_close {
            Some(close_pos) => close_pos < open_pos,
            None => true,
        };
        let is_fn_unclosed = match last_fn_close {
            Some(close_pos) => close_pos < open_pos,
            None => true,
        };
        if is_unclosed || is_fn_unclosed {
            eprintln!("[CANCEL] Stripping incomplete tool call at pos {}", open_pos);
            gen.response.truncate(open_pos);
            gen.logger_synced_len = gen.logger_synced_len.min(open_pos);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_generation_output(
    gen: &TokenGenState,
    token_pos: i32,
    context_size: u32,
    prompt_tok_per_sec: Option<f64>,
    gen_tok_per_sec: Option<f64>,
    gen_eval_ms: f64,
    n_eval: usize,
    prompt_eval_ms_internal: f64,
    n_p_eval: usize,
    prompt_tokens: usize,
    system_prompt_token_count: i32,
    tool_def_token_count: i32,
) -> GenerationOutput {
    GenerationOutput {
        response: gen.response.trim().to_string(),
        tokens_used: token_pos,
        max_tokens: context_size as i32,
        finish_reason: gen.finish_reason.clone(),
        prompt_tok_per_sec,
        gen_tok_per_sec,
        gen_eval_ms: if gen_eval_ms > 0.0 {
            Some(gen_eval_ms)
        } else {
            None
        },
        gen_tokens: if n_eval > 0 { Some(n_eval as i32) } else { None },
        prompt_eval_ms: if prompt_eval_ms_internal > 0.0 {
            Some(prompt_eval_ms_internal)
        } else {
            None
        },
        prompt_tokens: if n_p_eval > 0 {
            Some(n_p_eval as i32)
        } else {
            None
        },
        token_breakdown: Some(TokenBreakdown {
            system_prompt: system_prompt_token_count,
            tool_definitions: tool_def_token_count,
            conversation_messages: (prompt_tokens as i32
                - system_prompt_token_count
                - tool_def_token_count)
                .max(0),
            tool_calls_and_results: gen.tool_response_tokens,
            model_response: n_eval as i32,
        }),
    }
}
