use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

mod llm_backend;
mod llamacpp_backend;

use llm_backend::*;
use llamacpp_backend::LlamaCppBackendImpl;

fn initialize_backend() -> Result<()> {
    println!("🦙 Using LLaMA.cpp backend");
    Ok(())
}

/// Clear the terminal screen thoroughly
fn clear_terminal() {
    // Clear the terminal screen multiple times for thorough clearing
    for _ in 0..4 {
        print!("\x1B[2J\x1B[1;1H");
    }
    // Also clear scrollback buffer on supported terminals
    print!("\x1B[3J");
    io::stdout().flush().unwrap();
}

fn ask_and_save_model_path(path_file: &Path) -> Result<String> {
    println!("\n\n- Please enter the path to the GGUF model file:");
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    fs::write(path_file, trimmed)?;
    Ok(trimmed.to_string())
}

fn save_conversation(convo: &[ChatMessage], file_path: &str) -> Result<()> {
    let dir = Path::new("assets/conversations");
    fs::create_dir_all(dir)?;
    let file = File::create(file_path)?;
    serde_json::to_writer_pretty(file, &convo)?;
    Ok(())
}

fn detect_prompt_format(path: &str) -> PromptFormat {
    let name = Path::new(path)
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

fn run_chat_with_backend<T: LLMBackend>(mut backend: T) -> Result<()> {
    let mut conversation: Vec<ChatMessage> = Vec::new();
    let convo_id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let convo_path = format!("assets/conversations/chat_{}.json", convo_id);

    // Add system message based on backend
    let system_prompt = match backend.backend_name() {
        "candle" => "You are a helpful AI assistant powered by Candle.",
        _ => "You are a helpful AI assistant.",
    };

    conversation.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
    });
    save_conversation(&conversation, &convo_path)?;

    println!("\n\n\x1B[1;33m🚀 Interactive Chat Started\x1B[0m \x1B[90m(type 'exit' to quit)\x1B[0m\n");
    println!("🔧 Backend: {}", backend.backend_name());

    loop {
        print!("\n\n\x1B[36mYou: \x1B[0m");  // Cyan color for "You:"
        io::stdout().flush().unwrap();

        let mut user_input = String::new();
        if io::stdin().read_line(&mut user_input).is_err() {
            println!("\nError reading input.");
            break;
        }

        let user_input = user_input.trim();
        if user_input.eq_ignore_ascii_case("exit") {
            println!("\n\x1B[1;31m👋 Ending chat session...\x1B[0m");
            break;
        }

        if user_input.is_empty() {
            continue;
        }

        conversation.push(ChatMessage {
            role: "user".to_string(),
            content: user_input.to_string(),
        });
        save_conversation(&conversation, &convo_path)?;

        print!("\n\x1B[32mAssistant: \x1B[0m");  // Green color for "Assistant:"
        io::stdout().flush().unwrap();

        // Setup generation config
        let gen_config = GenerationConfig {
            max_tokens: 4096,
            stop_strings: vec!["<|im_end|>".to_string(), "<|end|>".to_string(), "</s>".to_string()],
        };

        // Generate response with token callback
        let response = backend.generate_response(
            &conversation,
            gen_config,
            Box::new(|token_info| {
                print!("{}", token_info.token_str);
                io::stdout().flush().unwrap();
                true // Continue generation
            })
        )?;

        println!();

        // Get context information
        let context_info = backend.get_context_info(&conversation, &response)?;
        
        // Display context usage with color coding
        let usage_color = if context_info.usage_percent >= 90 {
            "\x1B[1;31m"  // Bold red for high usage (>90%)
        } else if context_info.usage_percent >= 70 {
            "\x1B[1;33m"  // Bold yellow for medium usage (70-89%)
        } else {
            "\x1B[1;32m"  // Bold green for low usage (<70%)
        };
        
        println!("\x1B[90m📊 Context: {}{}/{}\x1B[0m \x1B[90m({}% used, {} tokens remaining) \x1B[90m[{}]\x1B[0m", 
                usage_color, context_info.total_tokens, context_info.context_size, 
                context_info.usage_percent, context_info.context_size - context_info.total_tokens,
                backend.backend_name());
        
        // Show warning if context is getting full
        if context_info.usage_percent >= 85 {
            println!("\x1B[1;33m⚠️  Warning: Context is {}% full. Consider starting a new conversation soon to avoid truncated responses.\x1B[0m", context_info.usage_percent);
        } else if context_info.usage_percent >= 95 {
            println!("\x1B[1;31m🚨 Critical: Context is {}% full! Responses may be cut short. Type 'exit' and start a new chat.\x1B[0m", context_info.usage_percent);
        }

        conversation.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.trim().to_string(),
        });
        save_conversation(&conversation, &convo_path)?;
    }

    Ok(())
}

fn main() -> Result<()> {
    clear_terminal();
    
    // Initialize backend
    initialize_backend()?;
    
    let model_path_file = Path::new("assets/model_path.txt");
    let model_path: String = if model_path_file.exists() {
        let prev_path = fs::read_to_string(model_path_file)?.trim().to_string();
        
        loop {
            println!("\n\n- Use previous model path? [{}] (Y/n) (default Y)", prev_path);
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            
            match answer.trim().to_lowercase().as_str() {
                "y" | "" => break prev_path,
                "n" => break ask_and_save_model_path(model_path_file)?,
                _ => {
                    println!("\n\x1B[31m❌ Invalid input. Please enter 'Y' for yes or 'n' for no.\x1B[0m");
                    continue;
                }
            }
        }
    } else {
        ask_and_save_model_path(model_path_file)?
    };

    let model_path_trimmed = model_path.trim();
    let gguf_file = Path::new(model_path_trimmed);
    if !gguf_file.exists() || gguf_file.extension().unwrap_or_default().to_string_lossy().to_lowercase() != "gguf" {
        return Err(anyhow::anyhow!("Provided path is not a valid .gguf file"));
    }

    let prompt_format = detect_prompt_format(model_path_trimmed);

    println!("\n- Set max context size (n_ctx, default 8192): ");
    let mut n_ctx_input = String::new();
    io::stdin().read_line(&mut n_ctx_input)?;
    let n_ctx = n_ctx_input.trim().parse::<u32>().unwrap_or(8192);

    // Create model configuration
    let model_config = ModelConfig {
        context_size: n_ctx,
        model_path: model_path_trimmed.to_string(),
        prompt_format,
    };

    // Initialize and run LLaMA.cpp backend
    println!("🦙 Initializing LLaMA.cpp backend...");
    let backend = LlamaCppBackendImpl::initialize(model_config)?;
    run_chat_with_backend(backend)
}