//! HuggingFace Hub Tauri commands — model search, download, management.

use crate::web::database::SharedDatabase;
use tauri::{AppHandle, Emitter};

// ─── HuggingFace Hub ──────────────────────────────────────────────────

#[tauri::command]
pub async fn search_hub_models(
    query: String,
    limit: Option<usize>,
    sort: Option<String>,
) -> Result<Vec<crate::web::routes::hub::HubModel>, String> {
    let limit = limit.unwrap_or(20).min(50);
    let sort = sort.unwrap_or_else(|| "downloads".to_string());
    tokio::task::spawn_blocking(move || crate::web::routes::hub::search_hf(&query, limit, &sort))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
pub async fn fetch_hub_tree(model_id: String) -> Result<Vec<crate::web::routes::hub::HubFile>, String> {
    tokio::task::spawn_blocking(move || crate::web::routes::hub::tree_hf(&model_id))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
pub async fn verify_hub_downloads(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<Vec<crate::web::database::hub_downloads::HubDownloadRecord>, String> {
    let db_clone = db.inner().clone();
    tokio::task::spawn_blocking(move || crate::web::routes::download::verify_hub_downloads(&db_clone))
        .await
        .map_err(|e| format!("Task failed: {e}"))
}

#[tauri::command]
pub async fn delete_hub_download(
    id: i64,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<(), String> {
    let db_clone = db.inner().clone();
    tokio::task::spawn_blocking(move || crate::web::routes::download::delete_hub_download_by_id(&db_clone, id))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
pub async fn download_hub_model(
    app: AppHandle,
    model_id: String,
    filename: String,
    destination: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<(), String> {
    use std::path::{Path, PathBuf};

    // Sanitize filename — strip any path components
    let sanitized = Path::new(&filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download.gguf")
        .to_string();

    let dest_dir = PathBuf::from(&destination);
    if !dest_dir.is_dir() {
        return Err("Destination directory does not exist".to_string());
    }

    let dest_file = dest_dir.join(&sanitized);
    let part_file = dest_dir.join(format!("{sanitized}.part"));
    let key = format!("{model_id}/{filename}");

    // If the final file already exists, emit done immediately
    if dest_file.exists() {
        let size = std::fs::metadata(&dest_file).map(|m| m.len()).unwrap_or(0);
        let _ = app.emit(
            "hub-download-progress",
            serde_json::json!({
                "key": key,
                "type": "done",
                "path": dest_file.to_string_lossy(),
                "bytes": size,
            }),
        );
        return Ok(());
    }

    // Build HF download URL
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        model_id,
        urlencoding::encode(&filename),
    );

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<String>(64);
    let db_clone: SharedDatabase = db.inner().clone();

    // Clone values for the blocking thread
    let model_id_b = model_id.clone();
    let filename_b = filename.clone();
    let dest_path_str = destination.clone();

    // Blocking download thread
    tokio::task::spawn_blocking(move || {
        crate::web::routes::download::download_file_blocking(
            &url,
            &dest_file,
            &part_file,
            db_clone,
            &model_id_b,
            &filename_b,
            &dest_path_str,
            progress_tx,
        );
    });

    // Forward progress events as Tauri events — key is embedded so the
    // frontend can demux multiple concurrent downloads.
    let key_clone = key.clone();
    tokio::spawn(async move {
        while let Some(raw) = progress_rx.recv().await {
            let mut payload: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let serde_json::Value::Object(ref mut map) = payload {
                map.insert("key".to_string(), serde_json::Value::String(key_clone.clone()));
            }
            let _ = app.emit("hub-download-progress", payload);
        }
    });

    Ok(())
}

