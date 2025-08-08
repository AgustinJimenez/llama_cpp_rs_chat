use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use sysinfo::System;

use anyhow::Result;

mod llm_backend;
mod llamacpp_backend;
mod ai_operations;
mod command_executor;
mod command_detection;
mod command_runner;

use llm_backend::*;
use llamacpp_backend::LlamaCppBackendImpl;
use command_detection::{extract_command_from_response, response_contains_commands};
use command_runner::{execute_command, should_generate_followup};

#[cfg(target_os = "windows")]
use wmi::{COMLibrary, WMIConnection};

fn initialize_backend() -> Result<()> {
    println!("🦙 Using LLaMA.cpp backend");
    Ok(())
}

fn try_nvidia_smi() -> Option<u64> {
    println!("🔍 Trying nvidia-smi...");
    match Command::new("nvidia-smi")
        .args(&["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if let Ok(vram_mb) = line.trim().parse::<u64>() {
                    let vram_bytes = vram_mb * 1_048_576; // Convert MB to bytes
                    println!("  ✅ nvidia-smi detected: {:.2} GB", vram_bytes as f64 / 1_073_741_824.0);
                    return Some(vram_bytes);
                }
            }
        }
        _ => println!("  ❌ nvidia-smi not available or failed")
    }
    None
}

fn try_wmic_pnpentity() -> Option<u64> {
    println!("🔍 Trying wmic PnPEntity query...");
    match Command::new("wmic")
        .args(&["path", "win32_VideoController", "get", "AdapterRAM", "/format:list"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if line.starts_with("AdapterRAM=") {
                    if let Ok(vram_bytes) = line[11..].trim().parse::<u64>() {
                        if vram_bytes > 0 {
                            println!("  ✅ wmic detected: {:.2} GB", vram_bytes as f64 / 1_073_741_824.0);
                            return Some(vram_bytes);
                        }
                    }
                }
            }
        }
        _ => println!("  ❌ wmic query failed")
    }
    None
}

fn try_powershell_gpu() -> Option<u64> {
    println!("🔍 Trying PowerShell GPU query...");
    match Command::new("powershell")
        .args(&["-Command", "Get-WmiObject -Class Win32_VideoController | Where-Object {$_.AdapterRAM -gt 0} | ForEach-Object {$_.AdapterRAM}"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let mut max_vram = 0u64;
            for line in output_str.lines() {
                if let Ok(vram_bytes) = line.trim().parse::<u64>() {
                    max_vram = max_vram.max(vram_bytes);
                }
            }
            if max_vram > 0 {
                println!("  ✅ PowerShell detected: {:.2} GB", max_vram as f64 / 1_073_741_824.0);
                return Some(max_vram);
            }
        }
        _ => println!("  ❌ PowerShell query failed")
    }
    None
}

fn try_macos_system_profiler() -> Option<u64> {
    println!("🔍 Trying macOS system_profiler...");
    
    // First try to get discrete GPU VRAM
    match Command::new("system_profiler")
        .args(&["SPDisplaysDataType"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            
            // Look for VRAM patterns in human-readable output
            for line in output_str.lines() {
                let line = line.trim();
                if line.starts_with("VRAM (Total):") || line.starts_with("VRAM (Dynamic, Max):") {
                    // Extract VRAM size - pattern like "VRAM (Total): 8 GB"
                    if let Some(colon_pos) = line.find(':') {
                        let vram_part = &line[colon_pos + 1..].trim();
                        let parts: Vec<&str> = vram_part.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(size) = parts[0].parse::<f64>() {
                                let vram_bytes = match parts[1].to_lowercase().as_str() {
                                    "gb" => (size * 1_073_741_824.0) as u64,
                                    "mb" => (size * 1_048_576.0) as u64,
                                    _ => continue,
                                };
                                if vram_bytes > 0 {
                                    println!("  ✅ system_profiler detected: {:.2} GB", vram_bytes as f64 / 1_073_741_824.0);
                                    return Some(vram_bytes);
                                }
                            }
                        }
                    }
                }
            }
            
            // For Apple Silicon Macs, check if it's Apple GPU and estimate based on total memory
            if output_str.contains("Apple M1") || output_str.contains("Apple M2") || output_str.contains("Apple M3") {
                println!("  🍎 Detected Apple Silicon Mac - using memory-based estimation");
                
                // Get total system memory and use a portion for GPU estimation
                let mut sys = sysinfo::System::new_all();
                sys.refresh_all();
                let total_memory = sys.total_memory();
                
                // Apple Silicon typically allocates about 25-40% of system memory for GPU
                // Use a conservative 25% for estimation
                let estimated_vram = (total_memory as f64 * 0.25) as u64;
                if estimated_vram > 0 {
                    println!("  ✅ Apple Silicon estimated GPU memory: {:.2} GB", estimated_vram as f64 / 1_073_741_824.0);
                    return Some(estimated_vram);
                }
            }
        }
        _ => println!("  ❌ system_profiler query failed")
    }
    None
}

