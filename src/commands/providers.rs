// ─── System Usage & Provider Commands ────────────────────────────────

use tauri::{AppHandle, Emitter};

use crate::event_payloads::{ProviderDoneEvent, ProviderTokenEvent};
use crate::web::database::SharedDatabase;

// ─── System Usage ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_system_usage() -> Result<serde_json::Value, String> {
    #[cfg(target_os = "windows")]
    let (cpu, ram, gpu, cpu_perf_pct, _app_ram_gb, vram_used_gb) = {
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(1500),
            tokio::task::spawn_blocking(crate::web::routes::system::get_windows_system_usage),
        )
        .await;
        match result {
            Ok(Ok(values)) => values,
            _ => crate::web::routes::system::get_cached_windows_system_usage(),
        }
    };

    #[cfg(not(target_os = "windows"))]
    let (cpu, ram, gpu, cpu_perf_pct, _app_ram_gb, vram_used_gb) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);

    // Hardware totals — populated by get_windows_system_usage on its first call.
    // Without these the frontend can't tell GPU-equipped machines from CPU-only ones,
    // which is critical so the VRAM optimizer doesn't try to load layers on a non-existent GPU.
    #[cfg(target_os = "windows")]
    let (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz) =
        crate::web::routes::system::get_hardware_totals();
    #[cfg(not(target_os = "windows"))]
    let (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz) =
        (0.0_f32, 0.0_f32, 0_u32, 0_u32);

    let cpu_ghz = (cpu_base_mhz as f32) * cpu_perf_pct / 100.0 / 1000.0;

    Ok(serde_json::json!({
        "cpu": cpu,
        "gpu": gpu,
        "ram": ram,
        "total_ram_gb": total_ram_gb,
        "total_vram_gb": total_vram_gb,
        "cpu_cores": cpu_cores,
        "cpu_ghz": cpu_ghz,
        "vram_used_gb": vram_used_gb,
    }))
}

// ─── Provider helpers ─────────────────────────────────────────────────

pub fn load_provider_api_keys_json(db: &SharedDatabase) -> Option<String> {
    let conn = db.connection();
    conn.query_row(
        "SELECT provider_api_keys FROM config WHERE id = 1",
        [],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

#[tauri::command]
pub async fn list_providers(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let api_keys_json = load_provider_api_keys_json(&db);
    let providers = crate::web::providers::list_providers_with_keys(api_keys_json.as_deref()).await;
    Ok(serde_json::json!({ "providers": providers }))
}

#[tauri::command]
pub async fn list_configured_providers(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let api_keys_json = load_provider_api_keys_json(&db);
    let providers =
        crate::web::providers::list_configured_providers_with_keys(api_keys_json.as_deref());
    Ok(serde_json::json!({ "providers": providers }))
}

#[tauri::command]
pub async fn list_cli_providers() -> Result<serde_json::Value, String> {
    let providers = crate::web::providers::list_cli_providers().await;
    Ok(serde_json::json!({ "providers": providers }))
}

// ─── Cloud provider streaming (Tauri variant — saves to Tauri DB) ─────

/// Finalize a provider conversation — messages are already saved incrementally
/// by the agentic loop in openai_compat.rs. This just updates metadata.
fn save_provider_turn_tauri(
    db: &SharedDatabase,
    conv_id: &str,
    provider_id: &str,
    provider_session_id: Option<&str>,
    _prompt: &str,
    _full_response: &str,
    now: u64,
) {
    let _ = db.set_conversation_provider_session_id(conv_id, Some(provider_id), provider_session_id);
    {
        let conn = db.connection();
        let _ = conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now as i64, conv_id],
        );
    }
}

