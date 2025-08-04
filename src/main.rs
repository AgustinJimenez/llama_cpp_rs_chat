use std::fs::{self, File};
use std::io::{self, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::thread;
use std::sync::mpsc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
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

/// Simple markdown renderer for terminal output (currently unused)
#[allow(dead_code)]
fn render_markdown(text: &str) -> String {
    let mut result = String::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut in_code_block = false;
    let mut code_fence_lang = String::new();
    
    for line in lines {
        let trimmed = line.trim();
        
        // Handle code blocks
        if trimmed.starts_with("```") {
            if in_code_block {
                // End code block
                result.push_str("\x1B[0m"); // Reset formatting
                in_code_block = false;
                code_fence_lang.clear();
            } else {
                // Start code block
                code_fence_lang = trimmed[3..].trim().to_string();
                result.push_str("\x1B[40m\x1B[37m"); // Dark background, white text
                in_code_block = true;
            }
            result.push('\n');
            continue;
        }
        
        if in_code_block {
            // Inside code block - preserve formatting
            result.push_str(&format!("\x1B[40m\x1B[37m{}\x1B[0m\n", line));
            continue;
        }
        
        let mut formatted_line = String::new();
        let mut chars = line.chars().peekable();
        
        while let Some(ch) = chars.next() {
            match ch {
                // Headers
                '#' if formatted_line.is_empty() => {
                    let mut header_level = 1;
                    while chars.peek() == Some(&'#') {
                        chars.next();
                        header_level += 1;
                    }
                    if chars.peek() == Some(&' ') {
                        chars.next(); // consume space
                    }
                    
                    let header_color = match header_level {
                        1 => "\x1B[1;36m", // Bold cyan
                        2 => "\x1B[1;35m", // Bold magenta  
                        3 => "\x1B[1;33m", // Bold yellow
                        _ => "\x1B[1;32m", // Bold green
                    };
                    
                    formatted_line.push_str(header_color);
                    let rest: String = chars.collect();
                    formatted_line.push_str(&rest);
                    formatted_line.push_str("\x1B[0m");
                    break;
                },
                
                // Bold **text**
                '*' if chars.peek() == Some(&'*') => {
                    chars.next(); // consume second *
                    formatted_line.push_str("\x1B[1m"); // Bold
                    
                    // Find closing **
                    let mut bold_text = String::new();
                    let mut found_closing = false;
                    while let Some(ch) = chars.next() {
                        if ch == '*' && chars.peek() == Some(&'*') {
                            chars.next(); // consume second *
                            found_closing = true;
                            break;
                        }
                        bold_text.push(ch);
                    }
                    
                    formatted_line.push_str(&bold_text);
                    if found_closing {
                        formatted_line.push_str("\x1B[0m"); // Reset
                    }
                },
                
                // Inline code `text`
                '`' => {
                    formatted_line.push_str("\x1B[43m\x1B[30m"); // Yellow background, black text
                    
                    let mut code_text = String::new();
                    let mut found_closing = false;
                    while let Some(ch) = chars.next() {
                        if ch == '`' {
                            found_closing = true;
                            break;
                        }
                        code_text.push(ch);
                    }
                    
                    formatted_line.push_str(&code_text);
                    if found_closing {
                        formatted_line.push_str("\x1B[0m"); // Reset
                    }
                },
                
                // List items
                '-' if formatted_line.trim().is_empty() && chars.peek() == Some(&' ') => {
                    chars.next(); // consume space
                    formatted_line.push_str("\x1B[36m• \x1B[0m"); // Cyan bullet
                },
                
                // Regular character
                _ => formatted_line.push(ch),
            }
        }
        
        result.push_str(&formatted_line);
        result.push('\n');
    }
    
    result
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(PartialEq)]
enum PromptFormat {
    Mistral,
    Qwen,
}

fn main() -> Result<()> {
    clear_terminal();
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
    let gguf_file = Path::new(model_path_trimmed).to_path_buf();
    if !gguf_file.exists()
        || gguf_file
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
            != "gguf"
    {
        return Err(anyhow::anyhow!("Provided path is not a valid .gguf file"));
    }

    let prompt_format = detect_prompt_format(&gguf_file);

    println!("\n- Set max context size (n_ctx, default 8192): ");
    let mut n_ctx_input = String::new();
    io::stdin().read_line(&mut n_ctx_input)?;
    let n_ctx = n_ctx_input.trim().parse::<u32>().unwrap_or(8192);
    let n_ctx_nonzero = NonZeroU32::new(n_ctx);

    let backend = LlamaBackend::init()?;
    let model = LlamaModel::load_from_file(&backend, &gguf_file, &Default::default())?;

    let ctx_params = LlamaContextParams::default().with_n_ctx(n_ctx_nonzero);
    let mut ctx = model.new_context(&backend, ctx_params)?;
    let mut sampler = LlamaSampler::greedy();

    let mut conversation: Vec<ChatMessage> = Vec::new();
    let convo_id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let convo_path = format!("assets/conversations/chat_{}.json", convo_id);

    let system_prompt = match prompt_format {
        PromptFormat::Mistral => {
            "<s>[INST] <<SYS>>\nYou are Devstral, a helpful agentic model trained by Mistral AI.\n<</SYS>>\n"
        }
        PromptFormat::Qwen => "<|im_start|>system\nYou are a helpful assistant.<|im_end|>",
    };

    conversation.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
    });
    save_conversation(&conversation, &convo_path)?;

    clear_terminal();
    
    println!("\n\n\x1B[1;33m🚀 Interactive Chat Started\x1B[0m \x1B[90m(type 'exit' to quit)\x1B[0m\n");

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

        let mut full_prompt = String::new();
        let mut system_added = false;

        for msg in &conversation {
            match prompt_format {
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
                    "user" => full_prompt
                        .push_str(&format!("\n<|im_start|>user\n{}<|im_end|>", msg.content)),
                    "assistant" => full_prompt.push_str(&format!(
                        "\n<|im_start|>assistant\n{}<|im_end|>",
                        msg.content
                    )),
                    _ => (),
                },
            }
        }

        // Add assistant role opening for the response
        match prompt_format {
            PromptFormat::Qwen => full_prompt.push_str("\n<|im_start|>assistant\n"),
            PromptFormat::Mistral => (), // Mistral format is ready after [/INST]
        }

        // Clear the KV cache to start fresh
        debug_print!("[DEBUG] Clearing KV cache");
        ctx.clear_kv_cache();

        debug_print!("[DEBUG] Tokenizing prompt: {} chars", full_prompt.len());
        let tokens = model.str_to_token(&full_prompt, AddBos::Never)?;
        debug_print!("[DEBUG] Got {} tokens | Context: {}/{} ({} remaining)", 
                    tokens.len(), tokens.len(), n_ctx, n_ctx as usize - tokens.len());
        
        let mut batch = LlamaBatch::new(1024, 1);
        for (i, &token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch.add(token, i as i32, &[0], is_last)?;
        }

        debug_print!("[DEBUG] About to decode batch");
        ctx.decode(&mut batch)?;
        debug_print!("[DEBUG] Batch decoded successfully");

        // Initialize response tracking variables
        let mut response = String::new();           // Complete response buffer
        let mut printed_response = String::new();   // What we've already printed to console
        print!("\n\x1B[32mAssistant: \x1B[0m");  // Green color for "Assistant:"
        io::stdout().flush().unwrap();

        let mut token_pos = tokens.len() as i32;
        
        // Define stop strings based on model format
        let stop_strings = if prompt_format == PromptFormat::Qwen {
            vec!["<|im_end|>", "<|end|>"]
        } else {
            vec!["</s>"]
        };

        // Set up interrupt detection (temporarily disabled)
        // println!("\x1B[90m(Press Enter to stop generation early)\x1B[0m");
        // let (interrupt_rx, interrupt_handle) = check_for_interrupt();
        // debug_print!("[DEBUG] Interrupt detection set up");
        
        // Main token generation loop
        debug_print!("[DEBUG] Starting token generation loop");
        let mut token_count = 0;
        let available_tokens = (n_ctx as usize).saturating_sub(tokens.len());
        let max_tokens = if available_tokens > 100 {
            available_tokens - 100 // Leave 100 token safety buffer
        } else {
            50 // Minimum tokens for a response, even if context is nearly full
        };
        debug_print!("[DEBUG] Dynamic token limit: {} (context: {}, used: {}, available: {}, buffer: {})", 
                    max_tokens, n_ctx, tokens.len(), available_tokens, if available_tokens > 100 { 100 } else { 0 });
        
        // Check if context is nearly exhausted
        if available_tokens < 20 {
            println!("\n\x1B[1;31m🚨 Warning: Context nearly full! Only {} tokens available. Consider starting a new conversation.\x1B[0m", available_tokens);
        }
        
        let mut recent_tokens: Vec<i32> = Vec::new();
        
        loop {
            token_count += 1;
            if token_count > max_tokens {
                debug_print!("[DEBUG] Hit max token limit ({}), stopping generation", max_tokens);
                println!("\n\x1B[33m⏹️  Stopped: Maximum token limit reached ({} tokens generated)\x1B[0m", token_count - 1);
                break;
            }
            // Check for user interrupt (temporarily disabled)
            // if let Ok(_) = interrupt_rx.try_recv() {
            //     debug_print!("[DEBUG] User interrupt detected");
            //     println!("\n\x1B[33m⏹️  Generation stopped by user\x1B[0m");
            //     break;
            // }
            // Sample next token from the model
            debug_print!("[DEBUG] About to sample token");
            let token = sampler.sample(&ctx, -1);
            debug_print!("[DEBUG] Sampled token: {}", token);
            
            // Track recent tokens for repetition detection
            recent_tokens.push(token.0);  // Convert LlamaToken to u32
            if recent_tokens.len() > 10 {
                recent_tokens.remove(0);
            }
            
            // Check for repetitive patterns (same token repeated)
            if recent_tokens.len() >= 5 {
                let last_token = recent_tokens[recent_tokens.len() - 1];
                let is_repeating = recent_tokens.iter().rev().take(5).all(|&t| t == last_token);
                if is_repeating {
                    debug_print!("[DEBUG] Detected repetitive pattern, stopping generation");
                    println!("\n\x1B[33m⏹️  Stopped: Repetitive pattern detected\x1B[0m");
                    break;
                }
            }
            
            // Check for end-of-sequence token
            if token == model.token_eos() {
                debug_print!("\n[DEBUG] Hit EOS token, stopping generation");
                break;
            }

            // Convert token to string and process it
            if let Ok(token_str) = model.token_to_str(token, Special::Tokenize) {
                debug_print!("\n[DEBUG] Generated token: '{}'", token_str.replace('\n', "\\n"));
                
                // Step 1: Add the new token to our complete response buffer
                response.push_str(&token_str);
                debug_print!("[DEBUG] Full response so far: '{}'", response.replace('\n', "\\n"));

                // Step 2: Check if adding this token completed any stop string
                let stop_result = check_for_stop_strings(&response, &stop_strings);
                
                if let Some(stop_pos) = stop_result {
                    debug_print!("[DEBUG] Found stop string at position {}", stop_pos);
                    
                    // Print any remaining text up to the stop position
                    print_remaining_text(&response, &mut printed_response, stop_pos);
                    
                    // Remove the stop string from final response
                    response.truncate(stop_pos);
                    debug_print!("[DEBUG] Final response after truncation: '{}'", response.replace('\n', "\\n"));
                    break;
                } else {
                    // No stop string found, safe to print this token
                    print_new_token(&response, &mut printed_response, &token_str, &stop_strings);
                }
            }

            // Prepare for next token generation
            batch.clear();
            if let Err(e) = batch.add(token, token_pos, &[0], true) {
                eprintln!("\nError adding token to batch: {}", e);
                break;
            }
            if let Err(e) = ctx.decode(&mut batch) {
                eprintln!("\nError decoding batch: {}", e);
                break;
            }
            token_pos += 1;
        }

        println!();

        // Calculate context usage
        let final_tokens = model.str_to_token(&full_prompt, AddBos::Never)?;
        let response_tokens = model.str_to_token(&response, AddBos::Never)?;
        let total_tokens_used = final_tokens.len() + response_tokens.len();
        let context_remaining = n_ctx as usize - total_tokens_used;
        let context_usage_percent = (total_tokens_used as f32 / n_ctx as f32 * 100.0).round() as u32;
        
        debug_print!("[DEBUG] Final context: Prompt={} + Response={} = Total={}/{} ({}% used, {} remaining)", 
                    final_tokens.len(), response_tokens.len(), total_tokens_used, n_ctx, 
                    context_usage_percent, context_remaining);
        
        // Display context usage with color coding
        let usage_color = if context_usage_percent >= 90 {
            "\x1B[1;31m"  // Bold red for high usage (>90%)
        } else if context_usage_percent >= 70 {
            "\x1B[1;33m"  // Bold yellow for medium usage (70-89%)
        } else {
            "\x1B[1;32m"  // Bold green for low usage (<70%)
        };
        
        println!("\x1B[90m📊 Context: {}{}/{}\x1B[0m \x1B[90m({}% used, {} tokens remaining)\x1B[0m", 
                usage_color, total_tokens_used, n_ctx, context_usage_percent, context_remaining);
        
        // Show warning if context is getting full
        if context_usage_percent >= 85 {
            println!("\x1B[1;33m⚠️  Warning: Context is {}% full. Consider starting a new conversation soon to avoid truncated responses.\x1B[0m", context_usage_percent);
        } else if context_usage_percent >= 95 {
            println!("\x1B[1;31m🚨 Critical: Context is {}% full! Responses may be cut short. Type 'exit' and start a new chat.\x1B[0m", context_usage_percent);
        }

        conversation.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.trim().to_string(),
        });
        save_conversation(&conversation, &convo_path)?;
    }

    Ok(())
}

