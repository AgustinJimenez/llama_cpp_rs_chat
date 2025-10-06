use std::env;
use std::fs;
use std::io::{self, Write};
use std::num::NonZeroU32;
use std::process::Command;

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
    send_logs_to_tracing, LogOptions,
};

// Enum for sampler types
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Variants are for future use with different models
enum SamplerType {
    Greedy,
    Temperature,
    Mirostat,
    TopP,
    TopK,
    Typical,
    MinP,
    TempExt,
    ChainTempTopP,
    ChainTempTopK,
    ChainFull,
}

/*
// Test 1: IBM Recommended (current)
  const SAMPLER_TYPE: &str = "mirostat";
  const MIROSTAT_TAU: f32 = 5.0;
  const MIROSTAT_ETA: f32 = 0.1;

  // Test 2: Conservative Mirostat
  const SAMPLER_TYPE: &str = "mirostat";
  const MIROSTAT_TAU: f32 = 3.0;
  const MIROSTAT_ETA: f32 = 0.1;

  // Test 3: Balanced Temperature
  const SAMPLER_TYPE: &str = "temperature";
  const TEMPERATURE: f32 = 0.7;

  // Test 4: Focused Temperature
  const SAMPLER_TYPE: &str = "temperature";
  const TEMPERATURE: f32 = 0.3;
*/
const DEBUG_TEST: bool = true;
const MODEL_PATH: &str = "/Users/agus/.lmstudio/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf";
const CONTEXT_SIZE: u32 = 32768; // Increased for Granite's 128K capacity
const LLAMACPP_DEBUG: bool = false;
const SHOW_COMMAND_OUTPUT: bool = true;

// Sampler configuration - optimal settings for Granite-4.0-H-Tiny based on IBM recommendations
const SAMPLER_TYPE: SamplerType = SamplerType::Greedy; // IBM-recommended would be ChainFull but it crashes with this model
const TEMPERATURE: f32 = 0.7; // IBM recommended for Granite models (was 0.8)
const TOP_P: f32 = 0.95; // IBM recommended for Granite models
const TOP_K: u32 = 20; // IBM recommended for Granite models
const MIROSTAT_TAU: f32 = 5.0; // Target entropy (default for Granite: 5.0)
const MIROSTAT_ETA: f32 = 0.1; // Learning rate (default for Granite: 0.1)

// Additional sampling parameters for future models
const TYPICAL_P: f32 = 1.0; // Typical sampling parameter
const MIN_P: f32 = 0.0; // Minimum probability threshold

// Test messages for DEBUG_TEST mode
const TEST_MESSAGES: &[&str] = &[
    "hello",
    "check the file 00_alejandro and answer the questions there, just one word answer for each question",
];

// Conversation logging
struct ConversationLogger {
    file_path: String,
    content: String,
}

impl ConversationLogger {
    fn new() -> io::Result<Self> {
        // Create assets/conversations directory if it doesn't exist
        let conversations_dir = "assets/conversations";
        fs::create_dir_all(conversations_dir)?;

        // Generate timestamp-based filename with YYYY-MM-DD-HH-mm-ss-SSS format
        let now = std::time::SystemTime::now();
        let since_epoch = now
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Convert to a more readable format
        let secs = since_epoch.as_secs();
        let millis = since_epoch.subsec_millis();

        // Simple conversion (this won't be perfect timezone-wise, but it's readable)
        let days_since_epoch = secs / 86400;
        let remaining_secs = secs % 86400;
        let hours = remaining_secs / 3600;
        let remaining_secs = remaining_secs % 3600;
        let minutes = remaining_secs / 60;
        let seconds = remaining_secs % 60;

        // Approximate date calculation (starting from 1970-01-01)
        let year = 1970 + (days_since_epoch / 365);
        let day_of_year = days_since_epoch % 365;
        let month = std::cmp::min(12, (day_of_year / 30) + 1);
        let day = (day_of_year % 30) + 1;

        let timestamp = format!(
            "{:04}-{:02}-{:02}-{:02}-{:02}-{:02}-{:03}",
            year, month, day, hours, minutes, seconds, millis
        );

        let file_path = format!("{}/chat_{}.txt", conversations_dir, timestamp);

        Ok(ConversationLogger {
            file_path,
            content: String::new(),
        })
    }

