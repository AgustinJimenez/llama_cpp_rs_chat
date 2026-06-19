// Server startup, single-instance enforcement, and main server loop.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use hyper::service::{make_service_fn, service_fn};
use hyper::Server;

use crate::web::database::{Database, SharedDatabase};
use crate::web::worker_pool::WorkerPool;

#[cfg(not(feature = "mock"))]
use crate::web::worker::process_manager::ProcessManager;
#[cfg(not(feature = "mock"))]
use crate::web::worker::worker_bridge::{SharedWorkerBridge, WorkerBridge};


/// Write our PID to `assets/server.pid`. On startup, if a PID file already exists and that
/// process is still alive, kill it first so only one server instance runs at a time.
pub fn enforce_single_instance() {
    const PID_FILE: &str = "assets/server.pid";

    // If a stale PID file exists, try to kill the old process.
    if let Ok(contents) = std::fs::read_to_string(PID_FILE) {
        if let Ok(old_pid) = contents.trim().parse::<u32>() {
            // On Windows, use taskkill; on Unix, send SIGTERM.
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(["/T", "/F", "/PID", &old_pid.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = std::process::Command::new("kill")
                    .args(["-TERM", &old_pid.to_string()])
                    .status();
            }
            // Give it a moment to release the port.
            std::thread::sleep(std::time::Duration::from_millis(500));
            eprintln!("[SERVER] Killed previous instance (PID {old_pid})");
        }
    }

    // Write our own PID.
    let my_pid = std::process::id();
    let _ = std::fs::write(PID_FILE, my_pid.to_string());

    // Remove PID file on exit via a dedicated thread watching for process death.
    // (Simple: just register a normal exit hook via std::panic + atexit isn't easy in Rust,
    //  so we rely on the OS to reclaim the file on next startup instead.)
}

pub async fn server_main() -> std::io::Result<()> {
    enforce_single_instance();

    // Initialize SQLite database
    let db: SharedDatabase = Arc::new(
        Database::new("assets/llama_chat.db").expect("Failed to initialize SQLite database"),
    );
    println!("📦 SQLite database initialized at assets/llama_chat.db");

    // Initialize background process tracking so remote provider tool calls can register processes
    let bg_session_id = format!("web_{}", std::process::id());
    llama_chat_command::background::init_background_tracking(db.clone(), bg_session_id);



    // Apply file logging setting from config
    {
        let config = db.load_config();
        crate::web::logger::LOGGER.set_enabled(!config.disable_file_logging);
        if config.disable_file_logging {
            println!("📝 File logging disabled (enable in settings)");
        }
    }

    // Spawn worker process
    #[cfg(not(feature = "mock"))]
    let worker_bridge: SharedWorkerBridge = {
        let pm = Arc::new(
            ProcessManager::spawn("assets/llama_chat.db")
                .expect("Failed to spawn worker process"),
        );
        Arc::new(WorkerBridge::new(pm, db.clone()))
    };
    let worker_pool = WorkerPool::new(worker_bridge.clone(), "assets/llama_chat.db", db.clone());

    // Spawn the agent heartbeat background task
    #[cfg(not(feature = "mock"))]
    {
        let hb_pool = worker_pool.clone();
        let hb_db = db.clone();
        tokio::spawn(async move {
            crate::web::agent_heartbeat_runner::run(hb_pool, hb_db).await;
        });
    }

    // Create HTTP service
    let make_svc = make_service_fn({
        #[cfg(not(feature = "mock"))]
        let worker_pool = worker_pool.clone();
        let db = db.clone();

        move |_conn| {
            #[cfg(not(feature = "mock"))]
            let worker_pool = worker_pool.clone();
            let db = db.clone();

            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let db = db.clone();
                    #[cfg(not(feature = "mock"))]
                    {
                        super::handle_request(req, worker_pool.clone(), db)
                    }
                    #[cfg(feature = "mock")]
                    {
                        super::handle_request(req, db)
                    }
                }))
            }
        }
    });

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 18080));
    let server = Server::bind(&addr).serve(make_svc);

    println!("🦙 LLaMA Chat Web Server starting on http://{addr}");
    println!("📡 Worker pool initialized with default worker for model inference");
    println!("Available endpoints:");
    println!("  GET  /health               - Health check");
    println!("  POST /api/chat             - Chat with LLaMA");
    println!("  GET  /api/config           - Get sampler configuration");
    println!("  POST /api/config           - Update sampler configuration");
    println!("  GET  /api/model/status     - Get current model status");
    println!("  GET  /api/model/history    - Get model path history");
    println!("  POST /api/model/history    - Add model path to history");
    println!("  POST /api/model/load       - Load a specific model");
    println!("  POST /api/model/unload     - Unload current model");
    println!("  POST /api/model/hard-unload - Force kill worker (reclaim all memory)");
    println!("  POST /api/upload           - Upload model file");
    println!("  GET  /api/conversations    - List conversation files");
    println!("  POST /api/tools/execute    - Execute tool calls");
    println!("  GET  /api/tools/web-fetch  - Fetch web page as text");
    println!("  GET  /api/browse           - Browse model files");
    println!("  GET  /                     - Web interface");

    server
        .await
        .map_err(std::io::Error::other)?;

    Ok(())
}
