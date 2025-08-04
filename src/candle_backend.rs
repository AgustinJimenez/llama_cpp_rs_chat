use crate::llm_backend::*;
use anyhow::{Result, Context};
use candle_core::{Device, DType, Tensor};
use candle_transformers::models::quantized_llama::ModelWeights;
use tokenizers::Tokenizer;
use std::path::Path;
use rand::{thread_rng, Rng};

const EOS_TOKEN: u32 = 2;

pub struct CandleBackendImpl {
    config: ModelConfig,
    model: ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
    temperature: Option<f64>,
}

impl LLMBackend for CandleBackendImpl {
    fn initialize(config: ModelConfig) -> Result<Self> {
        println!("🕯️  Initializing Candle backend");
        println!("📁 Model path: {}", config.model_path);
        println!("🔧 Context size: {}", config.context_size);
        
        let device = Device::cuda_if_available(0).unwrap_or(Device::Cpu);
        println!("🖥️  Device: {:?}", device);
        
        let (model, tokenizer) = Self::load_model_and_tokenizer(&config.model_path, &device)
            .with_context(|| "Failed to load model and tokenizer")?;
        
        let temperature = Some(0.8);
        
        println!("✅ Candle backend initialized successfully");
        
        Ok(Self {
            config,
            model,
            tokenizer,
            device,
            temperature,
        })
    }

    fn generate_response(
        &mut self,
        conversation: &[ChatMessage],
        gen_config: GenerationConfig,
        mut on_token: Box<dyn FnMut(TokenInfo) -> bool>,
    ) -> Result<String> {
        println!("🕯️  Generating response with Candle backend...");
        
        let prompt = self.build_prompt(conversation)?;
        let tokens = self.tokenizer.encode(prompt.clone(), true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?
            .get_ids()
            .to_vec();
        
        let mut generated_tokens = Vec::new();
        let mut response_text = String::new();
        let max_tokens = gen_config.max_tokens.min(self.config.context_size as usize - tokens.len());
        
        let mut input_tokens = tokens.clone();
        let mut _position = 0;
        
        for _step in 0..max_tokens {
            let input_tensor = Tensor::new(input_tokens.as_slice(), &self.device)?
                .reshape(&[1, input_tokens.len()])?;
            
            let logits = self.model.forward(&input_tensor, 0)?;
            let logits = logits.squeeze(0)?.squeeze(0)?.to_dtype(DType::F32)?;
            
            let next_token = self.sample_token(&logits)?;
            
            if next_token == EOS_TOKEN {
                break;
            }
            
            let token_str = self.tokenizer.decode(&[next_token], false)
                .map_err(|e| anyhow::anyhow!("Token decode failed: {}", e))?;
            
            if self.should_stop(&token_str, &gen_config.stop_strings) {
                break;
            }
            
            generated_tokens.push(next_token);
            response_text.push_str(&token_str);
            
            let token_info = TokenInfo {
                token_id: next_token,
                token_str: token_str.clone(),
            };
            
            if !on_token(token_info) {
                break;
            }
            
            input_tokens = vec![next_token];
            _position += input_tokens.len();
        }
        
        Ok(response_text)
    }

    fn get_context_info(&self, conversation: &[ChatMessage], response: &str) -> Result<ContextInfo> {
        let prompt = self.build_prompt(conversation)?;
        
        let prompt_tokens = self.tokenizer.encode(prompt, true)
            .map_err(|e| anyhow::anyhow!("Prompt tokenization failed: {}", e))?
            .len();
            
        let response_tokens = self.tokenizer.encode(response.to_string(), false)
            .map_err(|e| anyhow::anyhow!("Response tokenization failed: {}", e))?
            .len();
            
        let total_tokens = prompt_tokens + response_tokens;
        let usage_percent = (total_tokens as f32 / self.config.context_size as f32 * 100.0).round() as u32;

        Ok(ContextInfo {
            prompt_tokens,
            response_tokens,
            total_tokens,
            context_size: self.config.context_size as usize,
            usage_percent,
        })
    }

    fn backend_name(&self) -> &'static str {
        "candle"
    }

    fn clear_cache(&mut self) -> Result<()> {
        println!("🕯️  Clearing Candle backend cache");
        Ok(())
    }
}