    fn log_message(&mut self, role: &str, message: &str) {
        let log_entry = format!("{}:\n{}\n\n", role, message);
        self.content.push_str(&log_entry);

        // Write immediately to file
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {}", e);
        }
    }

    fn log_command_execution(&mut self, command: &str, output: &str) {
        let log_entry = format!("[COMMAND: {}]\n{}\n\n", command, output);
        self.content.push_str(&log_entry);

        // Write immediately to file
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {}", e);
        }
    }

    fn get_full_conversation(&self) -> String {
        // Return the complete conversation content from memory
        self.content.clone()
    }

    fn load_conversation_from_file(&self) -> io::Result<String> {
        // Read the conversation directly from file (source of truth)
        fs::read_to_string(&self.file_path)
    }

    fn save(&self) -> io::Result<()> {
        // Final save (content should already be written, but ensure it's there)
        fs::write(&self.file_path, &self.content)?;
        println!("Conversation saved to: {}", self.file_path);
        Ok(())
    }
}
fn get_system_prompt() -> String {
    let os_info = if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS (Darwin)"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Unix-like system"
    };

    let current_dir = env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let current_date = std::process::Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    format!(
        "
You are a local cli AI tool with shell access on a computer, your goal is to understand what the user wants and help with tasks.
The current system is running on {}
From that, you must automatically know what commands are available and how to format them

Rules of operation
- Don't ask the user to do tasks you can do
- You can freely manipulate files or folders for normal work.
- Try at least 10 times to do the tasks with a different approach before requesting more information to the user if you are stuck 
- Confirm only for risky changes (for example, deleting or overwriting many files, running privileged commands, installing software, or altering system paths).
- Before working with a file, verify that it exists first
- When looking for files: if not found in current directory, immediately use: find . -name \"*filename*\" -type f
- For file searches: use wildcards to match partial names across the entire project (e.g., find . -name \"*alejandro*\" -type f)
- IMPORTANT: Always put wildcards in quotes when using find command (e.g., \"*.gguf\" not *.gguf)
- NEVER search the entire filesystem with find / - use specific directories like . or ~/
- After finding file location, navigate and read the file from its actual path
- Always check subdirectories that seem relevant to the file you're looking for
- Always be thorough - execute search commands, don't just describe them
- Summarize the output briefly after execution and what you think about it.
- If a command fails, show the error and try a different approach - don't repeat the same failing command
- For web access, use curl, wget, or PowerShell's Invoke-WebRequest, with short timeouts and limited output.
- Keep responses concise, technical, and neutral.
- Try to run commands without moving from the current directory, don't use the 'cd' command
- Don't repeat the same commands over and over again

To run a command, use this exact format:
<COMMAND>command_here</COMMAND>

Current directory: {}
Current date: {}
",
        os_info, current_dir, current_date
    )
}

fn main() {
    println!("\n<======================================================================>\n");

    // Initialize conversation logger
    let mut conversation_logger = match ConversationLogger::new() {
        Ok(logger) => logger,
        Err(e) => {
            eprintln!("Failed to initialize conversation logger: {}", e);
            return;
        }
    };

    // Control llama.cpp log output
    if !LLAMACPP_DEBUG {
        // Redirect llama.cpp logs to tracing system (which we don't initialize, so they're discarded)
        send_logs_to_tracing(LogOptions::default());
    }

    println!("Loading model...");

    // Initialize backend
    let backend = LlamaBackend::init().expect("Failed to init backend");

    // Load model
    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, MODEL_PATH, &model_params)
        .expect("Failed to load model");

    println!("Model loaded! Using context: {} tokens", CONTEXT_SIZE);

    // Create sampler based on configuration
    // Available options: Greedy, Temperature, Mirostat, TopP, TopK,
    // Typical, MinP, TempExt, ChainTempTopP, ChainTempTopK, ChainFull
    let mut sampler = match SAMPLER_TYPE {
        SamplerType::Greedy => {
            println!("Using greedy sampler");
            LlamaSampler::greedy()
        }
        SamplerType::Temperature => {
            println!("Using temperature sampler (temp: {})", TEMPERATURE);
            LlamaSampler::temp(TEMPERATURE)
        }
        SamplerType::Mirostat => {
            println!(
                "Using mirostat sampler (tau: {}, eta: {})",
                MIROSTAT_TAU, MIROSTAT_ETA
            );
            LlamaSampler::mirostat_v2(0, MIROSTAT_TAU, MIROSTAT_ETA) // seed=0 for random
        }
        SamplerType::TopP => {
            println!(
                "Using top_p sampler (p: {}) - NOTE: crashes with current model/setup",
                TOP_P
            );
            // TODO: Fix top_p parameters - currently crashes with GGML_ASSERT
            LlamaSampler::greedy() // Fallback for now
        }
        SamplerType::TopK => {
            println!(
                "Using top_k sampler (k: {}) - NOTE: crashes with current model/setup",
                TOP_K
            );
            // TODO: Fix top_k parameters - currently crashes with GGML_ASSERT
            LlamaSampler::greedy() // Fallback for now
        }

        // Additional samplers - test these with different models
        SamplerType::Typical => {
            println!("Using typical sampler (p: {})", TYPICAL_P);
            LlamaSampler::typical(TYPICAL_P, 1)
        }
        SamplerType::MinP => {
            println!("Using min_p sampler (p: {})", MIN_P);
            LlamaSampler::min_p(MIN_P, 1)
        }
        SamplerType::TempExt => {
            println!(
                "Using extended temperature sampler (temp: {}, delta: 0.0, exp: 1.0)",
                TEMPERATURE
            );
            LlamaSampler::temp_ext(TEMPERATURE, 0.0, 1.0)
        }

        // Chain samplers - combining multiple techniques
        SamplerType::ChainTempTopP => {
            println!(
                "Using chained temperature + top_p sampler (temp: {}, p: {})",
                TEMPERATURE, TOP_P
            );
            let samplers = vec![
                LlamaSampler::temp(TEMPERATURE),
                LlamaSampler::top_p(TOP_P, 1),
            ];
            LlamaSampler::chain_simple(samplers)
        }
        SamplerType::ChainTempTopK => {
            println!(
                "Using chained temperature + top_k sampler (temp: {}, k: {})",
                TEMPERATURE, TOP_K
            );
            let samplers = vec![
                LlamaSampler::temp(TEMPERATURE),
                LlamaSampler::top_k(TOP_K as i32),
            ];
            LlamaSampler::chain_simple(samplers)
        }
        SamplerType::ChainFull => {
            println!(
                "Using full chain sampler (temp: {}, top_p: {}, top_k: {})",
                TEMPERATURE, TOP_P, TOP_K
            );
            let samplers = vec![
                LlamaSampler::temp(TEMPERATURE),
                LlamaSampler::top_p(TOP_P, 1),
                LlamaSampler::top_k(TOP_K as i32),
            ];
            LlamaSampler::chain_simple(samplers)
        }
    };

    println!("Ready to chat! Type 'exit' to quit.\n");

    // Debug test mode
    if DEBUG_TEST {
        println!("DEBUG: Testing with {} messages", TEST_MESSAGES.len());

        // Log system prompt at the beginning
        conversation_logger.log_message("SYSTEM", &get_system_prompt());

        for (i, message) in TEST_MESSAGES.iter().enumerate() {
            println!("\n--- Test {}/{} ---", i + 1, TEST_MESSAGES.len());
            println!("You: {}", message);
            print!("\nAI: ");
            io::stdout().flush().unwrap();

            // Log user message
            conversation_logger.log_message("USER", message);

            let mut full_ai_response = String::new();

            let mut current_message = message.to_string();

            loop {
                match generate_response(
                    &backend,
                    &model,
                    &mut sampler,
                    &current_message,
                    CONTEXT_SIZE,
                    &mut conversation_logger,
                ) {
                    Ok(response) => {
                        full_ai_response.push_str(&response);

                        if response.contains("[/Output]")
                            && response.contains("Based on this output:")
                        {
                            if let Some(continuation_start) = response.find("Based on this output:")
                            {
                                let conversation_so_far = &response[..continuation_start + 21];
                                current_message = format!(
                                    "Continue this conversation exactly where it left off:\n\n{}",
                                    conversation_so_far
                                );
                                continue;
                            }
                        }

                        // Log complete AI response
                        conversation_logger.log_message("ASSISTANT", &full_ai_response);
                        println!("\n");
                        break;
                    }
                    Err(e) => {
                        println!("Error: {}\n", e);
                        conversation_logger.log_message("ERROR", &e);
                        break;
                    }
                }
            }
        }

        // Save conversation before exiting
        if let Err(e) = conversation_logger.save() {
            eprintln!("Failed to save conversation: {}", e);
        }

        println!("DEBUG: Test completed, exiting...");
        return;
    }

    // Log system prompt at the beginning of interactive chat
    conversation_logger.log_message("SYSTEM", &get_system_prompt());

    // Chat loop
    loop {
        // Get user input
        print!("\nYou: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }

        let message = input.trim();
        if message == "exit" {
            // Save conversation before exiting
            if let Err(e) = conversation_logger.save() {
                eprintln!("Failed to save conversation: {}", e);
            }
            break;
        }

        if message.is_empty() {
            continue;
        }

        // Log user message
        conversation_logger.log_message("USER", message);

        // Generate response with live streaming
        print!("\nAI: ");
        io::stdout().flush().unwrap();

        let mut full_ai_response = String::new();

        let mut current_message = message.to_string();

        loop {
            match generate_response(
                &backend,
                &model,
                &mut sampler,
                &current_message,
                CONTEXT_SIZE,
                &mut conversation_logger,
            ) {
                Ok(response) => {
                    full_ai_response.push_str(&response);

                    if response.contains("[/Output]") && response.contains("Based on this output:")
                    {
                        if let Some(continuation_start) = response.find("Based on this output:") {
                            let conversation_so_far = &response[..continuation_start + 21];
                            current_message = format!(
                                "Continue this conversation exactly where it left off:\n\n{}",
                                conversation_so_far
                            );
                            continue;
                        }
                    }
                    // Normal response completion - log the full AI response
                    conversation_logger.log_message("ASSISTANT", &full_ai_response);
                    println!("\n");
                    break;
                }
                Err(e) => {
                    println!("Error: {}\n", e);
                    conversation_logger.log_message("ERROR", &e);
                    break;
                }
            }
        }
    }

    println!("Goodbye!");
}

fn detect_and_execute_command(
    text: &str,
    conversation_logger: &mut ConversationLogger,
) -> (String, bool) {
    // Check for both old and new function calling formats

    // New OpenAI-style function calling format
    if let Some(start) = text.find("<function_calls>") {
        if let Some(end) = text.find("</function_calls>") {
            if end > start {
                // Extract the function call block
                let function_block = &text[start..end + 16]; // 16 is length of "</function_calls>"

                // Look for execute_command function
                if function_block.contains("execute_command") {
                    if let Some(param_start) = function_block.find("<parameter name=\"command\">") {
                        if let Some(param_end) = function_block.find("</parameter>") {
                            if param_end > param_start {
                                let command_text = &function_block[param_start + 26..param_end]; // 26 is length of "<parameter name=\"command\">"
                                let before_command = &text[..start];
                                let _after_command = &text[end + 16..];

                                // Execute the command
                                let output = execute_command(command_text);

                                // Log command execution to conversation file
                                conversation_logger.log_command_execution(command_text, &output);

                                if SHOW_COMMAND_OUTPUT {
                                    println!("\n[Executing function: execute_command]");
                                    println!("[Command: {}]", command_text);
                                    println!("[Output:]");
                                    println!("{}", output);
                                    println!("[End of output]\n");
                                }

                                // Return the text with function call replaced by result and continuation marker
                                let new_text = format!(
                                    "{}[Function executed: execute_command({})]\n[Output:]\n{}\n[/Output]\n\nBased on this output: ",
                                    before_command, command_text, output
                                );
                                return (new_text, true);
                            }
                        }
                    }
                }
            }
        }
    }

    // Legacy <COMMAND> format for backward compatibility
    if let Some(start) = text.find("<COMMAND>") {
        if let Some(end) = text.find("</COMMAND>") {
            if end > start {
                let command_text = &text[start + 9..end]; // 9 is length of "<COMMAND>"
                let before_command = &text[..start];
                let _after_command = &text[end + 10..]; // 10 is length of "</COMMAND>"

                // Execute the command
                let output = execute_command(command_text);

                // Log command execution to conversation file
                conversation_logger.log_command_execution(command_text, &output);

                if SHOW_COMMAND_OUTPUT {
                    println!("\n[Executing command: {}]", command_text);
                    println!("[Command output:]");
                    println!("{}", output);
                    println!("[End of command output]\n");
                }

                // Return the text with command replaced by output and continuation marker
                let new_text = format!(
                    "{}[Command executed: {}]\n[Output:]\n{}\n[/Output]\n\nBased on this output: ",
                    before_command, command_text, output
                );
                return (new_text, true);
            }
        }
    }

    (text.to_string(), false)
}

fn parse_command_with_quotes(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current_part = String::new();
    let mut in_quotes = false;
    let mut chars = cmd.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                // Don't include the quote character in the output
            }
            ' ' if !in_quotes => {
                if !current_part.is_empty() {
                    parts.push(current_part.clone());
                    current_part.clear();
                }
            }
            _ => {
                current_part.push(ch);
            }
        }
    }

    if !current_part.is_empty() {
        parts.push(current_part);
    }

    parts
}

