use std::io::{self, Write};
use std::num::NonZeroU32;

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
};

use super::commands::detect_and_execute_command;
use super::logger::ConversationLogger;

pub(crate) fn generate_response(
    backend: &LlamaBackend,
    model: &LlamaModel,
    sampler: &mut LlamaSampler,
    user_message: &str,
    context_size: u32,
    conversation_logger: &mut ConversationLogger,
    system_prompt: &str,
    show_command_output: bool,
    debug_test: bool,
) -> Result<String, String> {
    let prompt = format!(
        "<|start_of_role|>system<|end_of_role|>{system_prompt}<|end_of_text|><|start_of_role|>user<|end_of_role|>{user_message}<|end_of_text|><|start_of_role|>assistant<|end_of_role|>"
    );

    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| format!("Tokenization failed: {e}"))?;

    let n_ctx = NonZeroU32::new(context_size).unwrap();
    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
    let mut context = model
        .new_context(backend, ctx_params)
        .map_err(|e| format!("Context creation failed: {e}"))?;

    let batch_size = std::cmp::min(tokens.len() + 1000, 4096);
    let mut batch = LlamaBatch::new(batch_size, 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(token, i as i32, &[0], is_last)
            .map_err(|e| format!("Batch add failed: {e}"))?;
    }

    context
        .decode(&mut batch)
        .map_err(|e| format!("Initial decode failed: {e}"))?;

    let mut response = String::new();
    let mut token_pos = tokens.len() as i32;

    loop {
        let next_token = sampler.sample(&context, -1);
        if next_token == model.token_eos() {
            break;
        }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, Special::Tokenize)
            .map_err(|e| format!("Token conversion failed: {e}"))?;

        if token_str.contains("<|user|>")
            || token_str.contains("<|end|>")
            || token_str.contains("<|endoftext|>")
            || token_str.contains("<|im_end|>")
            || response.ends_with("<|user|>")
            || response.ends_with("<|end|>")
        {
            break;
        }

        response.push_str(&token_str);

        if response.contains("<COMMAND>") && response.contains("</COMMAND>") {
            let (processed_response, command_executed) = detect_and_execute_command(
                &response,
                conversation_logger,
                show_command_output,
                debug_test,
            );

            if command_executed {
                print!("{token_str}");
                io::stdout().flush().unwrap();
                return Ok(processed_response);
            }
        }

        print!("{token_str}");
        io::stdout().flush().unwrap();

        if response.len() > 10000 {
            break;
        }

        batch.clear();
        batch
            .add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Batch add failed: {e}"))?;

        context
            .decode(&mut batch)
            .map_err(|e| format!("Decode failed: {e}"))?;

        token_pos += 1;
    }

    Ok(response.trim().to_string())
}
