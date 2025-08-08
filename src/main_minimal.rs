use std::io::{self, Write};
use anyhow::Result;
use std::num::NonZeroU32;
use std::process::Command;
use std::fs;
use serde_json::json;

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
};

#[derive(Debug, Clone)]
struct ChatTurn {
    role: String,   // "system" | "user" | "assistant"
    content: String,
}

fn execute_command(command: &str) -> String {
    println!("\n🔧 EXECUTING COMMAND: {}", command);
    io::stdout().flush().unwrap();

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

fn extract_commands_from_response(response: &str) -> Vec<String> {
    let mut commands = Vec::new();

    // Look for commands in <|EXEC|>command<|/EXEC|> tags
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

fn save_conversation(conversation: &[serde_json::Value], conversation_file: &str) -> Result<()> {
    // Ensure the conversations directory exists
    fs::create_dir_all("assets/conversations")?;

    // Write to file
    let json_content = serde_json::to_string_pretty(conversation)?;
    fs::write(conversation_file, json_content)?;

    println!("🔍 DEBUG: Conversation saved to {}", conversation_file);
    Ok(())
}

fn build_qwen_prompt(system_prompt: &str, history: &[ChatTurn]) -> String {
    // Qwen-3 chat template using <|im_start|>/<|im_end|>
    // We'll render all previous turns + append assistant header for generation

    let mut s = String::new();
    s.push_str("<|im_start|>system\n");
    s.push_str(system_prompt);
    s.push_str("\n<|im_end|>\n");

    for turn in history {
        match turn.role.as_str() {
            "user" => {
                s.push_str("<|im_start|>user\n");
                s.push_str(&turn.content);
                s.push_str("\n<|im_end|>\n");
            }
            "assistant" => {
                s.push_str("<|im_start|>assistant\n");
                s.push_str(&turn.content);
                s.push_str("\n<|im_end|>\n");
            }
            _ => {}
        }
    }

    s.push_str("<|im_start|>assistant\n"); // model should continue from here
    s
}

fn main() -> Result<()> {
    println!("🦙 Minimal LLaMA Chat Test - Direct API (Qwen chat template)");

    // Generate conversation ID and initialize conversation tracking
    let conversation_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let conversation_file = format!("assets/conversations/chat_{}.json", conversation_id);
    let mut conversation_json: Vec<serde_json::Value> = Vec::new();
    let mut history: Vec<ChatTurn> = Vec::new();

    // Model path (edit this to your local path)
    let model_path = r"E:\\.lmstudio\\models\\lmstudio-community\\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL-GGUF\\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL.gguf";

    println!("🔍 DEBUG: Initializing backend...");
    io::stdout().flush()?;
    let backend = LlamaBackend::init()?;

    println!("🔍 DEBUG: Loading model...");
    io::stdout().flush()?;
    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, model_path, &model_params)?;

    println!("🔍 DEBUG: Creating sampler...");
    io::stdout().flush()?;
    // Note: We keep greedy sampler for compatibility. If your crate exposes temperature/top-p,
    // switch to that to reduce early <eos>.
    let mut sampler = LlamaSampler::greedy();

    println!("🔍 DEBUG: Model loaded successfully");
    io::stdout().flush()?;

    let mut turn_count = 0;

    loop {
        turn_count += 1;
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
                break;
            }
            Ok(bytes) => {
                println!("🔍 DEBUG: Read {} bytes", bytes);
                io::stdout().flush()?;
                input.trim().to_string()
            }
            Err(e) => {
                println!("❌ DEBUG: Error reading: {}", e);
                break;
            }
        };

        if user_input.eq_ignore_ascii_case("exit") { break; }
        if user_input.is_empty() { continue; }

        println!("🔍 DEBUG: Processing input: '{}'", user_input);
        io::stdout().flush()?;

        // Add user message to history & JSON log
        history.push(ChatTurn { role: "user".to_string(), content: user_input.clone() });
        conversation_json.push(json!({ "role": "user", "content": user_input }));

        // Build system preamble and Qwen-formatted prompt with full history
        let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let os_name = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        let system_prompt = format!(
            "You are an intelligent AI assistant with command-line access. You can execute system commands when needed.\n\n\
            SYSTEM INFORMATION:\n\
            - Operating System: {} ({})\n\
            - Current Directory: {}\n\
            - Architecture: {}\n\n\
            COMMAND EXECUTION:\n\
            You can execute commands by wrapping them in <|EXEC|>command<|/EXEC|> tags.\n\
            Examples:\n\
            - <|EXEC|>dir<|/EXEC|> (Windows) or <|EXEC|>ls -la<|/EXEC|> (Unix/Linux)\n\
            - <|EXEC|>mkdir test_folder<|/EXEC|>\n\
            - <|EXEC|>echo 'Hello World' > test.txt<|/EXEC|>\n\
            - <|EXEC|>type test.txt<|/EXEC|> (Windows) or <|EXEC|>cat test.txt<|/EXEC|> (Unix/Linux)\n\n\
            GUIDELINES:\n\
            - Only use commands when they help accomplish the user's request\n\
            - Explain briefly what you're doing before running commands\n\
            - Use appropriate commands for the current OS ({})\n\
            - Be careful with destructive operations",
            os_name,
            if os_name == "windows" { "Windows" } else { "Unix/Linux" },
            current_dir.display(),
            arch,
            os_name
        );

        let prompt = build_qwen_prompt(&system_prompt, &history);

        println!("🔍 DEBUG: Creating context...");
        io::stdout().flush()?;
        let n_ctx_nonzero = NonZeroU32::new(32000);
        let ctx_params = LlamaContextParams::default().with_n_ctx(n_ctx_nonzero);
        let mut context = model.new_context(&backend, ctx_params)?;

        println!("🔍 DEBUG: Tokenizing prompt ({} chars)...", prompt.len());
        io::stdout().flush()?;
        let tokens = model.str_to_token(&prompt, AddBos::Always)?; // critical for some instruct models
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
            let logits = i == tokens.len() - 1; // only last needs logits for generation
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

        let eos_id = model.token_eos();
        let max_new_tokens: usize = 4096; // adjust as needed

        for generation_step in 0..max_new_tokens {
            let next_token = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sampler.sample(&mut context, -1)
            })) {
                Ok(token) => token,
                Err(_) => {
                    println!("\n❌ DEBUG: Sampling panicked at step {}", generation_step);
                    break;
                }
            };

            if next_token == eos_id {
                println!("\n🔍 DEBUG: EOS token encountered");
                break;
            }

            let token_str = model.token_to_str(next_token, Special::Tokenize)?;

            // Stop if model outputs the Qwen chat stop marker
            if token_str.contains("<|im_end|>") {
                if let Some(i) = token_str.find("<|im_end|>") {
                    let before = &token_str[..i];
                    print!("{}", before);
                    io::stdout().flush()?;
                    response.push_str(before);
                }
                println!("\n🔍 DEBUG: <|im_end|> encountered");
                break;
            }

            print!("{}", token_str);
            io::stdout().flush()?;

            response.push_str(&token_str);
            token_count += 1;

            // Feed token
            batch.clear();
            batch.add(next_token, (tokens.len() + token_count - 1) as i32, &[0], true)?;
            if let Err(e) = context.decode(&mut batch) { 
                println!("\n❌ DEBUG: Failed to decode token at step {}: {}", generation_step, e);
                break;
            }
        }

        let generation_time = std::time::SystemTime::now().duration_since(generation_start).unwrap_or_default();
        let tps = if generation_time.as_secs_f64() > 0.0 {
            token_count as f64 / generation_time.as_secs_f64()
        } else { 0.0 };

        println!("\n🔍 DEBUG: Generated {} tokens in {:.2}s ({:.1} t/s)", token_count, generation_time.as_secs_f64(), tps);
        println!("🔍 DEBUG: Response length: {} chars", response.len());

        // Log assistant response (trim trailing whitespace)
        let mut assistant_entry = json!({ "role": "assistant", "content": response.trim() });

        // Execute commands if present
        let commands = extract_commands_from_response(&response);
        let mut executed_commands = Vec::new();
        if !commands.is_empty() {
            println!("🔍 DEBUG: Found {} command(s) to execute", commands.len());
            for (i, command) in commands.iter().enumerate() {
                println!("🔍 DEBUG: Executing command {} of {}", i + 1, commands.len());
                let _out = execute_command(command);
                executed_commands.push(command.clone());
                println!("🔍 DEBUG: Command {} completed", i + 1);
            }
            assistant_entry["commands_executed"] = json!(executed_commands);
            println!("🔍 DEBUG: All commands executed.");
        } else {
            println!("🔍 DEBUG: No commands found in response");
        }

        // Append to history & JSON log
        history.push(ChatTurn { role: "assistant".to_string(), content: response.trim().to_string() });
        conversation_json.push(assistant_entry);

        // Persist conversation
        if let Err(e) = save_conversation(&conversation_json, &conversation_file) {
            println!("⚠️ DEBUG: Failed to save conversation: {}", e);
        }

        println!("🔍 DEBUG: Turn #{} completed successfully", turn_count);
        io::stdout().flush()?;

        // Cleanup context
        println!("🔍 DEBUG: Dropping context for cleanup...");
        drop(context);
        println!("🔍 DEBUG: Context dropped, ready for next turn");
        io::stdout().flush()?;
    }

    println!("🔍 DEBUG: Chat ended after {} turns", turn_count);
    Ok(())
}