fn execute_command(cmd: &str) -> String {
    // Parse command with proper quote handling
    let parts = parse_command_with_quotes(cmd.trim());
    if parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    let command_name = &parts[0];

    // Basic command validation - reject obviously invalid commands
    if command_name.len() < 2 || command_name.contains("/") && !command_name.starts_with("/") {
        return format!("Error: Invalid command format: {}", command_name);
    }

    // Let the system handle command validation naturally - don't hardcode specific words

    // Prevent dangerous filesystem-wide searches
    if command_name == "find" && parts.len() > 1 {
        let search_path = &parts[1];
        if search_path == "/" || search_path == "/usr" || search_path == "/System" {
            return format!("Error: Filesystem-wide searches are not allowed for performance and security reasons. Try searching in specific directories like /Users/$USER, ~/.local, or current directory '.'");
        }
    }

    // Special handling for cd command - actually change the process working directory
    if command_name == "cd" {
        if DEBUG_TEST {
            eprintln!("DEBUG: Executing cd command: {:?}", cmd);
            eprintln!("DEBUG: Command parts: {:?}", parts);
        }

        let target_dir = if parts.len() > 1 {
            &parts[1]
        } else {
            // Default to home directory if no argument
            return "Error: cd command requires a directory argument".to_string();
        };

        match env::set_current_dir(target_dir) {
            Ok(_) => {
                if let Ok(new_dir) = env::current_dir() {
                    format!("Successfully changed directory to: {}", new_dir.display())
                } else {
                    "Directory changed successfully".to_string()
                }
            }
            Err(e) => {
                format!("Error: Failed to change directory: {}", e)
            }
        }
    } else {
        // Normal command execution for non-cd commands
        let mut command = Command::new(&parts[0]);
        if parts.len() > 1 {
            command.args(&parts[1..]);
        }

        // Debug: print what command is being executed
        if DEBUG_TEST {
            eprintln!("DEBUG: Executing command: {:?}", cmd);
            eprintln!("DEBUG: Command parts: {:?}", parts);
        }

        match command.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Debug: print raw output
                /*  if DEBUG_TEST {
                               eprintln!("DEBUG: stdout length: {}", stdout.len());
                               eprintln!("DEBUG: stderr length: {}", stderr.len());
                               eprintln!("DEBUG: stdout: '{}'", stdout);
                               eprintln!("DEBUG: stderr: '{}'", stderr);
                               eprintln!("DEBUG: exit status: {}", output.status);
                           }
                */

                // Handle commands that succeed silently
                if output.status.success() && stdout.is_empty() && stderr.is_empty() {
                    match command_name.as_str() {
                        "find" => {
                            // Find command with no output means no files found
                            "No files found matching the search criteria".to_string()
                        }
                        "mkdir" => "Directory created successfully".to_string(),
                        "touch" => "File created successfully".to_string(),
                        "rm" | "rmdir" => "File/directory removed successfully".to_string(),
                        "mv" | "cp" => "File operation completed successfully".to_string(),
                        "chmod" => "Permissions changed successfully".to_string(),
                        _ => {
                            // Generic success message for other silent commands
                            if parts.len() > 1 {
                                format!("Command '{}' executed successfully", parts.join(" "))
                            } else {
                                format!("Command '{}' executed successfully", command_name)
                            }
                        }
                    }
                } else if !stderr.is_empty() {
                    format!("{}\nError: {}", stdout, stderr)
                } else {
                    stdout.to_string()
                }
            }
            Err(e) => {
                if DEBUG_TEST {
                    eprintln!("DEBUG: Command execution failed: {}", e);
                }
                format!("Failed to execute command: {}", e)
            }
        }
    }
}

