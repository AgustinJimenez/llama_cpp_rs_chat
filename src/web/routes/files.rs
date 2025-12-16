// File operation route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::fs;

use crate::web::{
    models::{FileItem, BrowseFilesResponse},
    response_helpers::{json_error, json_response, json_raw},
};

pub async fn handle_get_browse(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Parse query parameters for path
    let query = req.uri().query().unwrap_or("");
    let mut browse_path = "/app/models"; // Default path

    // Simple query parameter parsing
    for param in query.split('&') {
        if let Some((key, value)) = param.split_once('=') {
            if key == "path" {
                // Simple path assignment (assume already decoded by browser)
                browse_path = value;
            }
        }
    }

    // Security: ensure path is within allowed directories
    let allowed_paths = ["/app/models", "/app"];
    let is_allowed = allowed_paths.iter().any(|&allowed| {
        browse_path.starts_with(allowed)
    });

    if !is_allowed {
        return Ok(json_error(StatusCode::FORBIDDEN, "Path not allowed"));
    }

    let mut files = Vec::new();
    let current_path = browse_path.to_string();
    let parent_path = if browse_path != "/app/models" && browse_path != "/app" {
        std::path::Path::new(browse_path)
            .parent()
            .and_then(|p| p.to_str())
            .map(|s| s.to_string())
    } else {
        None
    };

    match fs::read_dir(browse_path) {
        Ok(entries) => {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if let (Some(name), Some(path_str)) = (
                        path.file_name().and_then(|n| n.to_str()),
                        path.to_str()
                    ) {
                        let is_directory = path.is_dir();
                        let size = if !is_directory {
                            entry.metadata().ok().map(|m| m.len())
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
            }

            // Sort: directories first, then files, both alphabetically
            files.sort_by(|a, b| {
                match (a.is_directory, b.is_directory) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                }
            });
        }
        Err(e) => {
            eprintln!("Failed to read directory {}: {}", browse_path, e);
            return Ok(json_error(StatusCode::NOT_FOUND, "Directory not found"));
        }
    }

    let response = BrowseFilesResponse {
        files,
        current_path,
        parent_path,
    };

    Ok(json_response(StatusCode::OK, &response))
}

pub async fn handle_post_upload(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Extract headers before consuming the request body
    let content_disposition = req.headers().get("content-disposition")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    let query = req.uri().query().unwrap_or("").to_string();

    // Handle file upload
    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read request body"));
        }
    };

    let filename = if content_disposition.contains("filename=") {
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

    // Ensure the filename ends with .gguf
    let filename = if filename.ends_with(".gguf") {
        filename.to_string()
    } else {
        format!("{}.gguf", filename)
    };

    // Save file to models directory
    let file_path = format!("/app/models/{}", filename);
    match fs::write(&file_path, &body_bytes) {
        Ok(_) => {
            let response = serde_json::json!({
                "success": true,
                "message": "File uploaded successfully",
                "file_path": file_path
            });
            Ok(json_raw(StatusCode::OK, response.to_string()))
        }
        Err(e) => {
            Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed to save file: {}", e)))
        }
    }
}
