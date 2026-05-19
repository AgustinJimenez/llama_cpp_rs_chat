// ─── File Browser ─────────────────────────────────────────────────────

use crate::web::models::{BrowseFilesResponse, FileItem};

#[tauri::command]
pub async fn browse_files(path: Option<String>) -> Result<BrowseFilesResponse, String> {
    let browse_path = path.unwrap_or_else(|| ".".into());
    let path_obj = std::path::Path::new(&browse_path);
    if !path_obj.exists() {
        return Err("Directory not found".into());
    }

    let mut files = Vec::new();
    let mut dir = tokio::fs::read_dir(&browse_path)
        .await
        .map_err(|e| format!("Failed to read directory: {e}"))?;

    while let Ok(Some(entry)) = dir.next_entry().await {
        let entry_path = entry.path();
        if let (Some(name), Some(path_str)) = (
            entry_path.file_name().and_then(|n| n.to_str()),
            entry_path.to_str(),
        ) {
            let is_directory = entry_path.is_dir();
            let size = if !is_directory {
                entry.metadata().await.ok().map(|m| m.len())
            } else {
                None
            };
            files.push(FileItem {
                name: name.to_string(),
                path: path_str.to_string(),
                is_directory,
                size,
            });
        }
    }

    files.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let parent_path = std::path::Path::new(&browse_path)
        .parent()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string());

    Ok(BrowseFilesResponse {
        files,
        current_path: browse_path,
        parent_path,
    })
}