impl CandleBackendImpl {
    fn load_model_and_tokenizer(model_path: &str, device: &Device) -> Result<(ModelWeights, Tokenizer)> {
        let model_path = Path::new(model_path);
        
        if !model_path.exists() {
            return Err(anyhow::anyhow!("Model file not found: {:?}", model_path));
        }
        
        println!("📦 Loading GGUF model from: {:?}", model_path);
        
        let mut file = std::fs::File::open(model_path)?;
        let content = candle_core::quantized::gguf_file::Content::read(&mut file)
            .with_context(|| "Failed to read GGUF file")?;
        let model = ModelWeights::from_gguf(content, &mut file, device)
            .with_context(|| "Failed to load GGUF model")?;
        
        let tokenizer_path = model_path.parent()
            .unwrap_or(model_path)
            .join("tokenizer.json");
            
        let tokenizer = if tokenizer_path.exists() {
            Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?
        } else {
            println!("⚠️  tokenizer.json not found, using default tokenizer");
            Self::create_default_tokenizer()?
        };
        
        println!("✅ Model and tokenizer loaded successfully");
        
        Ok((model, tokenizer))
    }
    
    fn create_default_tokenizer() -> Result<Tokenizer> {
        use tokenizers::models::bpe::BPE;
        use tokenizers::Tokenizer as TokenizerBuilder;
        
        let bpe = BPE::default();
        let tokenizer = TokenizerBuilder::new(bpe);
        Ok(tokenizer)
    }
    
    fn sample_token(&self, logits: &Tensor) -> Result<u32> {
        let logits_vec: Vec<f32> = logits.to_vec1()?;
        
        if let Some(temp) = self.temperature {
            if temp > 0.0 {
                let logits_vec: Vec<f32> = logits_vec.iter().map(|x| x / temp as f32).collect();
                let max_logit = logits_vec.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
                let exp_logits: Vec<f32> = logits_vec.iter().map(|x| (x - max_logit).exp()).collect();
                let sum_exp: f32 = exp_logits.iter().sum();
                let probs: Vec<f32> = exp_logits.iter().map(|x| x / sum_exp).collect();
                
                let mut rng = thread_rng();
                let rand_val: f32 = rng.sample(rand::distributions::Standard);
                let mut cumulative = 0.0;
                
                for (i, &prob) in probs.iter().enumerate() {
                    cumulative += prob;
                    if rand_val <= cumulative {
                        return Ok(i as u32);
                    }
                }
            }
        }
        
        let max_idx = logits_vec
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
            
        Ok(max_idx as u32)
    }
    
    fn build_prompt(&self, conversation: &[ChatMessage]) -> Result<String> {
        match self.config.prompt_format {
            PromptFormat::Mistral => {
                let mut prompt = String::new();
                for (i, msg) in conversation.iter().enumerate() {
                    match msg.role.as_str() {
                        "user" => {
                            if i == 0 {
                                prompt.push_str(&format!("<s>[INST] {} [/INST]", msg.content));
                            } else {
                                prompt.push_str(&format!(" [INST] {} [/INST]", msg.content));
                            }
                        }
                        "assistant" => {
                            prompt.push_str(&format!(" {}</s>", msg.content));
                        }
                        _ => {}
                    }
                }
                Ok(prompt)
            }
            PromptFormat::Qwen => {
                let mut prompt = String::new();
                for msg in conversation {
                    match msg.role.as_str() {
                        "user" => {
                            prompt.push_str(&format!("<|im_start|>user\n{}\n<|im_end|>\n", msg.content));
                        }
                        "assistant" => {
                            prompt.push_str(&format!("<|im_start|>assistant\n{}\n<|im_end|>\n", msg.content));
                        }
                        "system" => {
                            prompt.push_str(&format!("<|im_start|>system\n{}\n<|im_end|>\n", msg.content));
                        }
                        _ => {}
                    }
                }
                prompt.push_str("<|im_start|>assistant\n");
                Ok(prompt)
            }
        }
    }
    
    fn should_stop(&self, token_str: &str, stop_strings: &[String]) -> bool {
        for stop_str in stop_strings {
            if token_str.contains(stop_str) {
                return true;
            }
        }
        false
    }
}