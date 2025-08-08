// Standard library imports for I/O operations, error handling, and system commands
use std::io::{self, Write};
use anyhow::Result;
use std::num::NonZeroU32;
use std::process::Command;
use std::fs;
use serde_json::json;

// llama-cpp-2 crate imports for LLM inference
use llama_cpp_2::{
    context::params::LlamaContextParams,  // Configuration for inference context
    llama_backend::LlamaBackend,          // Backend initialization
    llama_batch::LlamaBatch,              // Batch processing for tokens
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special}, // Model loading and tokenization
    sampling::LlamaSampler,               // Token sampling strategies
};

// Data structure to represent a single turn in the conversation
// Each turn has a role (system/user/assistant) and content (the actual message)
#[derive(Debug, Clone)]
struct ChatTurn {
    role: String,   // "system" | "user" | "assistant"
    content: String,
}

// Execute system commands extracted from AI responses
// This function runs shell commands and returns their output as a formatted string
fn execute_command(command: &str) -> String {
    println!("\n🔧 EXECUTING COMMAND: {}", command);
    io::stdout().flush().unwrap();

    // Cross-platform command execution - use cmd on Windows, sh on Unix-like systems
    let output = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", command]).output()
    } else {
        Command::new("sh").args(["-c", command]).output()
    };

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);

            let mut output_text = String::new();
            if !stdout.is_empty() {
                output_text.push_str("STDOUT:\n");
                output_text.push_str(&stdout);
            }
            if !stderr.is_empty() {
                if !output_text.is_empty() {
                    output_text.push_str("\n");
                }
                output_text.push_str("STDERR:\n");
                output_text.push_str(&stderr);
            }
            if output_text.is_empty() {
                output_text = "Command executed successfully (no output)".to_string();
            }

            println!("📤 COMMAND OUTPUT:\n{}", output_text);
            output_text
        }
        Err(e) => {
            let error_text = format!("Failed to execute command: {}", e);
            println!("❌ COMMAND ERROR: {}", error_text);
            error_text
        }
    }
}

// Parse AI responses to extract command execution requests
// Commands are wrapped in <|EXEC|>command<|/EXEC|> tags
fn extract_commands_from_response(response: &str) -> Vec<String> {
    let mut commands = Vec::new();

    // Search for command tags throughout the response text
    // This allows multiple commands in a single response
    let mut start = 0;
    while let Some(start_pos) = response[start..].find("<|EXEC|>") {
        let actual_start = start + start_pos + 8; // 8 = length of "<|EXEC|>"
        if let Some(end_pos) = response[actual_start..].find("<|/EXEC|>") {
            let actual_end = actual_start + end_pos;
            let command = response[actual_start..actual_end].trim();
            if !command.is_empty() {
                commands.push(command.to_string());
            }
            start = actual_end + 9; // 9 = length of "<|/EXEC|>"
        } else {
            break;
        }
    }

    commands
}

fn check_available_memory() -> f64 {
    // Try to get available memory in GB
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = Command::new("vm_stat").output() {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                // Parse vm_stat output to estimate available memory
                if let Some(free_line) = stdout.lines().find(|line| line.contains("Pages free:")) {
                    if let Some(free_str) = free_line.split(':').nth(1) {
                        if let Ok(free_pages) = free_str.trim().replace('.', "").parse::<u64>() {
                            // Each page is 4KB on macOS
                            let free_bytes = free_pages * 4096;
                            return free_bytes as f64 / 1_073_741_824.0; // Convert to GB
                        }
                    }
                }
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            if let Some(available_line) = meminfo.lines().find(|line| line.starts_with("MemAvailable:")) {
                if let Some(kb_str) = available_line.split_whitespace().nth(1) {
                    if let Ok(kb) = kb_str.parse::<u64>() {
                        return kb as f64 / 1_048_576.0; // Convert KB to GB
                    }
                }
            }
        }
    }
    
    // Default fallback - assume reasonable memory is available
    8.0
}

