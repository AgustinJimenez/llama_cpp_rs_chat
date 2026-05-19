use llama_cpp_2::llama_batch::LlamaBatch;

pub fn inject_output_tokens(
    tokens: &[i32],
    batch: &mut LlamaBatch<'_>,
    context: &mut llama_cpp_2::context::LlamaContext<'_>,
    token_pos: &mut i32,
    conversation_id: &str,
) -> Result<(), String> {
    eprintln!(
        "[INJECT] token_pos={}, injecting {} tokens, ctx_size={}, conv={}",
        token_pos,
        tokens.len(),
        context.n_ctx(),
        conversation_id
    );
    if let Ok(dump_dir) = std::env::var("LLAMA_CHAT_DATA_DIR") {
        let dump_path = format!("{}/logs/last_inject_dump.txt", dump_dir);
        let entry = format!("[INJECT pos={} count={}] {:?}\n", token_pos, tokens.len(), tokens);
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&dump_path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, entry.as_bytes()));
    }
    log_debug!(
        conversation_id,
        "Injecting {} output tokens into context",
        tokens.len()
    );

    context.synchronize();

    let total = tokens.len();
    for (i, &token) in tokens.iter().enumerate() {
        batch.clear();
        let is_last = i == total - 1;
        batch
            .add(
                llama_cpp_2::token::LlamaToken(token),
                *token_pos,
                &[0],
                is_last,
            )
            .map_err(|e| format!("Batch add failed for command output: {e}"))?;

        std::thread::yield_now();

        let decode_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            context.decode(batch)
        }));
        match decode_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let err_str = format!("{e}");
                if err_str.contains("NoKvCacheSlot") || err_str.contains("no kv cache slot") {
                    return Err("CONTEXT_EXHAUSTED".to_string());
                }
                return Err(format!("Decode failed for command output: {e}"));
            }
            Err(_panic) => {
                eprintln!(
                    "[INJECT] decode() panicked/threw C++ exception during injection at pos {}",
                    token_pos
                );
                return Err("Decode crashed during tool injection (C++ exception)".to_string());
            }
        }

        *token_pos += 1;
    }

    let ctx_size = context.n_ctx();
    if *token_pos as u32 >= ctx_size.saturating_sub(ctx_size / 20) {
        eprintln!(
            "[INJECT] Context 95% full after injection ({}/{})",
            token_pos, ctx_size
        );
        return Err("CONTEXT_EXHAUSTED".to_string());
    }

    Ok(())
}
