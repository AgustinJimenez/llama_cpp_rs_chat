use std::io::{self, Write};
use anyhow::Result;
use std::num::NonZeroU32;

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
};

fn main() -> Result<()> {
    println!("🦙 Minimal LLaMA Chat Test - Direct API");
    
    // Model path
    let model_path = r"E:\.lmstudio\models\lmstudio-community\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL-GGUF\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL.gguf";
    
    println!("🔍 DEBUG: Initializing backend...");
    io::stdout().flush()?;
    let backend = LlamaBackend::init()?;
    
    println!("🔍 DEBUG: Loading model...");
    io::stdout().flush()?;
    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, model_path, &model_params)?;
    
    println!("🔍 DEBUG: Creating sampler...");
    io::stdout().flush()?;
    let mut sampler = LlamaSampler::greedy();
    
    println!("🔍 DEBUG: Model loaded successfully");
    io::stdout().flush()?;
    
    let mut turn_count = 0;
    
    loop {
        turn_count += 1;
        println!("\n🔍 DEBUG: Starting turn #{}", turn_count);
        io::stdout().flush()?;
        
        let user_input = if turn_count == 1 {
            let auto_message = "make me a laravel mvc people crud";
            println!("🔍 DEBUG: Auto-sending: '{}'", auto_message);
            println!("You: {}", auto_message);
            io::stdout().flush()?;
            auto_message.to_string()
        } else {
            print!("You: ");
            io::stdout().flush()?;
            
            println!("🔍 DEBUG: Reading user input...");
            io::stdout().flush()?;
            
            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(0) => {
                    println!("🔍 DEBUG: EOF detected, ending chat");
                    break;
                }
                Ok(bytes) => {
                    println!("🔍 DEBUG: Read {} bytes", bytes);
                    io::stdout().flush()?;
                }
                Err(e) => {
                    println!("❌ DEBUG: Error reading: {}", e);
                    break;
                }
            }
            
            input.trim().to_string()
        };
        
        if user_input.eq_ignore_ascii_case("exit") {
            break;
        }
        
        if user_input.is_empty() {
            continue;
        }
        
        println!("🔍 DEBUG: Processing input: '{}'", user_input);
        io::stdout().flush()?;
        
        // Build simple prompt
        let prompt = format!("You are a helpful AI assistant.\n\nHuman: {}\n\nAssistant:", user_input);
        
        println!("🔍 DEBUG: Creating context...");
        io::stdout().flush()?;
        let n_ctx_nonzero = NonZeroU32::new(32000);
        let ctx_params = LlamaContextParams::default().with_n_ctx(n_ctx_nonzero);
        let mut context = model.new_context(&backend, ctx_params)?;
        
        println!("🔍 DEBUG: Tokenizing prompt ({} chars)...", prompt.len());
        io::stdout().flush()?;
        let tokens = model.str_to_token(&prompt, AddBos::Never)?;
        println!("🔍 DEBUG: Got {} tokens", tokens.len());
        io::stdout().flush()?;
        
        println!("🔍 DEBUG: Clearing context and loading tokens...");
        io::stdout().flush()?;
        context.clear_kv_cache();
        
        // Calculate appropriate batch size  
        let available_tokens = 32000_usize.saturating_sub(tokens.len());
        let batch_size = std::cmp::min(1024, std::cmp::max(256, available_tokens));
        println!("🔍 DEBUG: Creating batch size {} for {} tokens", batch_size, tokens.len());
        io::stdout().flush()?;
        
        let mut batch = LlamaBatch::new(batch_size, 1);
        for (i, &token) in tokens.iter().enumerate() {
            // Only the last token needs logits for generation
            let logits = i == tokens.len() - 1;
            batch.add(token, i as i32, &[0], logits)?;
        }
        
        match context.decode(&mut batch) {
            Ok(()) => println!("🔍 DEBUG: Initial batch decoded successfully"),
            Err(e) => {
                println!("❌ DEBUG: Failed to decode initial batch: {}", e);
                continue;
            }
        }
        
        println!("🔍 DEBUG: Starting generation...");
        print!("Assistant: ");
        io::stdout().flush()?;
        
        let mut response = String::new();
        let mut token_count = 0;
        let generation_start = std::time::SystemTime::now();
        
        // Generate response
        for generation_step in 0..2048 { // Max 2048 tokens
            let next_token = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sampler.sample(&mut context, -1)
            })) {
                Ok(token) => token,
                Err(_) => {
                    println!("\n❌ DEBUG: Sampling panicked at step {}", generation_step);
                    break;
                }
            };
            
            if next_token == model.token_eos() {
                println!("\n🔍 DEBUG: EOS token encountered");
                break;
            }
            
            let token_str = model.token_to_str(next_token, Special::Tokenize)?;
            print!("{}", token_str);
            io::stdout().flush()?;
            
            response.push_str(&token_str);
            token_count += 1;
            
            // Add token to context for next iteration
            batch.clear();
            batch.add(next_token, (tokens.len() + token_count - 1) as i32, &[0], true)?;
            match context.decode(&mut batch) {
                Ok(()) => {},
                Err(e) => {
                    println!("\n❌ DEBUG: Failed to decode token at step {}: {}", generation_step, e);
                    break;
                }
            }
            
            // Simple stop condition
            if response.trim_end().ends_with("Human:") {
                println!("\n🔍 DEBUG: Stop pattern detected");
                break;
            }
        }
        
        let generation_time = std::time::SystemTime::now().duration_since(generation_start).unwrap_or_default();
        let tps = if generation_time.as_secs_f64() > 0.0 {
            token_count as f64 / generation_time.as_secs_f64()
        } else {
            0.0
        };
        
        println!("\n🔍 DEBUG: Generated {} tokens in {:.2}s ({:.1} t/s)", 
                token_count, generation_time.as_secs_f64(), tps);
        
        println!("🔍 DEBUG: Response length: {} chars", response.len());
        println!("🔍 DEBUG: Turn #{} completed successfully", turn_count);
        io::stdout().flush()?;
        
        // Try to force garbage collection or cleanup
        println!("🔍 DEBUG: Dropping context for cleanup...");
        drop(context);
        println!("🔍 DEBUG: Context dropped, ready for next turn");
        io::stdout().flush()?;
    }
    
    println!("🔍 DEBUG: Chat ended after {} turns", turn_count);
    Ok(())
}