#[tauri::command]
pub async fn stream_provider(
    app: AppHandle,
    db: tauri::State<'_, SharedDatabase>,
    provider: String,
    model: Option<String>,
    prompt: String,
    conversation_id: Option<String>,
    session_id: Option<String>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let api_keys = load_provider_api_keys_json(&db);

    let conv_id = conversation_id.unwrap_or_else(|| {
        format!("chat_{}", chrono::Local::now().format("%Y-%m-%d-%H-%M-%S-%3f"))
    });

    let provider_prompt = crate::web::providers::compose_prompt(
        &provider,
        &prompt,
        session_id.as_deref(),
    );

    let mut rx = crate::web::providers::generate(
        &provider,
        &provider_prompt,
        model.as_deref(),
        Some(50),
        None,
        session_id.as_deref(),
        api_keys.as_deref(),
        Some(&conv_id),
        Some(db.inner()),
        params.as_ref(),
    )
    .await
    .map_err(|e| format!("Failed to start provider: {e}"))?;

    let db_clone = db.inner().clone();
    let conv_id_clone = conv_id.clone();
    let prompt_clone = prompt.clone();
    let provider_clone = provider.clone();

    tokio::spawn(async move {
        let mut full_response = String::new();
        let mut tokens_since_save = 0u32;
        let mut last_save = std::time::Instant::now();
        let msg_id = uuid::Uuid::new_v4().to_string();

        // User message is now saved incrementally by openai_compat.rs agentic loop

        while let Some(token_data) = rx.recv().await {
            if token_data.is_done {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                save_provider_turn_tauri(
                    &db_clone,
                    &conv_id_clone,
                    &provider_clone,
                    token_data.session_id.as_deref(),
                    &prompt_clone,
                    &full_response,
                    now,
                );

                // Clear streaming buffer after successful save
                let _ = db_clone.connection().execute(
                    "DELETE FROM streaming_buffer WHERE conversation_id = ?1",
                    rusqlite::params![&conv_id_clone],
                );

                let _ = app.emit("provider-done", ProviderDoneEvent {
                    conversation_id: conv_id_clone.clone(),
                    session_id: token_data.session_id,
                    stop_reason: token_data.stop_reason,
                    cost_usd: token_data.cost_usd,
                    duration_ms: token_data.duration_ms,
                    input_tokens: token_data.input_tokens,
                    output_tokens: token_data.output_tokens,
                    model: token_data.model_id,
                });

                let _ = app.emit("conversation-title-updated", serde_json::json!({
                    "conversation_id": &conv_id_clone,
                }));
                // Emit again after a delay — title is generated asynchronously
                // in openai_compat after the done event
                let app2 = app.clone();
                let cid2 = conv_id_clone.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let _ = app2.emit("conversation-title-updated", serde_json::json!({
                        "conversation_id": &cid2,
                    }));
                });
                break;
            }

            if token_data.token.is_empty() {
                continue;
            }

            full_response.push_str(&token_data.token);
            tokens_since_save += 1;

            // Incrementally save to streaming_buffer every 50 tokens or 5 seconds
            // so partial responses survive crashes
            if tokens_since_save >= 50 || last_save.elapsed().as_secs() >= 5 {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let _ = db_clone.connection().execute(
                    "INSERT OR REPLACE INTO streaming_buffer (conversation_id, message_id, partial_content, tokens_used, max_tokens, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 0, ?5)",
                    rusqlite::params![&conv_id_clone, &msg_id, &full_response, tokens_since_save as i32, now_ms],
                );
                tokens_since_save = 0;
                last_save = std::time::Instant::now();
            }

            let _ = app.emit("provider-token", ProviderTokenEvent {
                token: token_data.token,
            });
        }
    });

    Ok(serde_json::json!({ "conversation_id": conv_id }))
}

#[tauri::command]
pub async fn queue_message(
    db: tauri::State<'_, SharedDatabase>,
    conversation_id: String,
    content: String,
) -> Result<serde_json::Value, String> {
    db.queue_message(&conversation_id, &content)?;
    Ok(serde_json::json!({"success": true}))
}
