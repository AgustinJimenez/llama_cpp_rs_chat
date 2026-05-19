use super::ExecBlockTracker;
use llama_cpp_2::token::LlamaToken;
use std::sync::Arc;
use std::time::Instant;

pub(crate) struct TokenGenState {
    pub response: String,
    pub token_pos: i32,
    pub total_tokens_generated: i32,
    pub generated_token_ids: Vec<LlamaToken>,
    pub logger_synced_len: usize,
    pub last_logger_sync: Instant,
    pub exec_tracker: ExecBlockTracker,
    pub recent_commands: Vec<String>,
    pub consecutive_loop_blocks: usize,
    pub last_exec_scan_pos: usize,
    pub finish_reason: String,
    pub tool_response_tokens: i32,
    pub loop_recoveries: u32,
}

#[allow(dead_code)]
pub(crate) struct TokenGenConfig<'a> {
    pub conversation_id: &'a str,
    pub tags: &'a super::super::tool_tags::ToolTags,
    pub template_type: Option<&'a str>,
    pub stop_tokens: &'a [String],
    pub context_size: u32,
    pub max_total_tokens: i32,
    pub use_htmd: bool,
    pub browser_backend: &'a crate::browser::BrowserBackend,
    pub n_batch: u32,
    pub mcp_manager: Option<Arc<dyn llama_chat_tools::McpManagerOps>>,
    pub db: llama_chat_db::SharedDatabase,
    pub backend: &'a llama_cpp_2::llama_backend::LlamaBackend,
    pub chat_template_string: Option<&'a str>,
    pub proactive_compaction: bool,
    pub safe_tool_injection: bool,
}

#[cfg(feature = "vision")]
pub(crate) type VisionCtxRef<'a> = Option<&'a llama_cpp_2::mtmd::MtmdContext>;
#[cfg(not(feature = "vision"))]
pub(crate) type VisionCtxRef<'a> = ();

pub(crate) const TOKEN_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
pub(crate) const REPETITION_CHECK_MIN_TOKENS: i32 = 500;
pub(crate) const REPETITION_CHECK_INTERVAL: i32 = 256;

pub(crate) fn detect_repetition_loop(text: &str) -> bool {
    const TAIL_LEN: usize = 2000;
    const THRESHOLD: f64 = 0.10;

    if text.len() < TAIL_LEN {
        return false;
    }

    let bytes = text.as_bytes();
    let start = bytes.len() - TAIL_LEN;
    let tail = &bytes[start..];
    let total_trigrams = tail.len().saturating_sub(2);
    if total_trigrams == 0 {
        return false;
    }

    let mut seen = std::collections::HashSet::with_capacity(128);
    for i in 0..total_trigrams {
        seen.insert([tail[i], tail[i + 1], tail[i + 2]]);
    }

    let ratio = seen.len() as f64 / total_trigrams as f64;
    ratio < THRESHOLD
}