fn convert_conversation_to_chat_format(conversation: &str) -> String {
    let mut chat_format = String::new();
    let mut current_role = "";
    let mut current_content = String::new();

    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            // Save previous role's content
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                let role_tag = match current_role {
                    "SYSTEM" => "system",
                    "USER" => "user",
                    "ASSISTANT" => "assistant",
                    _ => "user",
                };
                chat_format.push_str(&format!(
                    "<|start_of_role|>{}<|end_of_role|>{}<|end_of_text|>",
                    role_tag,
                    current_content.trim()
                ));
            }

            // Start new role
            current_role = line.trim_end_matches(":");
            current_content.clear();
        } else if !line.starts_with("[COMMAND:") {
            // Skip command execution logs in this conversion, add content
            if !line.trim().is_empty() {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }
    }

    // Add the final role content
    if !current_role.is_empty() && !current_content.trim().is_empty() {
        let role_tag = match current_role {
            "SYSTEM" => "system",
            "USER" => "user",
            "ASSISTANT" => "assistant",
            _ => "user",
        };
        chat_format.push_str(&format!(
            "<|start_of_role|>{}<|end_of_role|>{}<|end_of_text|>",
            role_tag,
            current_content.trim()
        ));
    }

    // Add assistant start for response generation
    chat_format.push_str("<|start_of_role|>assistant<|end_of_role|>");

    chat_format
}

