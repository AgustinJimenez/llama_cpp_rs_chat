mod system_prompts;
mod chat_templates;

#[cfg(test)]
mod tests;

// Re-export public API (preserves callers)
pub use system_prompts::{
    get_behavioral_system_prompt,
    get_universal_system_prompt_with_tags,
};
pub use chat_templates::apply_model_chat_template_with_tags;

#[cfg(test)]
pub use chat_templates::apply_model_chat_template;

use crate::jinja_templates::{
    apply_native_chat_template, get_available_tools_openai_with_mcp, parse_conversation_for_jinja,
};
use crate::tool_tags::ToolTags;
use llama_chat_tools::McpToolDefInfo as McpToolDef;

/// Try to render a prompt using the model's native Jinja2 chat template.
fn try_jinja_render(
    template_str: &str,
    conversation: &str,
    bos_token: &str,
    eos_token: &str,
    mcp_tools: Option<&[McpToolDef]>,
    enable_thinking: bool,
    custom_system_prompt: Option<&str>,
) -> Result<String, String> {
    let system_prompt = match custom_system_prompt {
        Some(custom) => custom.to_string(),
        None => get_behavioral_system_prompt(),
    };
    let messages = parse_conversation_for_jinja(conversation, &system_prompt);
    let tools = get_available_tools_openai_with_mcp(mcp_tools);

    apply_native_chat_template(
        template_str,
        messages,
        Some(tools),
        None,
        true,
        bos_token,
        eos_token,
        enable_thinking,
    )
}

/// Apply system prompt with model-specific tool tags.
///
/// Primary path: render using the model's native Jinja2 chat template.
/// Fallback: hardcoded template branches with tool tags in system prompt.
///
/// `custom_system_prompt`: when `Some`, overrides the default agentic system prompt
/// (e.g. from an agent's configured `system_prompt`). `None` uses the universal
/// agentic prompt.
#[allow(clippy::too_many_arguments)]
pub fn apply_system_prompt_by_type_with_tags(
    conversation: &str,
    template_type: Option<&str>,
    chat_template_string: Option<&str>,
    tags: &ToolTags,
    bos_token: &str,
    eos_token: &str,
    mcp_tools: Option<&[McpToolDef]>,
    enable_thinking: bool,
    custom_system_prompt: Option<&str>,
) -> Result<String, String> {
    if let Some(template_str) = chat_template_string {
        sys_info!("Trying Jinja template rendering (primary path, template len={})", template_str.len());
        match try_jinja_render(template_str, conversation, bos_token, eos_token, mcp_tools, enable_thinking, custom_system_prompt) {
            Ok(prompt) => {
                sys_info!("Jinja template rendered successfully ({} chars)", prompt.len());
                return Ok(prompt);
            }
            Err(e) => {
                sys_warn!("Jinja render failed ({}), falling back to hardcoded templates", e);
            }
        }
    } else {
        sys_info!("No Jinja template available, using hardcoded path");
    }
    sys_info!("Using hardcoded template (type={:?})", template_type);
    apply_model_chat_template_with_tags(conversation, template_type, tags, mcp_tools, custom_system_prompt)
}