// Persist conversation history to JSON file for later analysis or continuation
// This creates a permanent record of the chat session
fn save_conversation(conversation: &[serde_json::Value], conversation_file: &str) -> Result<()> {
    // Ensure the conversations directory exists (create if missing)
    fs::create_dir_all("assets/conversations")?;

    // Convert conversation array to pretty-printed JSON and write to file
    let json_content = serde_json::to_string_pretty(conversation)?;
    fs::write(conversation_file, json_content)?;

    println!("🔍 DEBUG: Conversation saved to {}", conversation_file);
    Ok(())
}

// Format conversation history using Qwen-3's specific chat template
// The <|im_start|>/<|im_end|> format is required for proper model inference
fn get_qwen_base_prompt(system_prompt: &str, history: &[ChatTurn]) -> String {
    // Build the complete prompt with system message and conversation history
    // This ensures the model understands the context and can generate appropriate responses

    let mut s = String::new();
    // Start with system prompt (instructions for the AI's behavior)
    s.push_str(&format!("<|im_start|>system\n{}\n<|im_end|>\n", system_prompt));

    // Add all previous conversation turns in chronological order
    for turn in history {
        match turn.role.as_str() {
            "user" => {
                s.push_str(&format!("<|im_start|>user\n{}\n<|im_end|>\n", turn.content));
            }
            "assistant" => {
                s.push_str(&format!("<|im_start|>assistant\n{}\n<|im_end|>\n", turn.content));
            }
            _ => {} // Ignore unknown roles
        }
    }

    // Add assistant header to prompt the model to generate a response
    s.push_str("<|im_start|>assistant\n");
    s
}

// === INITIALIZATION FUNCTIONS ===

// Initialize conversation tracking and file paths
fn initialize_conversation() -> Result<(String, Vec<serde_json::Value>, Vec<ChatTurn>, u32)> {
    let conversation_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let conversation_file = format!("assets/conversations/chat_{}.json", conversation_id);
    let conversation_json: Vec<serde_json::Value> = Vec::new();
    let history: Vec<ChatTurn> = Vec::new();
    let turn_count = 0;
    
    Ok((conversation_file, conversation_json, history, turn_count))
}

// Initialize LLM backend, model, and sampler
fn initialize_model(model_path: &str) -> Result<(LlamaBackend, LlamaModel, LlamaSampler, f64)> {
    println!("🔍 DEBUG: Initializing backend...");
    io::stdout().flush()?;
    let backend = LlamaBackend::init()?;

    println!("🔍 DEBUG: Loading model...");
    io::stdout().flush()?;
    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, model_path, &model_params)?;

    println!("🔍 DEBUG: Creating sampler...");
    io::stdout().flush()?;
    let sampler = LlamaSampler::greedy();

    // Check system memory for optimization
    println!("🔍 DEBUG: Checking system memory...");
    let available_memory_gb = check_available_memory();
    println!("🔍 DEBUG: Estimated available memory: {:.1} GB", available_memory_gb);
    
    if available_memory_gb < 4.0 {
        println!("⚠️ WARNING: Low memory detected ({:.1} GB). Using minimal settings.", available_memory_gb);
    }

    println!("🔍 DEBUG: Model loaded successfully");
    io::stdout().flush()?;

    Ok((backend, model, sampler, available_memory_gb))
}

// === USER INPUT/OUTPUT FUNCTIONS ===

