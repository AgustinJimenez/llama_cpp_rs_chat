use std::env;
use std::io::{self, Write};

use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, AddBos, LlamaModel},
    sampling::LlamaSampler,
    send_logs_to_tracing, LogOptions,
};

mod test_support;

use test_support::generation::generate_response;
use test_support::logger::ConversationLogger;

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
            eprintln!("Failed to initialize conversation logger: {e}");
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

    println!("Model loaded! Using context: {CONTEXT_SIZE} tokens");

    // Create sampler based on configuration
    // Available options: Greedy, Temperature, Mirostat, TopP, TopK,
    // Typical, MinP, TempExt, ChainTempTopP, ChainTempTopK, ChainFull
    let mut sampler = match SAMPLER_TYPE {
        SamplerType::Greedy => {
            println!("Using greedy sampler");
            LlamaSampler::greedy()
        }
        SamplerType::Temperature => {
            println!("Using temperature sampler (temp: {TEMPERATURE})");
            LlamaSampler::temp(TEMPERATURE)
        }
        SamplerType::Mirostat => {
            println!(
                "Using mirostat sampler (tau: {MIROSTAT_TAU}, eta: {MIROSTAT_ETA})"
            );
            LlamaSampler::mirostat_v2(0, MIROSTAT_TAU, MIROSTAT_ETA) // seed=0 for random
        }
        SamplerType::TopP => {
            println!(
                "Using top_p sampler (p: {TOP_P}) - NOTE: crashes with current model/setup"
            );
            // TODO: Fix top_p parameters - currently crashes with GGML_ASSERT
            LlamaSampler::greedy() // Fallback for now
        }
        SamplerType::TopK => {
            println!(
                "Using top_k sampler (k: {TOP_K}) - NOTE: crashes with current model/setup"
            );
            // TODO: Fix top_k parameters - currently crashes with GGML_ASSERT
            LlamaSampler::greedy() // Fallback for now
        }

        // Additional samplers - test these with different models
        SamplerType::Typical => {
            println!("Using typical sampler (p: {TYPICAL_P})");
            LlamaSampler::typical(TYPICAL_P, 1)
        }
        SamplerType::MinP => {
            println!("Using min_p sampler (p: {MIN_P})");
            LlamaSampler::min_p(MIN_P, 1)
        }
        SamplerType::TempExt => {
            println!(
                "Using extended temperature sampler (temp: {TEMPERATURE}, delta: 0.0, exp: 1.0)"
            );
            LlamaSampler::temp_ext(TEMPERATURE, 0.0, 1.0)
        }

        // Chain samplers - combining multiple techniques
        SamplerType::ChainTempTopP => {
            println!(
                "Using chained temperature + top_p sampler (temp: {TEMPERATURE}, p: {TOP_P})"
            );
            let samplers = vec![
                LlamaSampler::temp(TEMPERATURE),
                LlamaSampler::top_p(TOP_P, 1),
            ];
            LlamaSampler::chain_simple(samplers)
        }
        SamplerType::ChainTempTopK => {
            println!(
                "Using chained temperature + top_k sampler (temp: {TEMPERATURE}, k: {TOP_K})"
            );
            let samplers = vec![
                LlamaSampler::temp(TEMPERATURE),
                LlamaSampler::top_k(TOP_K as i32),
            ];
            LlamaSampler::chain_simple(samplers)
        }
        SamplerType::ChainFull => {
            println!(
                "Using full chain sampler (temp: {TEMPERATURE}, top_p: {TOP_P}, top_k: {TOP_K})"
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
            println!("You: {message}");
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
                    &get_system_prompt(),
                    SHOW_COMMAND_OUTPUT,
                    DEBUG_TEST,
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
                                    "Continue this conversation exactly where it left off:\n\n{conversation_so_far}"
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
                        println!("Error: {e}\n");
                        conversation_logger.log_message("ERROR", &e);
                        break;
                    }
                }
            }
        }

        // Save conversation before exiting
        if let Err(e) = conversation_logger.save() {
            eprintln!("Failed to save conversation: {e}");
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
                eprintln!("Failed to save conversation: {e}");
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
                &get_system_prompt(),
                SHOW_COMMAND_OUTPUT,
                DEBUG_TEST,
            ) {
                Ok(response) => {
                    full_ai_response.push_str(&response);

                    if response.contains("[/Output]") && response.contains("Based on this output:")
                    {
                        if let Some(continuation_start) = response.find("Based on this output:") {
                            let conversation_so_far = &response[..continuation_start + 21];
                            current_message = format!(
                                "Continue this conversation exactly where it left off:\n\n{conversation_so_far}"
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
                    println!("Error: {e}\n");
                    conversation_logger.log_message("ERROR", &e);
                    break;
                }
            }
        }
    }

    println!("Goodbye!");
}