fn generate_response(
    backend: &LlamaBackend,
    model: &LlamaModel,
    sampler: &mut LlamaSampler,
    user_message: &str,
    context_size: u32,
    conversation_logger: &mut ConversationLogger,
) -> Result<String, String> {
    // Use proper Granite chat format with dynamic OS detection
    let system_prompt = get_system_prompt();
    let prompt = format!(
        "<|start_of_role|>system<|end_of_role|>{}<|end_of_text|><|start_of_role|>user<|end_of_role|>{}<|end_of_text|><|start_of_role|>assistant<|end_of_role|>",
        system_prompt, user_message
    );

    // Tokenize
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| format!("Tokenization failed: {}", e))?;

    // Create context with safe size
    let n_ctx = NonZeroU32::new(context_size).unwrap();
    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
    let mut context = model
        .new_context(backend, ctx_params)
        .map_err(|e| format!("Context creation failed: {}", e))?;

    // Prepare batch with larger size to handle big contexts
    let batch_size = std::cmp::min(tokens.len() + 1000, 4096);
    let mut batch = LlamaBatch::new(batch_size, 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(token, i as i32, &[0], is_last)
            .map_err(|e| format!("Batch add failed: {}", e))?;
    }

    // Process initial tokens
    context
        .decode(&mut batch)
        .map_err(|e| format!("Initial decode failed: {}", e))?;

    // Generate response - let model decide when to stop
    let mut response = String::new();
    let mut token_pos = tokens.len() as i32;

    loop {
        // Sample next token
        let next_token = sampler.sample(&context, -1);

        // Check for end-of-sequence token
        if next_token == model.token_eos() {
            break;
        }

        // Convert to string
        let token_str = model
            .token_to_str(next_token, Special::Tokenize)
            .map_err(|e| format!("Token conversion failed: {}", e))?;

        // Check for proper stop tokens - more comprehensive
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

        // Check if we have a complete command to execute
        if response.contains("<COMMAND>") && response.contains("</COMMAND>") {
            let (processed_response, command_executed) =
                detect_and_execute_command(&response, conversation_logger);

            if command_executed {
                // Print the token that completed the command
                print!("{}", token_str);
                io::stdout().flush().unwrap();

                // Stop current generation and return the processed response
                // This will end this generation cycle and the processed response
                // (with command output) will be used for the next generation
                return Ok(processed_response);
            }
        }

        // Print token immediately for real-time streaming
        print!("{}", token_str);
        io::stdout().flush().unwrap();

        // Only safety measure to prevent infinite loops - much higher limit
        if response.len() > 10000 {
            break;
        }

        // Prepare next iteration - crucial fix here
        batch.clear();
        batch
            .add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Batch add failed: {}", e))?;

        context
            .decode(&mut batch)
            .map_err(|e| format!("Decode failed: {}", e))?;

        token_pos += 1;
    }

    Ok(response.trim().to_string())
}
