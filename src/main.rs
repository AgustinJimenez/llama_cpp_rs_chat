use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::{Arc, Mutex};
use sysinfo::{System};

use anyhow::Result;

mod llm_backend;
mod llamacpp_backend;
mod ai_operations;
mod command_executor;
mod file_manager;
mod operation_logger;
mod project_templates;
mod ai_chat_integration;

use llm_backend::*;
use llamacpp_backend::LlamaCppBackendImpl;
use ai_chat_integration::AIOperationsManager;

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
    
    // Method 1: Try nvidia-smi (most accurate for NVIDIA cards)
    if let Some(vram) = try_nvidia_smi() {
        return Ok(vram);
    }
    
    // Method 2: Try wmic command line
    if let Some(vram) = try_wmic_pnpentity() {
        return Ok(vram);
    }
    
    // Method 3: Try PowerShell
    if let Some(vram) = try_powershell_gpu() {
        return Ok(vram);
    }
    
    // Method 4: Original WMI approach as fallback
    println!("🔍 Trying WMI (fallback method)...");
    match try_wmi_detection() {
        Ok(vram) if vram > 0 => {
            println!("  ✅ WMI detected: {:.2} GB", vram as f64 / 1_073_741_824.0);
            return Ok(vram);
        }
        _ => println!("  ❌ WMI detection failed")
    }
    
    // Method 5: Manual input as last resort
    println!("\n⚠️  Could not auto-detect VRAM using any method.");
    println!("   For RTX 4090, this should be 24GB. Please enter manually:");
    println!("   Enter your GPU's VRAM in GB (e.g., 24 for 24GB): ");
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
    
    // Initialize AI operations manager
    let mut ai_ops = match AIOperationsManager::new() {
        Ok(ops) => Some(ops),
        Err(e) => {
            println!("⚠️  Warning: AI operations disabled due to initialization error: {}", e);
            println!("   Chat will continue without AI command execution capabilities.");
            None
        }
    };

    // Add system message based on backend
    let system_prompt = if ai_ops.is_some() {
        "You are an advanced AI assistant with powerful capabilities. You can:

IMPORTANT: When users ask about files, always check if they exist first using /read-file <path>.

File Operations:
- /list-dir [path] - List directory contents (use /ls for short)
- /read-file <path> - Read any file (use this to check file contents)
- /create-file <path> <content> - Create new files
- /modify-file <path> <line> <content> - Edit files
- /delete-file <path> - Delete files
- /create-dir <path> - Create directories

System Commands:
- /execute <command> - Run system commands safely
- /list-templates - Show available project templates
- /create-project <template> <name> - Generate complete projects

When users ask about files (like TODO.md), ALWAYS:
1. First use /list-dir to see what files are available
2. Then use /read-file <filename> to read specific files
Don't say files don't exist without checking first! Always explore the directory structure.

Use /help to see all available commands."
    } else {
        match backend.backend_name() {
            "candle" => "You are a helpful AI assistant powered by Candle.",
            _ => "You are a helpful AI assistant.",
        }
    };

    conversation.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
    });
    save_conversation(&conversation, &convo_path)?;

    println!("\n\n\x1B[1;33m🚀 Interactive Chat Started\x1B[0m \x1B[90m(type 'exit' to quit)\x1B[0m");
    if ai_ops.is_some() {
        println!("\x1B[1;32m🤖 AI Operations Enabled\x1B[0m \x1B[90m(type '/help' for AI commands)\x1B[0m");
    }
    println!();

    loop {
        print!("\n\n\x1B[36mYou: \x1B[0m");  // Cyan color for "You:"
        io::stdout().flush().unwrap();

        let mut user_input = String::new();
        match io::stdin().read_line(&mut user_input) {
            Ok(0) => {
                println!("\n\x1B[1;31m👋 End of input - closing chat session...\x1B[0m");
                break; // EOF reached
            }
            Ok(_) => {
                let user_input = user_input.trim();
                if user_input.eq_ignore_ascii_case("exit") {
                    println!("\n\x1B[1;31m👋 Ending chat session...\x1B[0m");
                    break;
                }

                if user_input.is_empty() {
                    continue;
                }
                
                // Check if this is an AI operation request
                let mut ai_response = None;
                if let Some(ref mut ai_ops_manager) = ai_ops {
                    if user_input.starts_with('/') {
                        match ai_ops_manager.process_ai_request(user_input) {
                            Ok(response) => {
                                if response != "No AI operation detected in the message." {
                                    ai_response = Some(response);
                                }
                            }
                            Err(e) => {
                                ai_response = Some(format!("❌ AI operation error: {}", e));
                            }
                        }
                    }
                }
                
                // If it was an AI operation, display the result and continue
                if let Some(response) = ai_response {
                    println!("\n🤖 AI Operation Result: {}", response);
                    continue;
                }
                
                // Process the input as normal chat...
                conversation.push(ChatMessage {
                    role: "user".to_string(),
                    content: user_input.to_string(),
                });
            }
            Err(e) => {
                println!("\n\x1B[1;31m❌ Error reading input: {}\x1B[0m", e);
                println!("Chat session ended due to input error.");
                break;
            }
        }
        save_conversation(&conversation, &convo_path)?;
        
        print!("\n\x1B[32mAssistant: \x1B[0m");  // Green color for "Assistant:"
        io::stdout().flush().unwrap();

        // Setup generation config
        let gen_config = GenerationConfig {
            max_tokens: 4096,
            stop_strings: vec!["<|im_end|>".to_string(), "<|end|>".to_string(), "</s>".to_string()],
        };

        // Generate response with token callback that tracks TPS
        let generation_start = SystemTime::now();
        let token_count = Arc::new(Mutex::new(0u32));
        
        let token_count_clone = Arc::clone(&token_count);
        
        let response = backend.generate_response(
            &conversation,
            gen_config,
            Box::new(move |token_info| {
                print!("{}", token_info.token_str);
                
                let mut count = token_count_clone.lock().unwrap();
                *count += 1;
                // No live TPS display - just count tokens for final summary
                
                io::stdout().flush().unwrap();
                true // Continue generation
            })
        )?;

        // Calculate and display final TPS
        let total_elapsed = SystemTime::now().duration_since(generation_start).unwrap_or_default();
        let final_count = *token_count.lock().unwrap();
        let final_tps = if total_elapsed.as_secs_f64() > 0.0 {
            final_count as f64 / total_elapsed.as_secs_f64()
        } else {
            0.0
        };
        
        println!("\n\x1B[90m⚡ Generation complete: {} tokens in {:.1}s ({:.1} tokens/sec)\x1B[0m", 
                final_count, total_elapsed.as_secs_f64(), final_tps);

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
        // Smart GPU layer calculation
        let model_size_gb = model_size as f64 / 1_073_741_824.0;
        let vram_gb = vram as f64 / 1_073_741_824.0;
        let available_ram_gb = available_ram as f64 / 1_073_741_824.0;
        
        // Reserve some VRAM for context and operations (2GB safety buffer)
        let usable_vram_gb = (vram_gb - 2.0).max(0.0);
        
        // Estimate number of layers that can fit in VRAM
        // Rough estimate: each layer uses ~0.5-1GB for large models
        let estimated_layers_per_gb = if model_size_gb > 20.0 { 1.0 } else { 1.5 }; // Larger models have bigger layers
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

    // Create model configuration
    let model_config = ModelConfig {
        context_size: n_ctx,
        model_path: model_path_trimmed.to_string(),
        prompt_format,
        n_gpu_layers,
    };

    // Initialize and run LLaMA.cpp backend
    println!("🦙 Initializing LLaMA.cpp backend...");
    if n_gpu_layers > 0 {
        println!("🚀 GPU Offloading: {} layers will be processed on GPU", n_gpu_layers);
        println!("   💡 Tip: Monitor GPU usage with Task Manager or nvidia-smi to verify GPU activity");
    } else {
        println!("💾 CPU-only mode: All processing will be on CPU");
    }
    
    let backend = LlamaCppBackendImpl::initialize(model_config)?;
    run_chat_with_backend(backend)
}