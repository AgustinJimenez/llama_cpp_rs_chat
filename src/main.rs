use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{LlamaModel, AddBos, Special},
    llama_batch::LlamaBatch,
    sampling::LlamaSampler,
    context::params::LlamaContextParams,
};

use std::num::NonZeroU32;

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
    let model_path_file = Path::new("assets/model_path.txt");
    let model_path: String = if model_path_file.exists() {
        let prev_path = fs::read_to_string(model_path_file)?.trim().to_string();
        println!("\n\n- Use previous model path? [{}] (Y/n)", prev_path);
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if answer.trim().to_lowercase() == "n" {
            ask_and_save_model_path(model_path_file)?
        } else {
            prev_path
        }
    } else {
        ask_and_save_model_path(model_path_file)?
    };

    let gguf_file = Path::new(&model_path).to_path_buf();
    if !gguf_file.exists() || gguf_file.extension().unwrap_or_default() != "gguf" {
        return Err(anyhow::anyhow!("Provided path is not a valid .gguf file"));
    }

    let prompt_format = detect_prompt_format(&gguf_file);

    println!("\n- Set max context size (n_ctx, default 8192): ");
    let mut n_ctx_input = String::new();
    io::stdin().read_line(&mut n_ctx_input)?;
    let n_ctx = n_ctx_input.trim().parse::<u32>().unwrap_or(8192);
    let n_ctx = NonZeroU32::new(n_ctx).unwrap_or_else(|| NonZeroU32::new(8192).unwrap());

    let backend = LlamaBackend::init()?;
    let model = LlamaModel::load_from_file(&backend, &gguf_file, &Default::default())?;

    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
    let mut ctx = model.new_context(&backend, ctx_params)?;
    let mut sampler = LlamaSampler::greedy();

    let mut conversation: Vec<ChatMessage> = Vec::new();
    let convo_id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let convo_path = format!("assets/conversations/chat_{}.json", convo_id);

    let system_prompt = match prompt_format {
        PromptFormat::Mistral => "<s>[INST] <<SYS>>\nYou are Devstral, a helpful agentic model trained by Mistral AI.\n<</SYS>>\n",
        PromptFormat::Qwen => "<|im_start|>system\nYou are a helpful assistant.<|im_end|>",
    };

    conversation.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
    });
    save_conversation(&conversation, &convo_path)?;

    println!("\n\nInteractive Chat Started (type 'exit' to quit)\n");

    loop {
        print!("\n\nYou: ");
        io::stdout().flush().unwrap();

        let mut user_input = String::new();
        if io::stdin().read_line(&mut user_input).is_err() {
            println!("\nError reading input.");
            break;
        }

        let user_input = user_input.trim();
        if user_input.eq_ignore_ascii_case("exit") {
            println!("\nEnding chat session...");
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
        for msg in &conversation {
            match prompt_format {
                PromptFormat::Mistral => match msg.role.as_str() {
                    "system" => full_prompt.push_str(&msg.content),
                    "user" => full_prompt.push_str(&format!("\n[INST] {} [/INST]", msg.content)),
                    "assistant" => full_prompt.push_str(&format!(" {} </s>", msg.content)),
                    _ => (),
                },
                PromptFormat::Qwen => match msg.role.as_str() {
                    "system" => full_prompt.push_str(&msg.content),
                    "user" => full_prompt.push_str(&format!("\n<|im_start|>user\n{}<|im_end|>", msg.content)),
                    "assistant" => full_prompt.push_str(&format!("\n<|im_start|>assistant\n{}<|im_end|>", msg.content)),
                    _ => (),
                },
            }
        }

        let tokens = model.str_to_token(&full_prompt, AddBos::Never)?;
        let mut batch = LlamaBatch::new(1024, 1);
        for (i, &token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch.add(token, i as i32, &[0], is_last)?;
        }

        ctx.decode(&mut batch)?;

        let mut response = String::new();
        print!("\nAssistant: ");
        io::stdout().flush().unwrap();

        let mut token_pos = tokens.len() as i32;
        loop {
            let token = sampler.sample(&ctx, -1);
            if token == model.token_eos() {
                break;
            }

            if let Ok(token_str) = model.token_to_str(token, Special::Tokenize) {
                print!("{}", token_str);
                io::stdout().flush().unwrap();
                response.push_str(&token_str);
            }

            batch.clear();
            if let Err(e) = batch.add(token, token_pos, &[0], true) {
                eprintln!("\nError: {}", e);
                break;
            }
            if let Err(e) = ctx.decode(&mut batch) {
                eprintln!("\nError: {}", e);
                break;
            }
            token_pos += 1;
        }

        println!();

        conversation.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.trim().to_string(),
        });
        save_conversation(&conversation, &convo_path)?;
    }

    Ok(())
}

fn detect_prompt_format(path: &PathBuf) -> PromptFormat {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
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

fn save_conversation(convo: &[ChatMessage], file_path: &str) -> Result<()> {
    let dir = Path::new("assets/conversations");
    fs::create_dir_all(dir)?;
    let file = File::create(file_path)?;
    serde_json::to_writer_pretty(file, &convo)?;
    Ok(())
}
