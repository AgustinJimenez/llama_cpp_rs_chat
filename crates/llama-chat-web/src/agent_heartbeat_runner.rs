// Background heartbeat runner.
// Spawned once at server start. Polls all conversations with heartbeat enabled
// and fires a generation turn into each one that is past its interval.

use llama_chat_db::{
    agent_heartbeat::{
        list_enabled_heartbeats, read_heartbeat_config, record_heartbeat_result,
        DEFAULT_HEARTBEAT_PROMPT,
    },
    SharedDatabase,
};
use crate::worker_pool::{lookup_worker_id_for_conversation, WorkerPool};
use llama_chat_worker::worker::worker_bridge::{GenerationResult, SharedWorkerBridge};

const POLL_SECS: u64 = 30; // How often to check if any conversation is due

/// Runs forever in a background tokio task. Call once after the worker is up.
pub async fn run(pool: WorkerPool, db: SharedDatabase) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(POLL_SECS)).await;

        let now = llama_chat_db::current_timestamp_secs();
        let enabled = list_enabled_heartbeats(&db);

        for (conv_id, cfg) in enabled {
            let interval_secs = cfg.interval_minutes as u64 * 60;
            if now.saturating_sub(cfg.last_fired_at) < interval_secs {
                continue;
            }

            let worker_id = lookup_worker_id_for_conversation(&db, &conv_id);
            let Some(bridge) = pool.get_or_default(worker_id.as_deref()) else {
                continue;
            };

            if bridge.is_generating().await {
                continue;
            }

            if bridge.model_status().await.map(|m| !m.loaded).unwrap_or(true) {
                continue;
            }

            fire_one(bridge.clone(), db.clone(), conv_id).await;
            // Fire at most one conversation per tick to avoid back-to-back model calls
            break;
        }
    }
}

/// Fire a single heartbeat turn into the given conversation.
/// Used by both the background loop and the manual-fire endpoint.
pub async fn fire_one(bridge: SharedWorkerBridge, db: SharedDatabase, conversation_id: String) {
    let cfg = read_heartbeat_config(&db, &conversation_id);
    let prompt = if cfg.prompt.trim().is_empty() {
        DEFAULT_HEARTBEAT_PROMPT.to_string()
    } else {
        cfg.prompt.clone()
    };

    let (mut token_rx, done_rx) = match bridge
        .generate(prompt, Some(conversation_id.clone()), true, None)
        .await
    {
        Ok(rx) => rx,
        Err(e) => {
            sys_warn!("[HEARTBEAT] generate() failed: {e}");
            return;
        }
    };

    // Collect full response
    let mut response = String::new();
    while let Some(token_data) = token_rx.recv().await {
        response.push_str(&token_data.token);
    }

    match done_rx.await {
        Ok(GenerationResult::Complete { .. }) => {}
        Ok(GenerationResult::Cancelled) => {
            sys_info!("[HEARTBEAT] Cancelled");
            return;
        }
        Ok(GenerationResult::Error(e)) => {
            sys_warn!("[HEARTBEAT] Error: {e}");
            return;
        }
        Err(e) => {
            sys_warn!("[HEARTBEAT] Result channel error: {e}");
            return;
        }
    }

    let trimmed = response.trim();
    let is_idle = trimmed.eq_ignore_ascii_case("idle") || trimmed.is_empty();
    let result = if is_idle { None } else { Some(trimmed) };

    sys_info!(
        "[HEARTBEAT] Fired → conv={} result={}",
        conversation_id,
        if is_idle { "IDLE" } else { "NOTIFIED" }
    );

    let _ = record_heartbeat_result(&db, &conversation_id, result);
}
