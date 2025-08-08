use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use anyhow::Result;

mod llm_backend;
mod llamacpp_backend;

use llm_backend::*;
use llamacpp_backend::LlamaCppBackendImpl;

fn main() -> Result<()> {
    println!("🦙 Simple LLaMA Chat Test");
    
    // Use hardcoded model path
    let model_path = r"E:\.lmstudio\models\lmstudio-community\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL-GGUF\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL.gguf".to_string();
    
    // Basic model config - no GPU layers, simple context
    let model_config = ModelConfig {
        context_size: 32000,
        model_path,
        prompt_format: PromptFormat::Qwen, // Qwen model format
        n_gpu_layers: 0, // CPU only for testing
    };
    
    println!("🚀 Loading model...");
    let mut backend = LlamaCppBackendImpl::initialize(model_config)?;
    
    // Simple conversation loop
    let mut conversation: Vec<ChatMessage> = Vec::new();
    
    // Add simple system message
    conversation.push(ChatMessage {
        role: "system".to_string(),
        content: "You are a helpful AI assistant.".to_string(),
    });
    
    println!("\n💬 Chat started! Type 'exit' to quit.\n");
    println!("🔧 Debug mode enabled - will show detailed logs\n");
    println!("🔧 First message will be auto-sent for debugging purposes\n");
    
    let mut turn_count = 0;
    
    loop {
        turn_count += 1;
        println!("🔍 DEBUG: Starting turn #{}", turn_count);
        io::stdout().flush()?; // Ensure turn number is shown immediately
        
        let user_input = if turn_count == 1 {
            // Auto-send the first message for debugging
            let auto_message = "make me a laravel mvc people crud";
            println!("🔍 DEBUG: Auto-sending first message: '{}'", auto_message);
            println!("You: {}", auto_message);
            io::stdout().flush()?;
            auto_message.to_string()
        } else {
            // Normal input handling for subsequent messages
            print!("You: ");
            io::stdout().flush()?; // Ensure prompt is shown immediately
            
            let mut input = String::new();
            println!("🔍 DEBUG: Reading user input...");
            io::stdout().flush()?; // Ensure debug message is shown
            
            match io::stdin().read_line(&mut input) {
                Ok(0) => {
                    println!("🔍 DEBUG: EOF detected (Ctrl+Z or pipe closed), ending chat");
                    io::stdout().flush()?;
                    break;
                }
                Ok(bytes) => {
                    println!("🔍 DEBUG: Read {} bytes from stdin", bytes);
                    io::stdout().flush()?;
                }
                Err(e) => {
                    println!("❌ DEBUG: Error reading stdin: {} (kind: {:?})", e, e.kind());
                    io::stdout().flush()?;
                    
                    // Try to determine what happened
                    if e.kind() == io::ErrorKind::UnexpectedEof {
                        println!("🔍 DEBUG: Unexpected EOF - input stream was closed");
                    } else if e.kind() == io::ErrorKind::Interrupted {
                        println!("🔍 DEBUG: Read was interrupted (Ctrl+C or signal)");
                    } else {
                        println!("🔍 DEBUG: Unknown stdin error - this might be a terminal issue");
                    }
                    io::stdout().flush()?;
                    break;
                }
            }
            
            input.trim().to_string()
        };
        
        println!("🔍 DEBUG: Processing input: '{}'", user_input);
        
        if user_input.eq_ignore_ascii_case("exit") {
            println!("🔍 DEBUG: Exit command received");
            break;
        }
        
        if user_input.is_empty() {
            println!("🔍 DEBUG: Empty input, continuing...");
            continue;
        }
        
        // Add user message
        println!("🔍 DEBUG: Adding user message to conversation");
        conversation.push(ChatMessage {
            role: "user".to_string(),
            content: user_input.to_string(),
        });
        println!("🔍 DEBUG: Conversation length now: {} messages", conversation.len());
        
        print!("Assistant: ");
        io::stdout().flush()?;
        println!("🔍 DEBUG: Starting AI response generation...");
        
        // Calculate dynamic max_tokens based on available context
        println!("🔍 DEBUG: Calculating available context space...");
        let current_context_info = match backend.get_context_info(&conversation, "") {
            Ok(info) => {
                println!("🔍 DEBUG: Current context usage: {}/{} ({}%)", 
                        info.total_tokens, info.context_size, info.usage_percent);
                info
            }
            Err(e) => {
                println!("❌ DEBUG: Could not get context info, using fallback: {}", e);
                // Fallback values
                ContextInfo {
                    prompt_tokens: 800,  // Conservative estimate for prompt
                    response_tokens: 200, // Conservative estimate for responses
                    total_tokens: 1000, // Conservative estimate
                    context_size: 4096,
                    usage_percent: 25,
                }
            }
        };
        
        let remaining_tokens = current_context_info.context_size.saturating_sub(current_context_info.total_tokens);
        // Reserve some tokens for safety buffer (10% of context or minimum 100 tokens)
        let safety_buffer = std::cmp::max(current_context_info.context_size / 10, 100);
        let available_tokens = remaining_tokens.saturating_sub(safety_buffer);
        
        // Set a reasonable minimum (256) and maximum (2048) for response length
        let dynamic_max_tokens = std::cmp::min(std::cmp::max(available_tokens, 256), 2048);
        
        println!("🔍 DEBUG: Context calculation - Remaining: {}, Safety buffer: {}, Available: {}, Using: {}", 
                remaining_tokens, safety_buffer, available_tokens, dynamic_max_tokens);
        
        let gen_config = GenerationConfig {
            max_tokens: dynamic_max_tokens,
            stop_strings: vec!["</s>".to_string()],
        };
        
        println!("🔍 DEBUG: Generation config - max_tokens: {}, stop_strings: {:?}", 
                gen_config.max_tokens, gen_config.stop_strings);
        
        let generation_start = std::time::SystemTime::now();
        let token_count = Arc::new(Mutex::new(0u32));
        let token_count_clone = Arc::clone(&token_count);
        
        println!("🔍 DEBUG: Calling backend.generate_response...");
        match backend.generate_response(
            &conversation,
            gen_config,
            Box::new(move |token_info| {
                print!("{}", token_info.token_str);
                let mut count = token_count_clone.lock().unwrap();
                *count += 1;
                io::stdout().flush().unwrap();
                true
            }),
        ) {
            Ok(response) => {
                println!(); // New line after response
                
                let total_elapsed = std::time::SystemTime::now().duration_since(generation_start).unwrap_or_default();
                let final_count = *token_count.lock().unwrap();
                
                println!("🔍 DEBUG: Response generation completed successfully");
                println!("🔍 DEBUG: Response length: {} chars, {} tokens generated in {:.2}s", 
                        response.len(), final_count, total_elapsed.as_secs_f64());
                println!("🔍 DEBUG: Response preview: '{}'", 
                        if response.len() > 100 { &response[..100] } else { &response });
                
                // Add assistant response to conversation
                println!("🔍 DEBUG: Adding assistant response to conversation");
                conversation.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: response.trim().to_string(),
                });
                println!("🔍 DEBUG: Conversation length now: {} messages", conversation.len());
                
                // Show context usage
                println!("🔍 DEBUG: Getting context info...");
                match backend.get_context_info(&conversation, &response) {
                    Ok(context_info) => {
                        println!("🔍 DEBUG: Context info retrieved successfully");
                        println!("🔍 DEBUG: Total tokens: {}, Context size: {}, Usage: {}%", 
                                context_info.total_tokens, context_info.context_size, context_info.usage_percent);
                        
                        let usage_color = if context_info.usage_percent >= 90 {
                            "\x1B[1;31m" // Red
                        } else if context_info.usage_percent >= 70 {
                            "\x1B[1;33m" // Yellow
                        } else {
                            "\x1B[1;32m" // Green
                        };
                        
                        println!(
                            "\x1B[90m📊 Context: {}{}/{}\x1B[0m \x1B[90m({}% used, {} tokens remaining)\x1B[0m",
                            usage_color,
                            context_info.total_tokens,
                            context_info.context_size,
                            context_info.usage_percent,
                            context_info.context_size - context_info.total_tokens
                        );
                        
                        if context_info.usage_percent >= 85 {
                            println!("🔍 DEBUG: ⚠️ High context usage detected ({}%)", context_info.usage_percent);
                        }
                    }
                    Err(e) => {
                        println!("❌ DEBUG: Failed to get context info: {}", e);
                        println!("\x1B[90m⚠️ Could not get context info: {}\x1B[0m", e);
                    }
                }
            }
            Err(e) => {
                println!("\n❌ DEBUG: Response generation failed with error: {}", e);
                println!("🔍 DEBUG: Error type: {:?}", e);
                println!("🔍 DEBUG: This might indicate memory issues, context overflow, or model problems");
                break;
            }
        }
        
        println!("🔍 DEBUG: Turn #{} completed successfully", turn_count);
        
        // Check system resources periodically
        if turn_count % 3 == 0 {
            #[cfg(feature = "sysinfo")]
            {
                let mut sys = sysinfo::System::new();
                sys.refresh_memory();
                let used_memory = sys.used_memory();
                let total_memory = sys.total_memory();
                let memory_percent = (used_memory as f64 / total_memory as f64) * 100.0;
                println!("🔍 DEBUG: System memory usage: {:.1}% ({} MB used / {} MB total)", 
                        memory_percent, used_memory / 1024 / 1024, total_memory / 1024 / 1024);
            }
        }
        
        println!(); // Extra space before next prompt
    }
    
    println!("🔍 DEBUG: Exiting chat loop after {} turns", turn_count);
    println!("👋 Chat ended!");
    Ok(())
}