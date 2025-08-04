use crate::llm_backend::*;
use anyhow::Result;
use std::num::NonZeroU32;

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend as LlamaCppBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
};

// Debug configuration
const DEBUG_MODE: bool = false;

// Debug macro - only prints if DEBUG_MODE is true
macro_rules! debug_print {
    ($($arg:tt)*) => {
        if DEBUG_MODE {
            println!($($arg)*);
        }
    };
}

pub struct LlamaCppBackendImpl {
    backend: LlamaCppBackend,
    model: LlamaModel,
    sampler: LlamaSampler,
    config: ModelConfig,
}

impl LlamaCppBackendImpl {
    fn detect_prompt_format(model_path: &str) -> PromptFormat {
        let name = std::path::Path::new(model_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if name.contains("qwen") {
            PromptFormat::Qwen
        } else {
            PromptFormat::Mistral
        }
    }

    fn build_prompt(&self, conversation: &[ChatMessage]) -> String {
        let mut full_prompt = String::new();
        let mut system_added = false;

        for msg in conversation {
            match self.config.prompt_format {
                PromptFormat::Mistral => match msg.role.as_str() {
                    "system" if !system_added => {
                        full_prompt.push_str(&msg.content);
                        system_added = true;
                    }
                    "user" => full_prompt.push_str(&format!("\n[INST] {} [/INST]", msg.content)),
                    "assistant" => full_prompt.push_str(&format!(" {} </s>", msg.content)),
                    _ => (),
                },
                PromptFormat::Qwen => match msg.role.as_str() {
                    "system" if !system_added => {
                        full_prompt.push_str(&msg.content);
                        system_added = true;
                    }
                    "user" => full_prompt.push_str(&format!("\n<|im_start|>user\n{}<|im_end|>", msg.content)),
                    "assistant" => full_prompt.push_str(&format!("\n<|im_start|>assistant\n{}<|im_end|>", msg.content)),
                    _ => (),
                },
            }
        }

        // Add assistant role opening for the response
        match self.config.prompt_format {
            PromptFormat::Qwen => full_prompt.push_str("\n<|im_start|>assistant\n"),
            PromptFormat::Mistral => (), // Mistral format is ready after [/INST]
        }

        full_prompt
    }
}

impl LLMBackend for LlamaCppBackendImpl {
    fn initialize(config: ModelConfig) -> Result<Self> {
        let backend = LlamaCppBackend::init()?;
        let model_path = std::path::Path::new(&config.model_path);
        let model = LlamaModel::load_from_file(&backend, model_path, &Default::default())?;
        let sampler = LlamaSampler::greedy();

        Ok(Self {
            backend,
            model,
            sampler,
            config,
        })
    }

