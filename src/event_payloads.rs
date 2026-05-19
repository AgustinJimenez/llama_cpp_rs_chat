// ─── Event Payloads ───────────────────────────────────────────────────

use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct ChatTokenEvent {
    pub token: String,
    pub tokens_used: i32,
    pub max_tokens: i32,
}

#[derive(Serialize, Clone)]
pub struct ChatDoneEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub conversation_id: Option<String>,
    pub tokens_used: Option<i32>,
    pub max_tokens: Option<i32>,
    pub error: Option<String>,
    pub prompt_tok_per_sec: Option<f64>,
    pub gen_tok_per_sec: Option<f64>,
    pub gen_eval_ms: Option<f64>,
    pub gen_tokens: Option<i32>,
    pub prompt_eval_ms: Option<f64>,
    pub prompt_tokens: Option<i32>,
    pub finish_reason: Option<String>,
}

// ─── Provider streaming event payloads ────────────────────────────────

#[derive(Serialize, Clone)]
pub struct ProviderTokenEvent {
    pub token: String,
}

#[derive(Serialize, Clone)]
pub struct ProviderDoneEvent {
    pub conversation_id: String,
    pub session_id: Option<String>,
    pub stop_reason: Option<String>,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub model: Option<String>,
}
