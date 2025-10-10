// This module contains the LLaMA chat logic extracted from test.rs
// It will be refactored to work with the Tauri application

use std::env;
use std::num::NonZeroU32;


use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
};

// Enum for sampler types
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Variants are for future use with different models
pub enum SamplerType {
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

impl SamplerType {
    pub fn from_string(s: &str) -> Self {
        match s {
            "Greedy" => SamplerType::Greedy,
            "Temperature" => SamplerType::Temperature,
            "Mirostat" => SamplerType::Mirostat,
            "TopP" => SamplerType::TopP,
            "TopK" => SamplerType::TopK,
            "Typical" => SamplerType::Typical,
            "MinP" => SamplerType::MinP,
            "TempExt" => SamplerType::TempExt,
            "ChainTempTopP" => SamplerType::ChainTempTopP,
            "ChainTempTopK" => SamplerType::ChainTempTopK,
            "ChainFull" => SamplerType::ChainFull,
            _ => SamplerType::Greedy, // Default fallback
        }
    }
}

// Configuration constants with environment variable support
pub fn get_model_path() -> String {
    env::var("MODEL_PATH").unwrap_or_else(|_| 
        "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf".to_string()
    )
}

pub fn get_context_size() -> u32 {
    env::var("LLAMA_CONTEXT_SIZE")
        .unwrap_or_else(|_| "32768".to_string())
        .parse()
        .unwrap_or(32768)
}

pub struct ChatConfig {
    pub sampler_type: SamplerType,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub mirostat_tau: f32,
    pub mirostat_eta: f32,
    pub typical_p: f32,
    pub min_p: f32,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            sampler_type: SamplerType::Greedy,
            temperature: 0.7,
            top_p: 0.95,
            top_k: 20,
            mirostat_tau: 5.0,
            mirostat_eta: 0.1,
            typical_p: 1.0,
            min_p: 0.0,
        }
    }
}


pub struct ChatEngine {
    backend: LlamaBackend,
    model: LlamaModel,
    config: ChatConfig,
}


impl ChatEngine {
    pub fn new(config: ChatConfig) -> Result<Self, String> {
        // Initialize backend
        let backend = LlamaBackend::init().map_err(|e| format!("Failed to init backend: {}", e))?;

        // Load model
        let model_path = get_model_path();
        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
            .map_err(|e| format!("Failed to load model from {}: {}", model_path, e))?;

        Ok(Self {
            backend,
            model,
            config,
        })
    }