fn detect_prompt_format(path: &PathBuf) -> PromptFormat {
    let name = path
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

fn ask_and_save_model_path(path_file: &Path) -> Result<String> {
    println!("\n\n- Please enter the path to the GGUF model file:");
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    fs::write(path_file, trimmed)?;
    Ok(trimmed.to_string())
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

/// Check if user wants to interrupt generation (non-blocking) - currently unused
#[allow(dead_code)]
fn check_for_interrupt() -> (mpsc::Receiver<()>, thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    
    let handle = thread::spawn(move || {
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            // User pressed any key (Enter to confirm)
            tx.send(()).ok();
        }
    });
    
    (rx, handle)
}

fn save_conversation(convo: &[ChatMessage], file_path: &str) -> Result<()> {
    let dir = Path::new("assets/conversations");
    fs::create_dir_all(dir)?;
    let file = File::create(file_path)?;
    serde_json::to_writer_pretty(file, &convo)?;
    Ok(())
}

/// Check if the response contains any stop strings and return the position of the first one found
fn check_for_stop_strings(response: &str, stop_strings: &[&str]) -> Option<usize> {
    for &stop_str in stop_strings {
        if let Some(pos) = response.find(stop_str) {
            debug_print!("[DEBUG] Found stop string '{}' at position {}", stop_str, pos);
            return Some(pos);
        }
    }
    None
}

/// Print any remaining text from the response up to the stop position
fn print_remaining_text(response: &str, printed_response: &mut String, stop_pos: usize) {
    if printed_response.len() < stop_pos {
        let to_print = &response[printed_response.len()..stop_pos];
        debug_print!("[DEBUG] Printing remaining text before stop: '{}'", to_print.replace('\n', "\\n"));
        print!("{}", to_print);
        io::stdout().flush().unwrap();
        printed_response.push_str(to_print);
    } else {
        debug_print!("[DEBUG] No remaining text to print (already printed up to stop position)");
    }
}

/// Print a new token if no stop string was found, using buffering to avoid printing partial stop strings
fn print_new_token(response: &str, printed_response: &mut String, _token_str: &str, stop_strings: &[&str]) {
    // Find the longest stop string to determine buffer size needed
    let max_stop_len = stop_strings.iter().map(|s| s.len()).max().unwrap_or(0);
    
    // Calculate what we could potentially print
    let available_to_print = &response[printed_response.len()..];
    debug_print!("[DEBUG] Available to print: '{}'", available_to_print.replace('\n', "\\n"));
    
    // If we have less content than the longest stop string, we need to be careful
    if available_to_print.len() < max_stop_len {
        // Check if any stop string could be starting at the end of our response
        let mut could_be_partial_stop = false;
        for &stop_str in stop_strings {
            // Check if the end of our response could be the beginning of this stop string
            for i in 1..=available_to_print.len().min(stop_str.len()) {
                if available_to_print.ends_with(&stop_str[..i]) {
                    debug_print!("[DEBUG] Potential partial stop string '{}' detected, holding back", &stop_str[..i]);
                    could_be_partial_stop = true;
                    break;
                }
            }
            if could_be_partial_stop { break; }
        }
        
        if could_be_partial_stop {
            debug_print!("[DEBUG] Holding back printing due to potential partial stop string");
            return;
        }
    }
    
    // Safe to print - either we have enough content or no partial stop string detected
    let to_print = if available_to_print.len() >= max_stop_len {
        // Print all but the last (max_stop_len - 1) characters to be safe
        let safe_print_len = available_to_print.len() - (max_stop_len - 1);
        &available_to_print[..safe_print_len]
    } else {
        // We already checked it's safe above
        available_to_print
    };
    
    if !to_print.is_empty() {
        debug_print!("[DEBUG] Printing safe content: '{}'", to_print.replace('\n', "\\n"));
        print!("{}", to_print);
        io::stdout().flush().unwrap();
        printed_response.push_str(to_print);
    } else {
        debug_print!("[DEBUG] Nothing safe to print yet");
    }
    
    debug_print!("[DEBUG] Total printed so far: '{}'", printed_response.replace('\n', "\\n"));
}
