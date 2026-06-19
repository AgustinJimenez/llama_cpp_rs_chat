use llama_cpp_2::llama_batch::LlamaBatch;
use llama_chat_db::event_log::log_event;

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
    log_event(
        conversation_id,
        "inject_start",
        &format!("Injecting {} tokens at pos {}", tokens.len(), token_pos),
    );
    if let Ok(dump_dir) = std::env::var("LLAMA_CHAT_DATA_DIR") {
        let dump_path = format!("{dump_dir}/logs/last_inject_dump.txt");
        let entry = format!("[INJECT pos={token_pos} count={}] {tokens:?}\n", tokens.len());
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

    let ctx_size = context.n_ctx();
    let current_pos = (*token_pos).max(0) as u32;
    let headroom = ctx_size.saturating_sub(current_pos);
    if tokens.len() as u32 > headroom {
        eprintln!(
            "[INJECT] Pre-flight: {} tokens would overflow ctx (pos={}, ctx={}, headroom={}) — aborting injection",
            tokens.len(),
            token_pos,
            ctx_size,
            headroom
        );
        return Err("CONTEXT_EXHAUSTED".to_string());
    }

    let total = tokens.len();
    for (i, &token) in tokens.iter().enumerate() {
        if *token_pos < 0 || *token_pos as u32 >= ctx_size {
            eprintln!(
                "[INJECT] token_pos {token_pos} >= n_ctx {ctx_size} — aborting injection (CONTEXT_EXHAUSTED)"
            );
            return Err("CONTEXT_EXHAUSTED".to_string());
        }

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

        // Log before decode so the DB shows the last attempted token if decode hangs.
        if i == 0 || i == total - 1 || i % 50 == 0 {
            log_event(
                conversation_id,
                "decode_token",
                &format!("decode token {}/{} at pos {}", i + 1, total, token_pos),
            );
        }

        let decode_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            context.decode(batch)
        }));
        match decode_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let err_str = format!("{e}");
                if err_str.contains("NoKvCacheSlot") || err_str.contains("no kv cache slot") {
                    log_event(conversation_id, "inject_error", "CONTEXT_EXHAUSTED during tool injection");
                    return Err("CONTEXT_EXHAUSTED".to_string());
                }
                log_event(conversation_id, "inject_error", &format!("decode failed at token {i}: {e}"));
                return Err(format!("Decode failed for command output: {e}"));
            }
            Err(_panic) => {
                eprintln!(
                    "[INJECT] decode() panicked/threw C++ exception during injection at pos {token_pos}"
                );
                log_event(conversation_id, "inject_error", &format!("decode panicked at token {i}/pos {token_pos}"));
                return Err("Decode crashed during tool injection (C++ exception)".to_string());
            }
        }

        *token_pos += 1;
    }

    if *token_pos as u32 >= ctx_size.saturating_sub(ctx_size / 20) {
        eprintln!(
            "[INJECT] Context 95% full after injection ({token_pos}/{ctx_size})"
        );
        log_event(conversation_id, "inject_error", "CONTEXT_EXHAUSTED after injection (95% full)");
        return Err("CONTEXT_EXHAUSTED".to_string());
    }

    log_event(
        conversation_id,
        "inject_done",
        &format!("Injected {} tokens, new pos {}", total, token_pos),
    );
    Ok(())
}
