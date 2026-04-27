//! Configuration Tauri commands — get/save app settings.

use crate::web::database::SharedDatabase;
use crate::web::models::SamplerConfig;
use crate::web::config::*;

// ─── Configuration Commands ───────────────────────────────────────────

#[tauri::command]
pub async fn get_config(db: tauri::State<'_, SharedDatabase>) -> Result<SamplerConfig, String> {
    let db_config = db.load_config();
    Ok(db_config_to_sampler_config(&db_config))
}

#[tauri::command]
pub async fn save_config(
    config: SamplerConfig,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    if !(0.0..=2.0).contains(&config.temperature) {
        return Err("temperature must be between 0.0 and 2.0".into());
    }
    if !(0.0..=1.0).contains(&config.top_p) {
        return Err("top_p must be between 0.0 and 1.0".into());
    }
    if config.context_size.unwrap_or(0) == 0 {
        return Err("context_size must be positive".into());
    }

    let existing = db.load_config();
    let mut merged = sampler_config_to_db(&config);
    merged.model_history = existing.model_history;

    db.save_config(&merged)
        .map_err(|e| format!("Failed to save configuration: {e}"))?;

    Ok(serde_json::json!({"success": true}))
}
