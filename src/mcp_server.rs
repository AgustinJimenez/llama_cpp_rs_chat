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
    serial: Arc<tokio::sync::Semaphore>,
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
            let permit = self
                .serial
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| rmcp::ErrorData::internal_error("Desktop executor unavailable", None))?;
            let tool_name = name.clone();
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(30);
            let cancel_ctx = web::desktop_tools::DesktopCancellationContext::with_timeout(timeout);
            eprintln!("[MCP] call_tool: {tool_name} — starting");

            // Extract image compression overrides before args is moved into the blocking closure.
            let img_format_owned = args.get("screenshot_format").and_then(|v| v.as_str()).map(|s| s.to_owned());
            let img_quality = args.get("screenshot_quality").and_then(|v| v.as_u64()).map(|v| v as u32);
            let img_max_width = args.get("screenshot_max_width").and_then(|v| v.as_u64()).map(|v| v as u32);

            let join = tokio::task::spawn_blocking({
                let cancel_ctx = cancel_ctx.clone();
                move || {
                    let _permit = permit;
                    web::desktop_tools::with_desktop_cancellation_context(cancel_ctx, || {
                        web::desktop_tools::dispatch_desktop_tool(&name, &args)
                    })
                }
            });

            let timed = tokio::select! {
                join_result = join => Ok(join_result),
                _ = tokio::time::sleep(timeout) => {
                    cancel_ctx.cancel();
                    Err(())
                }
            };

            let elapsed = start.elapsed();

            let result = match timed {
                Ok(join_result) => join_result.map_err(|e| {
                    eprintln!("[MCP] call_tool: {tool_name} — join error after {elapsed:.1?}");
                    rmcp::ErrorData::internal_error(format!("Task join error: {e}"), None)
                })?,
                Err(()) => {
                    eprintln!("[MCP] call_tool: {tool_name} — TIMEOUT after {elapsed:.1?}");
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Tool '{tool_name}' timed out after 30 seconds and cancellation was requested"
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
                    // Callers can override format/quality/max_width via tool args.
                    use base64::Engine;
                    for png_bytes in &native_result.images {
                        let compressed = compress_image_for_mcp(
                            png_bytes,
                            img_format_owned.as_deref(),
                            img_quality,
                            img_max_width,
                        );
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

/// Downscale + compress a PNG screenshot for efficient MCP transport.
/// Reduces ~2MB PNG → ~100-200KB JPEG at 1280px wide (default).
/// Supports configurable format (jpeg/png), quality, and max width.
fn compress_image_for_mcp(
    png_bytes: &[u8],
    format: Option<&str>,
    quality: Option<u32>,
    max_width: Option<u32>,
) -> CompressedImage {
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

    let out_format = format.unwrap_or("jpeg");
    let jpeg_quality = quality.unwrap_or(75).clamp(1, 100);
    let max_w = max_width.unwrap_or(1280).clamp(320, 3840);

    // Downscale: max width, preserve aspect ratio
    let (w, h) = img.dimensions();
    let resized = if w > max_w {
        img.resize(max_w, max_w * h / w, image::imageops::FilterType::Triangle)
    } else {
        img
    };

    if out_format == "png" {
        // Encode as PNG (lossless but still downscaled)
        let mut png_buf = Vec::with_capacity(512 * 1024);
        let mut cursor = std::io::Cursor::new(&mut png_buf);
        let encoder = image::codecs::png::PngEncoder::new(&mut cursor);
        match encoder.write_image(
            resized.to_rgba8().as_raw(),
            resized.width(),
            resized.height(),
            image::ColorType::Rgba8.into(),
        ) {
            Ok(()) => {
                eprintln!(
                    "[MCP] image: {}x{} PNG {}KB → {}x{} PNG {}KB ({:.0}% reduction)",
                    w, h, png_bytes.len() / 1024,
                    resized.width(), resized.height(), png_buf.len() / 1024,
                    (1.0 - png_buf.len() as f64 / png_bytes.len() as f64) * 100.0
                );
                CompressedImage {
                    data: png_buf,
                    mime: "image/png".to_string(),
                }
            }
            Err(_) => CompressedImage {
                data: png_bytes.to_vec(),
                mime: "image/png".to_string(),
            },
        }
    } else {
        // Encode as JPEG (default)
        let mut jpeg_buf = Vec::with_capacity(256 * 1024);
        let mut cursor = std::io::Cursor::new(&mut jpeg_buf);
        let encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, jpeg_quality as u8);
        match encoder.write_image(
            resized.to_rgb8().as_raw(),
            resized.width(),
            resized.height(),
            image::ColorType::Rgb8.into(),
        ) {
            Ok(()) => {
                eprintln!(
                    "[MCP] image: {}x{} PNG {}KB → {}x{} JPEG q{} {}KB ({:.0}% reduction)",
                    w, h, png_bytes.len() / 1024,
                    resized.width(), resized.height(), jpeg_quality,
                    jpeg_buf.len() / 1024,
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
    let server = DesktopToolsServer {
        tools,
        serial: Arc::new(tokio::sync::Semaphore::new(1)),
    };
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

    let mut config = StreamableHttpServerConfig::default();
    config.stateful_mode = true;
    config.sse_keep_alive = Some(std::time::Duration::from_secs(15));
    config.sse_retry = Some(std::time::Duration::from_secs(3));
    config.json_response = false;
    config.cancellation_token = ct.child_token();

    let session_manager = Arc::new(LocalSessionManager::default());

    // Factory creates a fresh DesktopToolsServer per session
    let service = StreamableHttpService::new(
        || {
            Ok(DesktopToolsServer {
                tools: build_tool_definitions(),
                serial: Arc::new(tokio::sync::Semaphore::new(1)),
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
