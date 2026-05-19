/// VLM OCR subprocess: loads PaddleOCR-VL, runs OCR on an image, prints extracted text to stdout.
/// Runs on CPU (0 GPU layers) so it doesn't interfere with the main model on GPU.
#[cfg(feature = "vision")]
pub fn vlm_ocr_main(args: &[String]) -> std::io::Result<()> {
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::model::LlamaModel;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::sampling::LlamaSampler;
    use llama_cpp_2::mtmd::{MtmdContext, MtmdContextParams, MtmdBitmap, MtmdInputText};
    use std::ffi::CString;

    let get_arg = |flag: &str| -> Option<&str> {
        args.windows(2).find(|w| w[0] == flag).map(|w| w[1].as_str())
    };
    let model_path = get_arg("--model").unwrap_or("assets/ocr-vlm/PaddleOCR-VL-1.5.gguf");
    let mmproj_path = get_arg("--mmproj").unwrap_or("assets/ocr-vlm/PaddleOCR-VL-1.5-mmproj.gguf");
    let image_path = match get_arg("--image") {
        Some(p) => p,
        None => { eprintln!("Error: --image required"); std::process::exit(1); }
    };

    let io_err = |msg: String| std::io::Error::new(std::io::ErrorKind::Other, msg);

    // Init backend
    let backend = LlamaBackend::init().map_err(|e| io_err(format!("Backend: {e}")))?;

    // Load model on CPU (0 GPU layers — doesn't interfere with main model on GPU)
    let llama_model_params = LlamaModelParams::default().with_n_gpu_layers(0);
    let model = LlamaModel::load_from_file(&backend, model_path, &llama_model_params)
        .map_err(|e| io_err(format!("Model load: {e}")))?;

    // Load mmproj for vision
    let mtmd_params = MtmdContextParams {
        use_gpu: false,
        print_timings: false,
        n_threads: 4,
        media_marker: CString::new("<__media__>").unwrap(),
    };
    let vision = MtmdContext::init_from_file(mmproj_path, &model, &mtmd_params)
        .map_err(|e| io_err(format!("Mmproj: {e}")))?;

    // Create context
    let n_ctx = std::num::NonZeroU32::new(8192);
    let mut ctx_params = LlamaContextParams::default()
        .with_n_ctx(n_ctx)
        .with_n_batch(512)
        .with_flash_attention_policy(0); // GLM-OCR requires flash-attn OFF
    if vision.decode_use_non_causal() {
        ctx_params = ctx_params.with_flash_attention_policy(0);
    }
    let mut ctx = model.new_context(&backend, ctx_params)
        .map_err(|e| io_err(format!("Context: {e}")))?;

    // Load image
    let img_bytes = std::fs::read(image_path)?;
    let bitmap = MtmdBitmap::from_buffer(&vision, &img_bytes)
        .map_err(|e| io_err(format!("Image: {e}")))?;

    // Build prompt with image marker — use simple OCR prompt
    let prompt = "<__media__>OCR the text in this image:";
    let text_input = MtmdInputText {
        text: prompt.to_string(),
        add_special: true,
        parse_special: true,
    };
    let chunks = vision.tokenize(text_input, &[&bitmap])
        .map_err(|e| io_err(format!("Tokenize: {e}")))?;

    // Evaluate prompt + image through the model
    let n_past = chunks.eval_chunks(&vision, &ctx, 0, 0, 512, true)
        .map_err(|e| io_err(format!("Eval: {e}")))?;

    // Generate text output
    // Greedy decoding with repetition penalty to avoid loops
    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::penalties(2048, 1.3, 0.0, 0.0), // repeat_penalty=1.3
        LlamaSampler::temp(0.0),
        LlamaSampler::greedy(),
    ]);

    let mut batch = LlamaBatch::new(1, 1);
    let mut output = String::new();
    let mut token_pos = n_past;
    let eos = model.token_eos();

    for _ in 0..2048 {
        let token = sampler.sample(&ctx, -1);
        if token == eos { break; }

        #[allow(deprecated)]
        let s = model.token_to_str(token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        // Stop on special tokens like <|user|>, <|endoftext|>
        if s.contains("<|user|>") || s.contains("<|endoftext|>") || s.contains("<|assistant|>") {
            break;
        }
        output.push_str(&s);

        batch.clear();
        if batch.add(token, token_pos, &[0], true).is_err() { break; }
        if ctx.decode(&mut batch).is_err() { break; }
        token_pos += 1;
    }

    print!("{}", output.trim());
    Ok(())
}

#[cfg(not(feature = "vision"))]
pub fn vlm_ocr_main(_args: &[String]) -> std::io::Result<()> {
    eprintln!("VLM OCR requires the 'vision' feature");
    std::process::exit(1);
}