// Handle user input with error checking and special commands
fn get_user_input(turn_count: u32) -> Result<Option<String>> {
    println!("\n🔍 DEBUG: Starting turn #{}", turn_count);
    io::stdout().flush()?;

    print!("You: ");
    io::stdout().flush()?;

    println!("🔍 DEBUG: Reading user input...");
    io::stdout().flush()?;

    let mut input = String::new();
    let user_input = match io::stdin().read_line(&mut input) {
        Ok(0) => {
            println!("🔍 DEBUG: EOF detected, ending chat");
            return Ok(None);  // Signal to exit
        }
        Ok(bytes) => {
            println!("🔍 DEBUG: Read {} bytes", bytes);
            io::stdout().flush()?;
            input.trim().to_string()
        }
        Err(e) => {
            println!("❌ DEBUG: Error reading: {}", e);
            return Ok(None);  // Signal to exit
        }
    };

    // Handle special commands and empty input
    if user_input.eq_ignore_ascii_case("exit") { 
        return Ok(None);  // Signal to exit
    }
    if user_input.is_empty() { 
        return Ok(Some(String::new()));  // Signal to continue but skip processing
    }

    println!("🔍 DEBUG: Processing input: '{}'", user_input);
    io::stdout().flush()?;

    Ok(Some(user_input))
}

// Generate dynamic system prompt with current environment information
fn create_system_prompt() -> String {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let os_name = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    format!(
        "You are an intelligent AI assistant. You communicate with the user and execute system commands when requested.\n\n\
        SYSTEM INFORMATION:\n\
        - Operating System: {} ({})\n\
        - Current Directory: {}\n\
        - Architecture: {}\n\n\
        RESPONSE FORMAT:\n\
        1. Always explain what you're doing before executing commands\n\
        2. Execute the command using <|EXEC|>command<|/EXEC|> tags\n\
        3. Provide additional help or context after the command if useful\n\
        4. Continue conversing normally - don't stop after commands\n\n\
        COMMAND EXAMPLES:\n\
        User: \"list files\"\n\
        Assistant: \"I'll list the files in the current directory for you.\n\
        <|EXEC|>ls -la<|/EXEC|>\n\
        This shows all files including hidden ones with detailed information.\"\n\n\
        IMPORTANT:\n\
        - Always provide conversational context around commands\n\
        - Don't end responses immediately after executing commands\n\
        - Be helpful and explain what the commands do",
        os_name,
        match os_name {
            "windows" => "Windows",
            "macos" => "macOS",
            "linux" => "Linux", 
            _ => "Unix/Linux"
        },
        current_dir.display(),
        arch
    )
}

// === CONTEXT MANAGEMENT FUNCTIONS ===

// Create inference context with adaptive sizing to prevent memory overflow
fn create_context_with_retry<'a>(
    model: &'a LlamaModel,
    backend: &'a LlamaBackend,
    available_memory_gb: f64
) -> Result<(llama_cpp_2::context::LlamaContext<'a>, usize)> {
    println!("🔍 DEBUG: Creating context...");
    io::stdout().flush()?;
    
    // Start with context size based on available memory
    let mut n_ctx: usize = if available_memory_gb < 4.0 {
        2048  // Very conservative for low memory
    } else if available_memory_gb < 8.0 {
        4096  // Conservative for moderate memory
    } else {
        8192  // Normal size for adequate memory
    };

    // Try creating context with progressive size reduction on failure
    let context = loop {
        println!("🔍 DEBUG: Trying context size: {}", n_ctx);
        io::stdout().flush()?;
        
        let n_ctx_nonzero = NonZeroU32::new(n_ctx as u32);
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(n_ctx_nonzero);
            
        match model.new_context(backend, ctx_params) {
            Ok(ctx) => {
                println!("🔍 DEBUG: Successfully created context with size {}", n_ctx);
                break ctx;
            },
            Err(e) => {
                println!("❌ DEBUG: Failed to create context with size {}: {}", n_ctx, e);
                n_ctx /= 2;
                if n_ctx < 2048 {
                    return Err(anyhow::anyhow!("Cannot create context even with minimum size"));
                }
                println!("🔍 DEBUG: Retrying with smaller context size: {}", n_ctx);
            }
        }
    };

    Ok((context, n_ctx))
}