fn get_vram() -> Result<u64> {
    println!("\n🔍 Detecting VRAM using multiple methods...");

    // Try NVIDIA tools first (works on all platforms with NVIDIA GPUs)
    if let Some(vram) = try_nvidia_smi() {
        return Ok(vram);
    }

    // Platform-specific detection methods
    #[cfg(target_os = "macos")]
    {
        if let Some(vram) = try_macos_system_profiler() {
            return Ok(vram);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(vram) = try_wmic_pnpentity() {
            return Ok(vram);
        }
        if let Some(vram) = try_powershell_gpu() {
            return Ok(vram);
        }
        
        println!("🔍 Trying WMI (fallback method)...");
        match try_wmi_detection() {
            Ok(vram) if vram > 0 => {
                println!("  ✅ WMI detected: {:.2} GB", vram as f64 / 1_073_741_824.0);
                return Ok(vram);
            }
            _ => println!("  ❌ WMI detection failed")
        }
    }

    println!("\n⚠️  Could not auto-detect VRAM using any method.");
    println!("   Please enter your GPU's VRAM in GB (e.g., 24 for 24GB): ");
    let mut vram_input = String::new();
    io::stdin().read_line(&mut vram_input)?;
    if let Ok(vram_gb) = vram_input.trim().parse::<f64>() {
        let vram_bytes = (vram_gb * 1_073_741_824.0) as u64;
        println!("   ✅ Using manually entered VRAM: {:.2} GB", vram_gb);
        return Ok(vram_bytes);
    }

    Ok(0)
}

#[cfg(target_os = "windows")]
fn try_wmi_detection() -> Result<u64> {
    let com_lib = COMLibrary::new()?;
    let wmi_con = WMIConnection::new(com_lib.into())?;

    let results: Vec<std::collections::HashMap<String, serde_json::Value>> = wmi_con
        .raw_query("SELECT Name, AdapterRAM FROM Win32_VideoController")?;

    let mut max_vram = 0u64;
    for video_controller in results {
        let vram = if let Some(serde_json::Value::Number(ram_num)) = video_controller.get("AdapterRAM") {
            ram_num.as_u64().unwrap_or(0)
        } else if let Some(serde_json::Value::String(ram_str)) = video_controller.get("AdapterRAM") {
            ram_str.parse::<u64>().unwrap_or(0)
        } else {
            0
        };
        max_vram = max_vram.max(vram);
    }

    Ok(max_vram)
}

#[cfg(not(target_os = "windows"))]
fn try_wmi_detection() -> Result<u64> {
    Ok(0)
}

fn clear_terminal() {
    for _ in 0..4 {
        print!("\x1B[2J\x1B[1;1H");
    }
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
    let convo_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let convo_path = format!("assets/conversations/chat_{}.json", convo_id);
    // Command executor is now handled by the command_runner module

    let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let os_name = std::env::consts::OS;
    
    let (command_examples, path_format) = match os_name {
        "windows" => (
            "Examples: `<|EXEC|>dir<|/EXEC|>` (list files), `<|EXEC|>mkdir myproject<|/EXEC|>` (create directory), `<|EXEC|>echo print('hello') > main.py<|/EXEC|>` (create file), `<|EXEC|>curl -s https://api.github.com/repos/microsoft/vscode<|/EXEC|>` (web fetch), `<|EXEC|>findstr . filename.txt<|/EXEC|>` (read file), `<|EXEC|>git init<|/EXEC|>` (initialize repo). Use '&&' to chain commands: `<|EXEC|>mkdir api && cd api && echo code > app.py<|/EXEC|>`. Use maximum 1-2 commands per response.",
            "Use Windows paths like C:\\path\\to\\file or relative paths like .\\src\\main.rs"
        ),
        "linux" | "macos" => (
            "Examples: `<|EXEC|>ls -la<|/EXEC|>` (list files), `<|EXEC|>cat filename.txt<|/EXEC|>` (read file), `<|EXEC|>cd subfolder<|/EXEC|>` (change directory)", 
            "Use Unix paths like /path/to/file or relative paths like ./src/main.rs"
        ),
        _ => (
            "Examples: `<|EXEC|>ls -la<|/EXEC|>` or `<|EXEC|>dir<|/EXEC|>` (list files), depending on your system",
            "Use appropriate path format for your operating system"
        )
    };

    let system_prompt = format!(
        "You are an intelligent AI assistant that can execute system commands when needed. \
         You have access to the command line through `<|EXEC|>command<|/EXEC|>` tags, but you should primarily \
         provide helpful explanations and analysis. Only use commands when necessary to \
         gather specific information or perform requested tasks.\
         \
         🔧 COMMAND ACCESS: \
         You can execute commands by wrapping them in `<|EXEC|>command<|/EXEC|>` tags when you need to: \
         • Check file contents or directory listings \
         • Perform specific tasks requested by the user \
         • Gather system information that helps answer questions \
         \
         SYSTEM INFO: \
         - Operating System: {} \
         - Current working directory: {} \
         - {} \
         \
         ⚠️ GUIDELINES: \
         • Provide thoughtful explanations and analysis \
         • Only use commands when they add specific value \
         • Use maximum 1-2 commands per response \
         • Explain shortly what you're doing before running commands \
         • Focus on being helpful through knowledge, not just commands \
         • NEVER add code blocks or markdown after <|EXEC|> - wait for execution results \
         \
         Command examples: {} \
         \
         Be helpful, informative, and use commands strategically to enhance your responses.",
        os_name,
        current_dir.display(),
        path_format,
        command_examples
    );

    conversation.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
    });
    save_conversation(&conversation, &convo_path)?;

    println!("\n\n\x1B[1;33m🚀 Interactive Chat Started\x1B[0m \x1B[90m(type 'exit' to quit)\x1B[0m");
    println!();

    loop {
        print!("\n\n\x1B[36mYou: \x1B[0m");
        io::stdout().flush().unwrap();

        let mut user_input = String::new();
        if io::stdin().read_line(&mut user_input)? == 0 {
            println!("\n\x1B[1;31m👋 End of input - closing chat session...\x1B[0m");
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

        print!("\n\x1B[32mAssistant: \x1B[0m");
        io::stdout().flush().unwrap();

        let gen_config = GenerationConfig {
            max_tokens: 4096,
            stop_strings: vec!["<|im_end|>".to_string(), "<|end|>".to_string(), "</s>".to_string()],
        };

        let generation_start = std::time::SystemTime::now();
        let token_count = Arc::new(Mutex::new(0u32));
        let token_count_clone = Arc::clone(&token_count);

        println!("🔍 MAIN: About to call backend.generate_response");
        
        let response = match backend.generate_response(
            &conversation,
            gen_config.clone(),
            Box::new(move |token_info| {
                print!("{}", token_info.token_str);
                let mut count = token_count_clone.lock().unwrap();
                *count += 1;
                io::stdout().flush().unwrap();
                true
            }),
        ) {
            Ok(resp) => {
                println!("\n🔍 MAIN: backend.generate_response returned successfully");
                resp
            },
            Err(e) => {
                println!("\n❌ MAIN: backend.generate_response failed with error: {}", e);
                return Err(e);
            }
        };

        // === RESPONSE PROCESSING SECTION ===
        println!("\n📋 MAIN: Response generation completed successfully");
        println!("📋 MAIN: Response length: {}", response.len());
        println!("📋 MAIN: Response preview: '{}'", 
                if response.len() > 100 { &response[..100] } else { &response });
        
        // Force flush to ensure output appears immediately
        io::stdout().flush().unwrap();

        // Add assistant response to conversation
        conversation.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.trim().to_string(),
        });

        // === COMMAND DETECTION SECTION ===
        println!("📋 MAIN: Checking for commands in response...");
        
        if response_contains_commands(&response) {
            println!("✅ MAIN: Commands detected, starting command processing...");
            
            // Extract the command using our modular detector
            match extract_command_from_response(&response) {
                Some(extracted_command) => {
                    println!("✅ MAIN: Command extracted successfully");
                    
                    // Execute the command using our modular runner
                    match execute_command(extracted_command.clone()) {
                        Ok(command_result) => {
                            println!("✅ MAIN: Command executed, adding result to conversation");
                            conversation.push(command_result);
                            
                            // Save conversation after command execution
                            save_conversation(&conversation, &convo_path)?;
                            
                            // Generate follow-up response if appropriate
                            if should_generate_followup(&extracted_command.command) {
                                println!("🔄 MAIN: Generating follow-up analysis...");
                                
                                print!("\n\x1B[32mAssistant: \x1B[0m");
                                io::stdout().flush().unwrap();
                                
                                // Use smaller token limit for continuation
                                let continue_config = GenerationConfig {
                                    max_tokens: 512,
                                    stop_strings: gen_config.stop_strings.clone(),
                                };
                                
                                match backend.generate_response(
                                    &conversation,
                                    continue_config,
                                    Box::new(move |token_info| {
                                        print!("{}", token_info.token_str);
                                        io::stdout().flush().unwrap();
                                        true
                                    }),
                                ) {
                                    Ok(continuation_response) => {
                                        if !continuation_response.trim().is_empty() {
                                            conversation.push(ChatMessage {
                                                role: "assistant".to_string(),
                                                content: continuation_response.trim().to_string(),
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        println!("\n⚠️ MAIN: Could not generate follow-up analysis: {}", e);
                                    }
                                }
                            } else {
                                println!("📋 MAIN: No follow-up analysis needed for this command");
                            }
                        }
                        Err(e) => {
                            println!("❌ MAIN: Command execution failed: {}", e);
                            conversation.push(ChatMessage {
                                role: "system".to_string(),
                                content: format!("Command execution failed: {}", e),
                            });
                        }
                    }
                }
                None => {
                    println!("❌ MAIN: Failed to extract command from response");
                    conversation.push(ChatMessage {
                        role: "system".to_string(),
                        content: "Error: Could not extract command from response".to_string(),
                    });
                }
            }
        } else {
            println!("📋 MAIN: No commands detected in response");
        }


        let total_elapsed = std::time::SystemTime::now().duration_since(generation_start).unwrap_or_default();
        let final_count = *token_count.lock().unwrap();
        let final_tps = if total_elapsed.as_secs_f64() > 0.0 {
            final_count as f64 / total_elapsed.as_secs_f64()
        } else {
            0.0
        };

        println!(
            "\n\x1B[90m⚡ Generation complete: {} tokens in {:.1}s ({:.1} tokens/sec)\x1B[0m",
            final_count,
            total_elapsed.as_secs_f64(),
            final_tps
        );

        println!("🔍 MAIN: About to call get_context_info");
        let context_info = match backend.get_context_info(&conversation, &response) {
            Ok(info) => {
                println!("🔍 MAIN: get_context_info returned successfully");
                info
            },
            Err(e) => {
                println!("❌ MAIN: get_context_info failed with error: {}", e);
                return Err(e);
            }
        };
        let usage_color = if context_info.usage_percent >= 90 {
            "\x1B[1;31m"
        } else if context_info.usage_percent >= 70 {
            "\x1B[1;33m"
        } else {
            "\x1B[1;32m"
        };

        println!(
            "\x1B[90m📊 Context: {}{}/{}\x1B[0m \x1B[90m({}% used, {} tokens remaining) \x1B[90m[{}]\x1B[0m",
            usage_color,
            context_info.total_tokens,
            context_info.context_size,
            context_info.usage_percent,
            context_info.context_size - context_info.total_tokens,
            backend.backend_name()
        );

        if context_info.usage_percent >= 85 {
            println!("\x1B[1;33m⚠️  Warning: Context is {}% full. Consider starting a new conversation soon to avoid truncated responses.\x1B[0m", context_info.usage_percent);
        } else if context_info.usage_percent >= 95 {
            println!("\x1B[1;31m🚨 Critical: Context is {}% full! Responses may be cut short. Type 'exit' and start a new chat.\x1B[0m", context_info.usage_percent);
        }

        println!("🔍 MAIN: About to save conversation");
        save_conversation(&conversation, &convo_path)?;
        println!("🔍 MAIN: Conversation saved successfully");
        println!("🔍 MAIN: End of main loop iteration");
    }

    Ok(())
}


fn main() -> Result<()> {
    clear_terminal();
    
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

    let model_size = fs::metadata(gguf_file)?.len();
    let mut sys = System::new_all();
    sys.refresh_all();
    let total_ram = sys.total_memory();
    let available_ram = sys.available_memory();
    let vram = get_vram().unwrap_or(0);

    println!("\n\n---");
    println!("  Model Size: {:.2} GB", model_size as f64 / 1_073_741_824.0);
    println!("  Available RAM: {:.2} GB / {:.2} GB", available_ram as f64 / 1_073_741_824.0, total_ram as f64 / 1_073_741_824.0);
    if vram > 0 {
        println!("  Available VRAM: {:.2} GB", vram as f64 / 1_073_741_824.0);
    }
    println!("---");

    let mut n_gpu_layers = 0;
    if vram > 0 {
        let model_size_gb = model_size as f64 / 1_073_741_824.0;
        let vram_gb = vram as f64 / 1_073_741_824.0;
        let available_ram_gb = available_ram as f64 / 1_073_741_824.0;
        
        let usable_vram_gb = (vram_gb - 2.0).max(0.0);
        
        let estimated_layers_per_gb = if model_size_gb > 20.0 { 1.0 } else { 1.5 };
        let max_gpu_layers = (usable_vram_gb * estimated_layers_per_gb) as u32;
        
        println!("\n🧠 Smart GPU Offloading Analysis:");
        println!("  Model Size: {:.2} GB", model_size_gb);
        println!("  Available VRAM: {:.2} GB (usable: {:.2} GB after 2GB buffer)", vram_gb, usable_vram_gb);
        println!("  Available RAM: {:.2} GB", available_ram_gb);
        
        let (recommended_layers, recommendation_reason) = if usable_vram_gb >= model_size_gb {
            (999, "🚀 Full GPU offload recommended - model fits entirely in VRAM")
        } else if usable_vram_gb >= model_size_gb * 0.75 {
            (max_gpu_layers, "⚡ High GPU offload recommended - most model fits in VRAM")
        } else if usable_vram_gb >= model_size_gb * 0.5 {
            ((max_gpu_layers as f64 * 0.8) as u32, "⚖️  Balanced offload recommended - split between GPU/CPU")
        } else if usable_vram_gb >= model_size_gb * 0.25 {
            ((max_gpu_layers as f64 * 0.6) as u32, "🔄 Light GPU offload recommended - mostly CPU with GPU assist")
        } else if available_ram_gb < model_size_gb {
            (max_gpu_layers.min(20), "⚠️  Model too large for system - offload what you can to GPU")
        } else {
            (0, "💾 CPU-only recommended - insufficient VRAM, but enough RAM")
        };
        
        println!("  Recommendation: {}", recommendation_reason);
        println!("  Estimated max GPU layers: ~{}", max_gpu_layers);
        println!();
        
        println!("- GPU Layer Options:");
        println!("  0     - CPU only");
        if max_gpu_layers >= 10 {
            println!("  {}   - Light GPU assist", max_gpu_layers / 4);
        }
        if max_gpu_layers >= 20 {
            println!("  {}   - Balanced GPU/CPU", max_gpu_layers / 2);
        }
        if max_gpu_layers >= 30 {
            println!("  {}   - Heavy GPU", (max_gpu_layers as f64 * 0.8) as u32);
        }
        println!("  999   - Full GPU (all available layers)");
        
        println!("\n- Enter number of layers to offload to GPU ({} layers default):", recommended_layers);
        print!("-> ");
        io::stdout().flush()?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        
        if let Ok(layers) = input.parse::<u32>() {
            n_gpu_layers = layers;
            println!("  Using {} layers on GPU", layers);
        } else if input.is_empty() {
            n_gpu_layers = recommended_layers;
            println!("  Using recommended {} layers on GPU", recommended_layers);
        } else {
            println!("  Invalid input, using recommended {} layers", recommended_layers);
            n_gpu_layers = recommended_layers;
        }
    }

    let model_config = ModelConfig {
        context_size: n_ctx,
        model_path: model_path_trimmed.to_string(),
        prompt_format,
        n_gpu_layers,
    };

    println!("🦙 Initializing LLaMA.cpp backend...");
    if n_gpu_layers > 0 {
        println!("🚀 GPU Offloading: {} layers will be processed on GPU", n_gpu_layers);
    } else {
        println!("💾 CPU-only mode: All processing will be on CPU");
    }
    clear_terminal();
    let backend = LlamaCppBackendImpl::initialize(model_config)?;
    run_chat_with_backend(backend)
}