    fn generate_response(
        &mut self,
        conversation: &[ChatMessage],
        gen_config: GenerationConfig,
        mut on_token: Box<dyn FnMut(TokenInfo) -> bool>,
    ) -> Result<String> {
        // Create context for this generation
        let n_ctx_nonzero = NonZeroU32::new(self.config.context_size);
        let ctx_params = LlamaContextParams::default().with_n_ctx(n_ctx_nonzero);
        let mut context = self.model.new_context(&self.backend, ctx_params)?;
        
        // Build the full prompt
        let full_prompt = self.build_prompt(conversation);
        
        debug_print!("[DEBUG] Clearing KV cache");
        context.clear_kv_cache();

        debug_print!("[DEBUG] Tokenizing prompt: {} chars", full_prompt.len());
        let tokens = self.model.str_to_token(&full_prompt, AddBos::Never)?;
        debug_print!("[DEBUG] Got {} tokens | Context: {}/{} ({} remaining)", 
                    tokens.len(), tokens.len(), self.config.context_size, 
                    self.config.context_size as usize - tokens.len());

        // Prepare batch
        let mut batch = LlamaBatch::new(1024, 1);
        for (i, &token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch.add(token, i as i32, &[0], is_last)?;
        }

        debug_print!("[DEBUG] About to decode batch");
        match context.decode(&mut batch) {
            Ok(()) => {
                debug_print!("[DEBUG] Batch decoded successfully");
            },
            Err(e) => {
                debug_print!("[DEBUG] Batch decode failed: {}", e);
                return Err(anyhow::anyhow!("Failed to decode initial batch: {}. This might indicate GPU memory issues. Try reducing context size or restarting the application.", e));
            }
        }

        // Generate response
        let mut response = String::new();
        let mut token_count = 0;
        let mut token_pos = tokens.len() as i32;
        let mut recent_tokens: Vec<i32> = Vec::new();

        // Calculate dynamic token limit
        let available_tokens = (self.config.context_size as usize).saturating_sub(tokens.len());
        let max_tokens = if available_tokens > 100 {
            available_tokens - 100 // Leave 100 token safety buffer
        } else {
            50 // Minimum tokens for a response, even if context is nearly full
        }.min(gen_config.max_tokens);

        debug_print!("[DEBUG] Dynamic token limit: {} (context: {}, used: {}, available: {})", 
                    max_tokens, self.config.context_size, tokens.len(), available_tokens);

        debug_print!("[DEBUG] Starting token generation loop");
        
        loop {
            token_count += 1;
            if token_count > max_tokens {
                debug_print!("[DEBUG] Hit max token limit ({}), stopping generation", max_tokens);
                break;
            }

            // Sample next token
            debug_print!("[DEBUG] About to sample token");
            let token = self.sampler.sample(&context, -1);
            debug_print!("[DEBUG] Sampled token: {}", token);

            // Memory check every 100 tokens
            if token_count % 100 == 0 {
                debug_print!("[DEBUG] Memory check at token {} - checking for Metal issues", token_count);
            }

            // Track recent tokens for repetition detection
            recent_tokens.push(token.0);
            if recent_tokens.len() > 10 {
                recent_tokens.remove(0);
            }

            // Check for repetitive patterns
            if recent_tokens.len() >= 5 {
                let last_token = recent_tokens[recent_tokens.len() - 1];
                let is_repeating = recent_tokens.iter().rev().take(5).all(|&t| t == last_token);
                if is_repeating {
                    debug_print!("[DEBUG] Detected repetitive pattern, stopping generation");
                    break;
                }
            }

            // Check for end-of-sequence token
            if token == self.model.token_eos() {
                debug_print!("\n[DEBUG] Hit EOS token, stopping generation");
                break;
            }

            // Convert token to string
            if let Ok(token_str) = self.model.token_to_str(token, Special::Tokenize) {
                debug_print!("\n[DEBUG] Generated token: '{}'", token_str.replace('\n', "\\n"));
                
                response.push_str(&token_str);
                debug_print!("[DEBUG] Full response so far: '{}'", response.replace('\n', "\\n"));

                // Check for stop strings
                let mut should_stop = false;
                for stop_str in &gen_config.stop_strings {
                    if let Some(pos) = response.find(stop_str) {
                        debug_print!("[DEBUG] Found stop string '{}' at position {}", stop_str, pos);
                        response.truncate(pos);
                        should_stop = true;
                        break;
                    }
                }

                if should_stop {
                    break;
                }

                // Call the token callback
                let token_info = TokenInfo {
                    token_id: token.0 as u32,
                    token_str: token_str.clone(),
                };
                
                if !on_token(token_info) {
                    debug_print!("[DEBUG] Token callback requested stop");
                    break;
                }
            }

            // Prepare for next token generation
            batch.clear();
            if let Err(e) = batch.add(token, token_pos, &[0], true) {
                debug_print!("[DEBUG] Error adding token to batch: {}", e);
                return Err(anyhow::anyhow!("Failed to add token to batch - {}. This might indicate insufficient KV cache space.", e));
            }
            if let Err(e) = context.decode(&mut batch) {
                debug_print!("[DEBUG] Error decoding batch: {}", e);
                return Err(anyhow::anyhow!("Failed to decode batch - {}. This might indicate insufficient KV cache space.", e));
            }
            token_pos += 1;
        }

        Ok(response.trim().to_string())
    }

    fn get_context_info(&self, conversation: &[ChatMessage], response: &str) -> Result<ContextInfo> {
        let full_prompt = self.build_prompt(conversation);
        let final_tokens = self.model.str_to_token(&full_prompt, AddBos::Never)?;
        let response_tokens = self.model.str_to_token(response, AddBos::Never)?;
        let total_tokens_used = final_tokens.len() + response_tokens.len();
        let context_remaining = self.config.context_size as usize - total_tokens_used;
        let context_usage_percent = (total_tokens_used as f32 / self.config.context_size as f32 * 100.0).round() as u32;

        debug_print!("[DEBUG] Final context: Prompt={} + Response={} = Total={}/{} ({}% used, {} remaining)", 
                    final_tokens.len(), response_tokens.len(), total_tokens_used, self.config.context_size, 
                    context_usage_percent, context_remaining);

        Ok(ContextInfo {
            prompt_tokens: final_tokens.len(),
            response_tokens: response_tokens.len(),
            total_tokens: total_tokens_used,
            context_size: self.config.context_size as usize,
            usage_percent: context_usage_percent,
        })
    }

    fn backend_name(&self) -> &'static str {
        "llama-cpp-2"
    }

    fn clear_cache(&mut self) -> Result<()> {
        // Since we create context per generation, no persistent cache to clear
        Ok(())
    }
}