// Process prompt tokens through the model with error recovery
fn decode_initial_batch(
    context: &mut llama_cpp_2::context::LlamaContext,
    tokens: &[llama_cpp_2::token::LlamaToken],
    n_ctx: usize
) -> Result<LlamaBatch> {
    println!("🔍 DEBUG: Tokenizing prompt with {} tokens", tokens.len());
    io::stdout().flush()?;

    println!("🔍 DEBUG: Clearing context and loading tokens...");
    io::stdout().flush()?;
    context.clear_kv_cache();

    // Calculate appropriate batch size based on actual context size
    let available_tokens = n_ctx.saturating_sub(tokens.len());
    let batch_size = std::cmp::min(512, std::cmp::max(256, available_tokens));
    println!("🔍 DEBUG: Creating batch size {} for {} tokens", batch_size, tokens.len());
    io::stdout().flush()?;

    // Build batch with all prompt tokens
    let mut batch = LlamaBatch::new(batch_size, 1);
    for (i, &token) in tokens.iter().enumerate() {
        let logits = i == tokens.len() - 1; // Only last token needs logits for next token prediction
        batch.add(token, i as i32, &[0], logits)?;
    }

    // Try initial decode with error recovery
    match context.decode(&mut batch) {
        Ok(()) => {
            println!("🔍 DEBUG: Initial batch decoded successfully");
            Ok(batch)
        }
        Err(e) => {
            println!("❌ DEBUG: Failed to decode initial batch: {}", e);
            println!("🔧 DEBUG: Attempting recovery with smaller batch...");
            
            // Fallback: Try with significantly smaller batch size
            let recovery_batch_size = std::cmp::max(64, batch_size / 4);
            println!("🔍 DEBUG: Trying recovery with batch size {}", recovery_batch_size);
            
            let mut recovery_batch = LlamaBatch::new(recovery_batch_size, 1);
            for (i, &token) in tokens.iter().enumerate() {
                let logits = i == tokens.len() - 1;
                recovery_batch.add(token, i as i32, &[0], logits)?;
            }
            
            match context.decode(&mut recovery_batch) {
                Ok(()) => {
                    println!("✅ DEBUG: Recovery batch decoded successfully");
                    Ok(recovery_batch)
                },
                Err(e2) => {
                    println!("❌ DEBUG: Recovery batch also failed: {}", e2);
                    Err(anyhow::anyhow!("Failed to decode batch even with recovery: {}", e2))
                }
            }
        }
    }
}

// === TEXT GENERATION FUNCTIONS ===

