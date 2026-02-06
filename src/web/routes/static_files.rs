// Static file serving route handlers

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;
use tokio::fs;

use crate::web::response_helpers::cors_preflight;

pub async fn handle_index(
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Serve the main index.html from the built frontend
    match fs::read_to_string("./dist/index.html").await {
        Ok(content) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html")
            .body(Body::from(content))
            .unwrap()),
        Err(_) => {
            // Fallback HTML if dist files aren't found
            let html = r#"<!DOCTYPE html>
<html>
<head><title>LLaMA Chat Web</title></head>
<body>
<h1>🦙 LLaMA Chat Web Server</h1>
<p>Web server is running successfully!</p>
<p>Frontend files not found. API endpoints:</p>
<ul>
<li>GET /health - Health check</li>
<li>POST /api/chat - Chat endpoint</li>
<li>GET /api/config - Configuration</li>
</ul>
</body>
</html>"#;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/html")
                .body(Body::from(html))
                .unwrap())
        }
    }
}

pub async fn handle_static_asset(
    path: &str,
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Serve static assets (JS, CSS, etc.)
    let file_path = format!("./dist{}", path);
    match fs::read(&file_path).await {
        Ok(content) => {
            let content_type = if path.ends_with(".js") {
                "application/javascript"
            } else if path.ends_with(".css") {
                "text/css"
            } else if path.ends_with(".png") {
                "image/png"
            } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
                "image/jpeg"
            } else if path.ends_with(".svg") {
                "image/svg+xml"
            } else if path.ends_with(".json") {
                "application/json"
            } else if path.ends_with(".wasm") {
                "application/wasm"
            } else if path.ends_with(".html") || path.ends_with(".htm") {
                "text/html"
            } else if path.ends_with(".txt") {
                "text/plain"
            } else {
                "application/octet-stream"
            };

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", content_type)
                .header("cache-control", "public, max-age=31536000") // 1 year cache
                .body(Body::from(content))
                .unwrap())
        }
        Err(_) => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Asset not found"))
            .unwrap()),
    }
}

pub async fn handle_options(
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    Ok(cors_preflight())
}
