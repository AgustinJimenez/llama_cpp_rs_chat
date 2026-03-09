//! MCP (Model Context Protocol) server binary that exposes desktop automation tools.
//!
//! Two transport modes:
//!   stdio (default): `claude mcp add desktop-tools -- ./target/release/mcp_desktop_tools.exe`
//!   HTTP/SSE:        `mcp_desktop_tools.exe --http 18090`
//!                    then: `claude mcp add desktop-tools --transport http --url http://localhost:18090/mcp`
//!
//! Gives Claude Code access to ~70 desktop automation tools: mouse, keyboard, screenshot,
//! OCR, UI automation, window management, clipboard, process control, and more.

#![allow(dead_code, unused_imports)]

mod web;

use std::sync::Arc;

use rmcp::{
    ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, Content, Implementation, ListToolsResult,
        ServerCapabilities, ServerInfo, Tool,
    },
    serve_server,
    service::RequestContext,
    service::RoleServer,
    transport::io::stdio,
};
use serde_json::Value;

struct DesktopToolsServer {
    tools: Vec<Tool>,
}

impl ServerHandler for DesktopToolsServer {
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .build();
        ServerInfo::new(capabilities)
            .with_server_info(Implementation::new(
                "desktop-tools",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Desktop automation tools for controlling the user's computer. \
                 Use take_screenshot to see the screen, then click_screen/type_text/press_key to interact. \
                 For multi-step desktop tasks, ALWAYS use show_status_overlay at the start with a step count \
                 (e.g. \"Step 1/4: Opening Blender...\"), update_status_overlay before each major step, \
                 and hide_status_overlay when done. Use position=\"bottom\" to avoid blocking app toolbars."
                    .to_owned(),
            )
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult {
            meta: None,
            tools: self.tools.clone(),
            next_cursor: None,
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_ {
        let name = request.name.to_string();
        let args = request
            .arguments
            .map(Value::Object)
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        async move {
            let tool_name = name.clone();
            let start = std::time::Instant::now();
            eprintln!("[MCP] call_tool: {tool_name} — starting");

            // Tool calls may block (screenshots, OCR, UI automation) — run on blocking thread
            // with a 30-second timeout to prevent the server from ever hanging.
            let timed = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                tokio::task::spawn_blocking(move || {
                    web::desktop_tools::dispatch_desktop_tool(&name, &args)
                }),
            )
            .await;

            let elapsed = start.elapsed();

            let result = match timed {
                Ok(join_result) => join_result.map_err(|e| {
                    eprintln!("[MCP] call_tool: {tool_name} — join error after {elapsed:.1?}");
                    rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None)
                })?,
                Err(_) => {
                    eprintln!("[MCP] call_tool: {tool_name} — TIMEOUT after {elapsed:.1?}");
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Tool '{tool_name}' timed out after 30 seconds"
                    ))]));
                }
            };

            eprintln!("[MCP] call_tool: {tool_name} — done in {elapsed:.1?}");

            match result {
                Some(native_result) => {
                    let mut content: Vec<Content> = Vec::new();

                    if !native_result.text.is_empty() {
                        content.push(Content::text(native_result.text));
                    }

                    // Convert images → downscaled JPEG for MCP transport efficiency.
                    // Raw PNGs are 1-3MB each; downscale + JPEG brings them to ~100-200KB.
                    use base64::Engine;
                    for png_bytes in &native_result.images {
                        let compressed = compress_image_for_mcp(png_bytes);
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed.data);
                        content.push(Content::image(b64, &compressed.mime));
                    }

                    if content.is_empty() {
                        content.push(Content::text("(no output)"));
                    }

                    Ok(CallToolResult::success(content))
                }
                None => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Unknown tool: '{tool_name}'"
                ))])),
            }
        }
    }
}

struct CompressedImage {
    data: Vec<u8>,
    mime: String,
}