// Generate AI response token by token with streaming output
fn generate_response(
    context: &mut llama_cpp_2::context::LlamaContext,
    model: &LlamaModel,
    sampler: &mut LlamaSampler,
    mut batch: LlamaBatch,
    tokens_len: usize
) -> Result<String> {
    println!("🔍 DEBUG: Starting generation...");
    print!("Assistant: ");
    io::stdout().flush()?;

    let mut response = String::new();                           // Accumulate generated text
    let mut token_count = 0;                                   // Track generation progress
    let generation_start = std::time::SystemTime::now();       // Measure performance

    let eos_id = model.token_eos();                             // End-of-sequence token ID
    let max_new_tokens: usize = 4096;                          // Maximum response length
    
    println!("🔍 DEBUG: EOS token ID is: {:?}", eos_id);
    
    // Try to convert EOS token to string to see what it looks like
    let eos_str = model.token_to_str(eos_id, Special::Tokenize)
        .unwrap_or_else(|_| "ERROR_CONVERTING_EOS".to_string());
    println!("🔍 DEBUG: EOS token as string: '{}'", eos_str);
    io::stdout().flush()?;

    // Token-by-token generation with streaming output and error handling
    for generation_step in 0..max_new_tokens {
        println!("🔍 DEBUG: Generation step {}", generation_step);
        io::stdout().flush()?;
        
        // Sample next token with panic protection (prevents crashes)
        let next_token = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sampler.sample(context, -1)  // -1 means use last token for prediction
        })) {
            Ok(token) => {
                println!("🔍 DEBUG: Sampled token: {:?}", token);
                io::stdout().flush()?;
                token
            },
            Err(_) => {
                println!("\n❌ DEBUG: Sampling panicked at step {}", generation_step);
                break;  // Stop generation on sampling failure
            }
        };

        // Check for natural end of response
        println!("🔍 DEBUG: Comparing token {:?} with EOS {:?}", next_token, eos_id);
        if next_token == eos_id {
            println!("\n🔍 DEBUG: EOS token encountered! (token: {:?} == eos: {:?})", next_token, eos_id);
            break;
        } else {
            println!("🔍 DEBUG: Not EOS, continuing...");
        }

        // Convert token back to text
        let token_str = model.token_to_str(next_token, Special::Tokenize)?;
        
        // Debug: Print each token as it's generated
        print!("PRINTING-TOKEN_STR: '{}'", token_str);
        io::stdout().flush()?;

        // Check for Qwen-3 specific chat template end marker
        if token_str.contains("<|im_end|>") {
            println!("\n🔍 DEBUG: Found <|im_end|> in token_str: '{}'", token_str);
            if let Some(i) = token_str.find("<|im_end|>") {
                let before = &token_str[..i];
                println!("🔍 DEBUG: Text before <|im_end|>: '{}'", before);
                print!("{}", before);
                io::stdout().flush()?;
                response.push_str(before);
            }
            println!("\n🔍 DEBUG: <|im_end|> encountered - stopping generation");
            break;  // Stop at chat template boundary
        }

        // Stream token to user in real-time
        print!("{}", token_str);
        io::stdout().flush()?;

        response.push_str(&token_str);
        token_count += 1;

        // Feed generated token back into context for next prediction
        batch.clear();
        batch.add(next_token, (tokens_len + token_count - 1) as i32, &[0], true)?;
        if let Err(e) = context.decode(&mut batch) { 
            println!("\n❌ DEBUG: Failed to decode token at step {}: {}", generation_step, e);
            break;  // Stop on decoding failure
        }
    }

    // Calculate and display performance metrics
    let generation_time = std::time::SystemTime::now().duration_since(generation_start).unwrap_or_default();
    let tps = if generation_time.as_secs_f64() > 0.0 {
        token_count as f64 / generation_time.as_secs_f64()  // Tokens per second
    } else { 0.0 };

    println!("\n🔍 DEBUG: Generated {} tokens in {:.2}s ({:.1} t/s)", token_count, generation_time.as_secs_f64(), tps);
    println!("🔍 DEBUG: Response length: {} chars", response.len());

    Ok(response)
}

// === RESPONSE PROCESSING FUNCTIONS ===

// Process AI response and execute any embedded commands
fn process_response_and_execute_commands(response: &str) -> serde_json::Value {
    let mut assistant_entry = json!({ "role": "assistant", "content": response.trim() });

    // Check if AI requested any command executions
    let commands = extract_commands_from_response(response);
    let mut executed_commands = Vec::new();
    
    if !commands.is_empty() {
        println!("🔍 DEBUG: Found {} command(s) to execute", commands.len());
        // Execute each command sequentially and track results
        for (i, command) in commands.iter().enumerate() {
            println!("🔍 DEBUG: Executing command {} of {}", i + 1, commands.len());
            let _out = execute_command(command);  // Run system command and get output
            executed_commands.push(command.clone());
            println!("🔍 DEBUG: Command {} completed", i + 1);
        }
        // Log executed commands in conversation history for context
        assistant_entry["commands_executed"] = json!(executed_commands);
        println!("🔍 DEBUG: All commands executed.");
    } else {
        println!("🔍 DEBUG: No commands found in response");
    }

    assistant_entry
}

