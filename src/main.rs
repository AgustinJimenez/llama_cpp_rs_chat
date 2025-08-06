use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use sysinfo::{System};

use anyhow::Result;

mod llm_backend;
mod llamacpp_backend;
mod ai_operations;
mod command_executor;
mod ai_chat_integration;
mod debug_logger;

use llm_backend::*;
use llamacpp_backend::LlamaCppBackendImpl;
use ai_operations::{CommandRequest, CommandExecutor};
use command_executor::SystemCommandExecutor;

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

fn get_vram() -> Result<u64> {
    println!("\n🔍 Detecting VRAM using multiple methods...");

    if let Some(vram) = try_nvidia_smi() {
        return Ok(vram);
    }
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
    let command_executor = SystemCommandExecutor::new();

    let system_prompt = 
        "You are a helpful AI assistant with the ability to execute shell commands. "
        .to_string() +
        "To execute a command, wrap it in `!CMD!` tags. For example: `!CMD!ls -l`. " +
        "The command will be executed and the output will be provided to you. " +
        &format!("You are running on a {} operating system. The current working directory is {}.",
        std::env::consts::OS,
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).display());

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

        let response = backend.generate_response(
            &conversation,
            gen_config.clone(),
            Box::new(move |token_info| {
                print!("{}", token_info.token_str);
                let mut count = token_count_clone.lock().unwrap();
                *count += 1;
                io::stdout().flush().unwrap();
                true
            }),
        )?;

        conversation.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.trim().to_string(),
        });

        if response.contains("!CMD!") {
            let command_to_execute = response.split("!CMD!").collect::<Vec<&str>>()[1];
            let parts: Vec<&str> = command_to_execute.split_whitespace().collect();
            let command = parts[0].to_string();
            let args = parts[1..].iter().map(|s| s.to_string()).collect();

            let request = CommandRequest {
                command,
                args,
                working_dir: None,
                timeout_ms: None,
                environment: std::collections::HashMap::new(),
            };

            let result = command_executor.execute(request)?;
            let output = if result.success {
                result.output
            } else {
                result.error
            };

            conversation.push(ChatMessage {
                role: "system".to_string(),
                content: format!("Command output:\n```\n{}\n```", output),
            });

            // Let the assistant continue after the command execution
            print!("\n\x1B[32mAssistant: \x1B[0m");
            io::stdout().flush().unwrap();
            let continuation_response = backend.generate_response(
                &conversation,
                gen_config,
                Box::new(move |token_info| {
                    print!("{}", token_info.token_str);
                    io::stdout().flush().unwrap();
                    true
                }),
            )?;
            conversation.push(ChatMessage {
                role: "assistant".to_string(),
                content: continuation_response.trim().to_string(),
            });
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

        let context_info = backend.get_context_info(&conversation, &response)?;
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

        save_conversation(&conversation, &convo_path)?;
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
    
    let backend = LlamaCppBackendImpl::initialize(model_config)?;
    run_chat_with_backend(backend)
}