/// Downscale + JPEG compress a PNG screenshot for efficient MCP transport.
/// Reduces ~2MB PNG → ~100-200KB JPEG at 1280px wide.
fn compress_image_for_mcp(png_bytes: &[u8]) -> CompressedImage {
    use image::{GenericImageView, ImageEncoder};

    let img = match image::load_from_memory(png_bytes) {
        Ok(img) => img,
        Err(_) => {
            // Can't decode — send raw PNG as fallback
            return CompressedImage {
                data: png_bytes.to_vec(),
                mime: "image/png".to_string(),
            };
        }
    };

    // Downscale: max 1280px wide, preserve aspect ratio
    let (w, h) = img.dimensions();
    let max_width = 1280u32;
    let resized = if w > max_width {
        img.resize(max_width, max_width * h / w, image::imageops::FilterType::Triangle)
    } else {
        img
    };

    // Encode as JPEG quality 75
    let mut jpeg_buf = Vec::with_capacity(256 * 1024);
    let mut cursor = std::io::Cursor::new(&mut jpeg_buf);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 75);
    match encoder.write_image(
        resized.to_rgb8().as_raw(),
        resized.width(),
        resized.height(),
        image::ColorType::Rgb8.into(),
    ) {
        Ok(()) => {
            eprintln!(
                "[MCP] image: {}x{} PNG {}KB → {}x{} JPEG {}KB ({:.0}% reduction)",
                w, h, png_bytes.len() / 1024,
                resized.width(), resized.height(), jpeg_buf.len() / 1024,
                (1.0 - jpeg_buf.len() as f64 / png_bytes.len() as f64) * 100.0
            );
            CompressedImage {
                data: jpeg_buf,
                mime: "image/jpeg".to_string(),
            }
        }
        Err(_) => CompressedImage {
            data: png_bytes.to_vec(),
            mime: "image/png".to_string(),
        },
    }
}

/// Convert tool definitions from our JSON schema format to rmcp Tool structs.
fn build_tool_definitions() -> Vec<Tool> {
    web::chat::jinja_templates::get_desktop_tool_definitions()
        .into_iter()
        .filter_map(|def| {
            let name = def.get("name")?.as_str()?.to_owned();
            let description = def.get("description")?.as_str()?.to_owned();
            let schema = match def.get("parameters") {
                Some(Value::Object(map)) => map.clone(),
                _ => serde_json::Map::new(),
            };
            Some(Tool::new(name, description, Arc::new(schema)))
        })
        .collect()
}

#[tokio::main]
async fn main() {
    let tools = build_tool_definitions();
    eprintln!(
        "MCP Desktop Tools server starting with {} tools",
        tools.len()
    );

    let args: Vec<String> = std::env::args().collect();

    // --http [port]  → HTTP/SSE transport (default port 18090)
    // (no args)      → stdio transport (default)
    if let Some(pos) = args.iter().position(|a| a == "--http") {
        let port: u16 = args
            .get(pos + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(18090);
        run_http(tools, port).await;
    } else {
        run_stdio(tools).await;
    }
}

async fn run_stdio(tools: Vec<Tool>) {
    let server = DesktopToolsServer { tools };
    let transport = stdio();

    match serve_server(server, transport).await {
        Ok(running) => {
            eprintln!("MCP server initialized (stdio), waiting for requests...");
            let _ = running.waiting().await;
        }
        Err(e) => {
            eprintln!("Failed to start MCP server: {e:?}");
            std::process::exit(1);
        }
    }
}

async fn run_http(_tools: Vec<Tool>, port: u16) {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
        session::local::LocalSessionManager,
    };
    use tokio_util::sync::CancellationToken;

    let ct = CancellationToken::new();

    let config = StreamableHttpServerConfig {
        stateful_mode: true,
        sse_keep_alive: Some(std::time::Duration::from_secs(15)),
        sse_retry: Some(std::time::Duration::from_secs(3)),
        json_response: false,
        cancellation_token: ct.child_token(),
    };

    let session_manager = Arc::new(LocalSessionManager::default());

    // Factory creates a fresh DesktopToolsServer per session
    let service = StreamableHttpService::new(
        || {
            Ok(DesktopToolsServer {
                tools: build_tool_definitions(),
            })
        },
        session_manager,
        config,
    );

    let router = axum::Router::new().nest_service("/mcp", service);

    let addr = format!("127.0.0.1:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {addr}: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("MCP server listening on http://{addr}/mcp (HTTP/SSE)");

    if let Err(e) = axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await
    {
        eprintln!("HTTP server error: {e}");
        std::process::exit(1);
    }
}
