mod summarize;
mod truncate;
#[cfg(feature = "vision")]
mod image_summary;

// Re-export public API (preserves callers)
pub use summarize::{
    run_summary_pass_public,
    run_summary_reusing_ctx,
    summarize_tool_output_with_prompt,
};
pub use truncate::{
    tool_use_one_liner,
    maybe_truncate_tool_output,
    maybe_summarize_tool_output,
};

// Crate-internal re-exports
pub(crate) use summarize::{
    SUMMARIZE_THRESHOLD,
    summarize_tool_output,
};
#[cfg(feature = "vision")]
pub(crate) use image_summary::run_image_vision_summary;

/// Wrap tool output in the model's chat template turn structure.
pub(crate) fn wrap_output_for_model(output_block: &str, template_type: Option<&str>) -> String {
    match template_type {
        Some("ChatML") => {
            format!(
                "<|im_end|>\n<|im_start|>user\n{output_block}<|im_end|>\n<|im_start|>assistant\n"
            )
        }
        Some("Llama3") => {
            format!(
                "<|eot_id|><|start_header_id|>tool<|end_header_id|>\n\n{output_block}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n"
            )
        }
        Some("Gemma") => {
            format!(
                "<end_of_turn>\n<start_of_turn>user\n{output_block}<end_of_turn>\n<start_of_turn>model\n"
            )
        }
        Some("Harmony") => {
            format!(
                "<|end|>\n{output_block}\n<|start|>assistant<|channel|>analysis<|message|>"
            )
        }
        Some("GLM") => {
            let output_block = output_block.trim();
            format!("\n<|observation|>\n{output_block}\n<|assistant|>\n")
        }
        _ => {
            output_block.to_string()
        }
    }
}