// Update conversation history and save to persistent storage
fn update_conversation_state(
    history: &mut Vec<ChatTurn>,
    conversation_json: &mut Vec<serde_json::Value>,
    user_input: String,
    assistant_entry: serde_json::Value,
    conversation_file: &str,
    system_prompt: Option<&str>,
) -> Result<()> {
    // Add system prompt to conversation log if this is the first interaction
    if let Some(sys_prompt) = system_prompt {
        if conversation_json.is_empty() {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            
            conversation_json.push(json!({ 
                "role": "system", 
                "content": sys_prompt,
                "timestamp": timestamp,
                "note": "Auto-generated system prompt for this conversation"
            }));
        }
    }

    // Add user message to both in-memory history and persistent JSON log
    history.push(ChatTurn { role: "user".to_string(), content: user_input.clone() });
    conversation_json.push(json!({ "role": "user", "content": user_input }));

    // Add assistant response
    let assistant_content = assistant_entry["content"].as_str().unwrap_or("").to_string();
    history.push(ChatTurn { role: "assistant".to_string(), content: assistant_content });
    conversation_json.push(assistant_entry);

    // Save conversation to JSON file for persistence across sessions
    if let Err(e) = save_conversation(conversation_json, conversation_file) {
        println!("⚠️ DEBUG: Failed to save conversation: {}", e);
    }

    Ok(())
}

fn main() -> Result<()> {
    println!("🦙 Minimal LLaMA Chat Test - Direct API (Qwen chat template)");

    // === INITIALIZATION ===
    let model_path = r"/Users/agus/.lmstudio/models/unsloth/Qwen3-Coder-30B-A3B-Instruct-1M-GGUF/Qwen3-Coder-30B-A3B-Instruct-1M-UD-TQ1_0.gguf";
    
    // Initialize conversation tracking and model components
    let (conversation_file, mut conversation_json, mut history, mut turn_count) = initialize_conversation()?;
    let (backend, model, mut sampler, available_memory_gb) = initialize_model(model_path)?;

    // === MAIN CHAT LOOP ===
    loop {
        turn_count += 1;

        // Get user input
        match get_user_input(turn_count)? {
            Some(user_input) => {
                if user_input.is_empty() { continue; }  // Skip empty input

                // Create system prompt and build full prompt with history
                let system_prompt = create_system_prompt();
                let prompt = get_qwen_base_prompt(&system_prompt, &history);

                // Tokenize prompt
                println!("🔍 DEBUG: Tokenizing prompt ({} chars)...", prompt.len());
                io::stdout().flush()?;
                let tokens = model.str_to_token(&prompt, AddBos::Always)?;
                println!("🔍 DEBUG: Got {} tokens", tokens.len());

                // Create context with adaptive sizing and process initial tokens
                let (mut context, n_ctx) = create_context_with_retry(&model, &backend, available_memory_gb)?;
                let batch = match decode_initial_batch(&mut context, &tokens, n_ctx) {
                    Ok(batch) => batch,
                    Err(e) => {
                        println!("❌ DEBUG: Skipping turn due to batch decode failure: {}", e);
                        drop(context);
                        continue;
                    }
                };

                // Generate AI response
                let response = generate_response(&mut context, &model, &mut sampler, batch, tokens.len())?;

                // Process response and execute any commands
                let assistant_entry = process_response_and_execute_commands(&response);

                // Update conversation state and save
                update_conversation_state(
                    &mut history,
                    &mut conversation_json,
                    user_input,
                    assistant_entry,
                    &conversation_file,
                    Some(&system_prompt),
                )?;

                println!("🔍 DEBUG: Turn #{} completed successfully", turn_count);
                io::stdout().flush()?;

                // Clean up context memory
                println!("🔍 DEBUG: Dropping context for cleanup...");
                drop(context);
                println!("🔍 DEBUG: Context dropped, ready for next turn");
                io::stdout().flush()?;
            }
            None => break,  // Exit signal
        }
    }

    // === CHAT SESSION TERMINATION ===
    println!("🔍 DEBUG: Chat ended after {} turns", turn_count);
    Ok(())
}
