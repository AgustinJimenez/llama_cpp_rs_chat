// File operation route handlers

use futures_util::StreamExt;
use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::web::{
    models::{BrowseFilesResponse, FileItem},
    response_helpers::{json_error, json_raw, json_response},
};

// Import logging macros
use crate::sys_error;

pub async fn handle_get_browse(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Parse query parameters for path
    let query = req.uri().query().unwrap_or("");
    let mut browse_path_owned = String::from("/app/models"); // Default path

    // Simple query parameter parsing
    for param in query.split('&') {
        if let Some((key, value)) = param.split_once('=') {
            if key == "path" {
                // URL-decode the path (frontend sends encodeURIComponent)
                browse_path_owned = urlencoding::decode(value)
                    .unwrap_or(std::borrow::Cow::Borrowed(value))
                    .into_owned();
            }
        }
    }
    let browse_path = browse_path_owned.as_str();

    // Security: ensure path is within allowed directories
    // On native/Windows (drive letter like E:), allow any path since the app is local-only.
    // In Docker, restrict to /app paths.
    let allowed_paths = ["/app/models", "/app"];
    let is_native = browse_path.chars().nth(1) == Some(':');
    let is_allowed = is_native
        || allowed_paths
            .iter()
            .any(|&allowed| browse_path.starts_with(allowed));

    if !is_allowed {
        return Ok(json_error(StatusCode::FORBIDDEN, "Path not allowed"));
    }

    let mut files = Vec::new();
    let current_path = browse_path.to_string();
    // Show parent path unless we're at a root (drive root on Windows, /app on Docker)
    let is_root = browse_path == "/app/models"
        || browse_path == "/app"
        || (browse_path.len() <= 3 && browse_path.ends_with(':'))
        || (browse_path.len() <= 3 && browse_path.ends_with(":\\"));
    let parent_path = if !is_root {
        std::path::Path::new(browse_path)
            .parent()
            .and_then(|p| p.to_str())
            .map(|s| s.to_string())
    } else {
        None
    };

    let mut dir = match fs::read_dir(browse_path).await {
        Ok(d) => d,
        Err(e) => {
            sys_error!("Failed to read directory {}: {}", browse_path, e);
            return Ok(json_error(StatusCode::NOT_FOUND, "Directory not found"));
        }
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        let path = entry.path();
        if let (Some(name), Some(path_str)) =
            (path.file_name().and_then(|n| n.to_str()), path.to_str())
        {
            let is_directory = path.is_dir();
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

    // Sort: directories first, then files, both alphabetically
    files.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let response = BrowseFilesResponse {
        files,
        current_path,
        parent_path,
    };

    Ok(json_response(StatusCode::OK, &response))
}

/// POST /api/browse/pick-directory — open native OS folder picker, return selected path
pub async fn handle_post_pick_directory(
    #[cfg(not(feature = "mock"))] _bridge: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    let result = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new().pick_folder()
    })
    .await
    .unwrap_or(None);

    let path = result.map(|p| p.to_string_lossy().into_owned());
    let json = serde_json::json!({ "path": path });
    Ok(json_raw(StatusCode::OK, json.to_string()))
}

/// POST /api/browse/pick-file — open native OS file picker filtered to .gguf, return selected path
pub async fn handle_post_pick_file(
    #[cfg(not(feature = "mock"))] _bridge: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    let result = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter("GGUF Model Files", &["gguf"])
            .pick_file()
    })
    .await
    .unwrap_or(None);

    let path = result.map(|p| p.to_string_lossy().into_owned());
    let json = serde_json::json!({ "path": path });
    Ok(json_raw(StatusCode::OK, json.to_string()))
}

pub async fn handle_post_upload(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Extract headers before consuming the request body
    let content_disposition = req
        .headers()
        .get("content-disposition")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    let query = req.uri().query().unwrap_or("").to_string();

    // Handle file upload with a size guard and streaming write
    const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024; // 100MB
    let mut body = req.into_body();
    let mut total_bytes: usize = 0;

    let raw_filename = if content_disposition.contains("filename=") {
        content_disposition
            .split("filename=")
            .nth(1)
            .and_then(|s| s.split(';').next())
            .map(|s| s.trim_matches('"'))
            .unwrap_or("uploaded_model.gguf")
    } else {
        // Try to get filename from query parameter
        let mut filename = "uploaded_model.gguf";
        for param in query.split('&') {
            if let Some((key, value)) = param.split_once('=') {
                if key == "filename" {
                    filename = value;
                    break;
                }
            }
        }
        filename
    };

    // Sanitize filename to prevent path traversal
    let sanitized_name = Path::new(raw_filename)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("uploaded_model.gguf");

    // Ensure the filename ends with .gguf
    let filename = if sanitized_name.ends_with(".gguf") {
        sanitized_name.to_string()
    } else {
        format!("{sanitized_name}.gguf")
    };

    // Build destination path safely under /app/models
    let mut dest = PathBuf::from("/app/models");
    dest.push(&filename);

    // Save file to models directory (streaming)
    match fs::File::create(&dest).await {
        Ok(mut file) => {
            while let Some(chunk) = body.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(_) => {
                        return Ok(json_error(
                            StatusCode::BAD_REQUEST,
                            "Failed to read upload chunk",
                        ))
                    }
                };
                total_bytes = total_bytes.saturating_add(chunk.len());
                if total_bytes > MAX_UPLOAD_BYTES {
                    return Ok(json_error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Uploaded file too large",
                    ));
                }
                if let Err(e) = file.write_all(&chunk).await {
                    return Ok(json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &format!("Failed to save file: {e}"),
                    ));
                }
            }

            let response = serde_json::json!({
                "success": true,
                "message": "File uploaded successfully",
                "file_path": dest.to_string_lossy()
            });
            Ok(json_raw(StatusCode::OK, response.to_string()))
        }
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to save file: {e}"),
        )),
    }
}