    pub fn create_sampler(&self) -> LlamaSampler {
        match self.config.sampler_type {
            SamplerType::Greedy => {
                println!("Using greedy sampler");
                LlamaSampler::greedy()
            }
            SamplerType::Temperature => {
                println!("Using temperature sampler (temp: {})", self.config.temperature);
                LlamaSampler::temp(self.config.temperature)
            }
            SamplerType::Mirostat => {
                println!(
                    "Using mirostat sampler (tau: {}, eta: {})",
                    self.config.mirostat_tau, self.config.mirostat_eta
                );
                LlamaSampler::mirostat_v2(0, self.config.mirostat_tau, self.config.mirostat_eta)
            }
            SamplerType::TopP => {
                println!("Using top_p sampler (p: {}) - NOTE: may crash with current model", self.config.top_p);
                LlamaSampler::greedy() // Fallback for now
            }
            SamplerType::TopK => {
                println!("Using top_k sampler (k: {}) - NOTE: may crash with current model", self.config.top_k);
                LlamaSampler::greedy() // Fallback for now
            }
            SamplerType::Typical => {
                println!("Using typical sampler (p: {})", self.config.typical_p);
                LlamaSampler::typical(self.config.typical_p, 1)
            }
            SamplerType::MinP => {
                println!("Using min_p sampler (p: {})", self.config.min_p);
                LlamaSampler::min_p(self.config.min_p, 1)
            }
            SamplerType::TempExt => {
                println!(
                    "Using extended temperature sampler (temp: {}, delta: 0.0, exp: 1.0)",
                    self.config.temperature
                );
                LlamaSampler::temp_ext(self.config.temperature, 0.0, 1.0)
            }
            SamplerType::ChainTempTopP => {
                println!(
                    "Using chained temperature + top_p sampler (temp: {}, p: {})",
                    self.config.temperature, self.config.top_p
                );
                let samplers = vec![
                    LlamaSampler::temp(self.config.temperature),
                    LlamaSampler::top_p(self.config.top_p, 1),
                ];
                LlamaSampler::chain_simple(samplers)
            }
            SamplerType::ChainTempTopK => {
                println!(
                    "Using chained temperature + top_k sampler (temp: {}, k: {})",
                    self.config.temperature, self.config.top_k
                );
                let samplers = vec![
                    LlamaSampler::temp(self.config.temperature),
                    LlamaSampler::top_k(self.config.top_k as i32),
                ];
                LlamaSampler::chain_simple(samplers)
            }
            SamplerType::ChainFull => {
                println!(
                    "Using full chain sampler (temp: {}, top_p: {}, top_k: {})",
                    self.config.temperature, self.config.top_p, self.config.top_k
                );
                let samplers = vec![
                    LlamaSampler::temp(self.config.temperature),
                    LlamaSampler::top_p(self.config.top_p, 1),
                    LlamaSampler::top_k(self.config.top_k as i32),
                ];
                LlamaSampler::chain_simple(samplers)
            }
        }
    }

    pub async fn generate_response(&self, user_message: &str) -> Result<String, String> {
        // Use the actual LLaMA generation logic
        self.generate_llama_response(user_message).await
    }

    async fn generate_llama_response(&self, user_message: &str) -> Result<String, String> {
        // Create sampler for this generation
        let mut sampler = self.create_sampler();
        
        // Use proper Granite chat format with dynamic OS detection
        let system_prompt = get_system_prompt();
        let prompt = format!(
            "<|start_of_role|>system<|end_of_role|>{}<|end_of_text|><|start_of_role|>user<|end_of_role|>{}<|end_of_text|><|start_of_role|>assistant<|end_of_role|>",
            system_prompt, user_message
        );

        // Tokenize
        let tokens = self.model
            .str_to_token(&prompt, AddBos::Never)
            .map_err(|e| format!("Tokenization failed: {}", e))?;

        // Create context with safe size
        let context_size = get_context_size();
        let n_ctx = NonZeroU32::new(context_size).unwrap();
        let ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
        let mut context = self.model
            .new_context(&self.backend, ctx_params)
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

        // Generate response - limited to reasonable length
        let mut response = String::new();
        let mut token_pos = tokens.len() as i32;
        let max_tokens = 2048; // Limit response length

        for _ in 0..max_tokens {
            // Sample next token
            let next_token = sampler.sample(&context, -1);

            // Check for end-of-sequence token
            if next_token == self.model.token_eos() {
                break;
            }

            // Convert token to string
            let token_str = self.model
                .token_to_str(next_token, Special::Tokenize)
                .map_err(|e| format!("Token conversion failed: {}", e))?;

            response.push_str(&token_str);

            // Check for natural stopping points
            if response.contains("<|end_of_text|>") || response.contains("<|end_of_role|>") {
                break;
            }

            // Prepare next batch
            batch.clear();
            batch
                .add(next_token, token_pos, &[0], true)
                .map_err(|e| format!("Batch add failed: {}", e))?;

            // Decode next token
            context
                .decode(&mut batch)
                .map_err(|e| format!("Decode failed: {}", e))?;

            token_pos += 1;
        }

        // Clean up the response
        let response = response
            .replace("<|end_of_text|>", "")
            .replace("<|end_of_role|>", "")
            .trim()
            .to_string();

        Ok(response)
    }
}

// System prompt function (extracted from test.rs)
pub fn get_system_prompt() -> String {
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