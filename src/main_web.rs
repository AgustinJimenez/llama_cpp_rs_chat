// Simple web server version of LLaMA Chat (without Tauri)
#[allow(unused_imports)]
#[macro_use]
extern crate llama_chat_types;

mod server;
mod vlm_ocr;
mod web; // Declare web module for model capabilities and utilities

// Import all types and functions from web modules
use web::database::SharedDatabase;

use std::convert::Infallible;

#[cfg(not(feature = "mock"))]
use web::worker_pool::WorkerPool;

// HTTP server using hyper
use hyper::{Body, Request, Response};

// Note: All struct definitions (SamplerConfig, TokenData, ChatRequest, ChatResponse, etc.)
// and helper functions (load_config, add_to_model_history, get_model_status, etc.)
// are now imported from web modules (web::config, web::command, web::model_manager, etc.)

// All helper functions and struct definitions are imported from web modules

#[cfg(not(feature = "mock"))]
async fn handle_request(
    req: Request<Body>,
    worker_pool: WorkerPool,
    db: SharedDatabase,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, Some(worker_pool), db).await
}

#[cfg(feature = "mock")]
async fn handle_request(
    req: Request<Body>,
    db: SharedDatabase,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, None, db).await
}

async fn handle_request_impl(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] worker_pool: Option<WorkerPool>,
    #[cfg(feature = "mock")] _worker_bridge: Option<()>,
    db: SharedDatabase,
) -> std::result::Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        web::http_dispatch::dispatch(req, worker_pool, db).await
    }
    #[cfg(feature = "mock")]
    {
        web::http_dispatch::dispatch(req, _worker_bridge, db).await
    }
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // VLM OCR subprocess mode
    if args.iter().any(|a| a == "--vlm-ocr") {
        return vlm_ocr::vlm_ocr_main(&args);
    }

    // Check for --worker flag BEFORE creating tokio runtime.
    if args.iter().any(|a| a == "--worker") {
        let db_path = args
            .windows(2)
            .find(|w| w[0] == "--db-path")
            .map(|w| w[1].as_str())
            .unwrap_or("assets/llama_chat.db")
            .to_string();

        // The agent's native browser (wry/tao) is driven from inside generation, which
        // runs in THIS worker process. On macOS that WebView needs the event loop on the
        // MAIN thread (AppKit), so run the worker on a background thread and hand the main
        // thread to the WebView loop. Other platforms run the worker on the main thread —
        // their wry loop spawns its own thread (allowed on Windows/Linux).
        #[cfg(target_os = "macos")]
        {
            std::thread::spawn(move || {
                web::worker::worker_main::run_worker(&db_path);
                std::process::exit(0);
            });
            llama_chat_tools::wry_browser::serve_browser_on_main_thread()
        }
        #[cfg(not(target_os = "macos"))]
        {
            web::worker::worker_main::run_worker(&db_path);
            return Ok(());
        }
    }

    // Create tokio runtime for the server
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(server::server_main())
}
