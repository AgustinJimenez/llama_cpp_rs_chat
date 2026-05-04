// Web server modules for LLaMA Chat
// Most code lives in workspace crates — these modules re-export from them.
// Only modules with unique code have actual .rs files here.

// ── Modules with unique code (keep .rs files) ───────────────────────
pub mod browser; // Headless Chrome (has make_dispatch_context)
pub mod native_tools; // Has unique make_dispatch_context()
pub mod utils; // Has unique silent_command(), get_available_tools_json()

// ── Directory modules (keep mod.rs with per-module re-exports) ──────
pub mod routes;
pub mod providers;

// ── Inline re-exports from workspace crates ─────────────────────────
#[allow(unused_imports)]
pub mod chat { pub use llama_chat_engine::*; }
#[allow(unused_imports)]
pub mod background { pub use llama_chat_command::background::*; }
#[allow(unused_imports)]
pub mod command { pub(crate) use llama_chat_command::*; }
#[allow(unused_imports)]
pub mod config {
    pub use llama_chat_config::*;
    pub use llama_chat_config::load_config;
}
#[allow(unused_imports)]
pub mod database { pub use llama_chat_db::*; }
#[allow(unused_imports)]
pub mod filename_patterns { pub use llama_chat_engine::filename_patterns::*; }
#[allow(unused_imports)]
pub mod gguf_info { pub use llama_chat_engine::gguf_info::*; }
#[allow(unused_imports)]
pub mod gguf_utils { pub use llama_chat_engine::gguf_utils::*; }
#[allow(unused_imports)]
pub mod logger { pub use llama_chat_types::logger::*; }
#[allow(unused_imports)]
pub mod model_manager { pub use llama_chat_engine::model_manager::*; }
#[allow(unused_imports)]
pub mod models {
    pub use llama_chat_types::models::*;
    pub use llama_chat_engine::SharedConversationLogger;
}
#[allow(dead_code, unused_imports)]
pub mod mcp {
    pub use llama_chat_worker::mcp::*;
    pub mod client { pub use llama_chat_worker::mcp::client::*; }
    pub mod config { pub use llama_chat_types::mcp_config::*; }
    pub mod manager { pub use llama_chat_worker::mcp::manager::*; }
    pub mod tool_registry { pub use llama_chat_worker::mcp::tool_registry::*; }
}
#[allow(unused_imports)]
pub mod desktop_tools { pub use llama_chat_desktop_tools::*; }
#[allow(unused_imports)]
pub mod event_log { pub use llama_chat_db::event_log::*; }
#[allow(unused_imports)]
pub mod prevent_sleep { pub use llama_chat_worker::prevent_sleep::*; }
#[allow(unused_imports)]
pub mod request { pub use llama_chat_web::request::*; }
#[allow(unused_imports)]
pub mod request_parsing { pub use llama_chat_web::request_parsing::*; }
#[allow(unused_imports)]
pub mod response_helpers { pub use llama_chat_web::response_helpers::*; }
#[allow(unused_imports)]
pub mod skills { pub use llama_chat_web::skills::*; }
#[allow(unused_imports)]
pub mod vram_calculator { pub use llama_chat_engine::vram_calculator::*; }
#[allow(unused_imports)]
pub mod websocket { pub use llama_chat_web::websocket::*; }
#[allow(unused_imports)]
pub mod websocket_utils { pub use llama_chat_web::websocket_utils::*; }
#[allow(unused_imports)]
pub mod worker {
    pub use llama_chat_worker::*;
    pub mod process_manager { pub use llama_chat_worker::ProcessManager; }
    pub mod worker_bridge { pub use llama_chat_worker::worker::worker_bridge::*; }
    pub mod worker_main { pub use llama_chat_worker::run_worker; }
    pub mod ipc_types { pub use llama_chat_worker::ipc_types::*; }
}
