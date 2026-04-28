//! Extended config functions that depend on engine types.
//!
//! These functions live in the engine crate rather than llama-chat-config
//! because they depend on `SharedLlamaState` and system prompt resolution
//! which are engine-level concerns.

use llama_chat_types::SharedLlamaState;
use llama_chat_db::Database;
use llama_chat_config::load_config;

use crate::tool_tags::get_tool_tags_for_model;
use crate::templates::get_universal_system_prompt_with_tags;

/// Get the resolved system prompt based on config and model state.
///
/// Uses a cache on LlamaState to avoid re-resolving on every request.
/// Cache key: (config.system_prompt, general_name). Invalidated on config
/// or model change.
///
/// Priority: 1. "__AGENTIC__" → universal agentic prompt
///           2. Custom string → use as-is
///           3. None → fallback to agentic prompt
pub fn get_resolved_system_prompt(
    db: &Database,
    llama_state: &Option<SharedLlamaState>,
) -> Option<String> {
    let config = load_config(db);
    let current_key = (config.system_prompt.clone(), {
        llama_state.as_ref().and_then(|s| {
            s.lock()
                .ok()
                .and_then(|g| g.as_ref().and_then(|st| st.general_name.clone()))
        })
    });

    // Check cache
    if let Some(ref state_arc) = llama_state {
        if let Ok(mut guard) = state_arc.lock() {
            if let Some(ref mut state) = *guard {
                if state.cached_prompt_key.as_ref() == Some(&current_key) {
                    return state.cached_system_prompt.clone();
                }
            }
        }
    }

    // Cache miss: resolve
    let resolved = match config.system_prompt.as_deref() {
        Some("__AGENTIC__") | None => {
            // Both explicit agentic marker and no prompt default to agentic mode
            let general_name = current_key.1.as_deref();
            let tags = get_tool_tags_for_model(general_name);
            Some(get_universal_system_prompt_with_tags(&tags))
        }
        Some(custom) => Some(custom.to_string()),
    };

    // Store in cache
    if let Some(ref state_arc) = llama_state {
        if let Ok(mut guard) = state_arc.lock() {
            if let Some(ref mut state) = *guard {
                state.cached_system_prompt = resolved.clone();
                state.cached_prompt_key = Some(current_key);
            }
        }
    }

    resolved
}
