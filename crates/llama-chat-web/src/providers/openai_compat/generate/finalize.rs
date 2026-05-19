//! Post-generation finalization: cost computation, timing persistence, done event, title generation.

use crate::providers::openai_compat_request::provider_cost_per_million;
use crate::providers::{clear_remote_generating, CliTokenData};
use super::super::db::{maybe_generate_title_after_response, provider_log, save_message_now};

use serde_json::Value;
use tokio::sync::mpsc;

/// All accumulated counters from the agentic loop.
pub struct LoopCounters {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    pub total_reasoning_tokens: u64,
    pub actual_model: Option<String>,
    pub final_stop_reason: String,
}

/// Emit the `is_done` event, persist timings, clear remote tracker, and kick off title generation.
pub fn finalize_generation(
    tx: &mpsc::UnboundedSender<CliTokenData>,
    counters: &LoopCounters,
    start: std::time::Instant,
    provider_id: &str,
    model_name: &str,
    url: &str,
    api_key: &str,
    prompt: &str,
    conv_id: &Option<String>,
    db: &Option<llama_chat_db::SharedDatabase>,
    messages: &[Value],
) {
    let duration_ms = start.elapsed().as_millis() as u64;

    provider_log(conv_id, "provider_complete",
        &format!("model={:?} stop={} duration={}ms tokens={}in/{}out",
            counters.actual_model, counters.final_stop_reason, duration_ms,
            counters.total_input_tokens, counters.total_output_tokens));

    // Compute cost estimate (cache-aware: cached tokens are discounted)
    let cost_usd = provider_cost_per_million(provider_id, model_name)
        .map(|(ic, oc, cache_discount)| {
            let uncached = counters.total_input_tokens.saturating_sub(counters.total_cached_tokens) as f64;
            let cached = counters.total_cached_tokens as f64;
            let input_cost = (uncached * ic + cached * ic * cache_discount) / 1_000_000.0;
            let output_cost = counters.total_output_tokens as f64 * oc / 1_000_000.0;
            input_cost + output_cost
        });

    if counters.total_cached_tokens > 0 {
        provider_log(conv_id, "cache_stats",
            &format!("cached={} uncached={} reasoning={}", counters.total_cached_tokens,
                counters.total_input_tokens.saturating_sub(counters.total_cached_tokens),
                counters.total_reasoning_tokens));
    }

    // Save timings on the last assistant message so stats persist after refresh
    if let (Some(cid), Some(ref db_ref)) = (conv_id, db) {
        let conn = db_ref.connection();
        let last_asst_id: Option<String> = conn.query_row(
            "SELECT id FROM messages WHERE conversation_id = ?1 AND role = 'assistant' ORDER BY sequence_order DESC LIMIT 1",
            [cid],
            |row| row.get(0),
        ).ok();
        if let Some(msg_id) = last_asst_id {
            let gen_tok_per_sec = if duration_ms > 0 {
                Some(counters.total_output_tokens as f64 / (duration_ms as f64 / 1000.0))
            } else {
                None
            };
            let _ = db_ref.update_message_timings(
                &msg_id,
                None,
                gen_tok_per_sec,
                Some(duration_ms as f64),
                Some(counters.total_output_tokens as i32),
                None,
                Some(counters.total_input_tokens as i32),
            );
        }
    }

    // Send done event
    let _ = tx.send(CliTokenData {
        token: String::new(),
        is_done: true,
        session_id: None,
        stop_reason: Some(counters.final_stop_reason.clone()),
        cost_usd,
        duration_ms: Some(duration_ms),
        model_id: counters.actual_model.clone(),
        input_tokens: if counters.total_input_tokens > 0 { Some(counters.total_input_tokens) } else { None },
        output_tokens: if counters.total_output_tokens > 0 { Some(counters.total_output_tokens) } else { None },
        cached_tokens: if counters.total_cached_tokens > 0 { Some(counters.total_cached_tokens) } else { None },
    });

    // Clear remote generation tracker
    clear_remote_generating();

    // Generate title after response is done (non-blocking, cheap API call)
    if let (Some(cid), Some(ref db_ref)) = (conv_id, db) {
        maybe_generate_title_after_response(
            cid,
            db_ref,
            messages,
            prompt,
            url,
            api_key,
            model_name,
            conv_id,
        );
    }
}

/// Save the initial user message and system prompt to the database.
pub fn save_initial_messages(
    db: &llama_chat_db::SharedDatabase,
    conv_id: &str,
    provider_id: &str,
    prompt: &str,
    system_prompt: &str,
) {
    use super::super::db::ensure_conversation_row;
    ensure_conversation_row(db, conv_id, provider_id);
    // Save system prompt on first turn so frontend can display it
    if db.get_messages(conv_id).map(|m| m.is_empty()).unwrap_or(true) {
        save_message_now(db, conv_id, "system", system_prompt);
    }
    save_message_now(db, conv_id, "user", prompt);
}
