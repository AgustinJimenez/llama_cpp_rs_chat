// Web server modules for LLaMA Chat

pub mod browser; // Headless Chrome for web search/fetch
pub mod chat;
pub mod background; // Background process tracking and lifecycle
pub mod command;
pub mod config;
pub mod database;
pub mod filename_patterns; // Model filename pattern matching
// generation_queue removed — replaced by out-of-process worker bridge
pub mod gguf_info; // GGUF model info extraction
pub mod gguf_utils; // GGUF metadata utilities
pub mod logger;
pub mod model_manager;
pub mod models;
#[allow(dead_code, unused_imports)]
pub mod mcp; // MCP (Model Context Protocol) client integration
pub mod desktop_tools; // Desktop automation tools (mouse, keyboard, scroll)
pub mod event_log; // In-memory event log for conversation debugging
pub mod native_tools; // Native file I/O and code execution tools
pub mod providers; // Multi-provider backend (local, Claude Code, etc.)
pub mod request;
pub mod request_parsing; // Request body parsing utilities
pub mod response_helpers; // Reusable HTTP response builders
pub mod routes;
pub mod skills; // Skills system: markdown-based prompt templates
pub mod utils;
pub mod vram_calculator; // GPU/VRAM calculations
pub mod websocket;
pub mod websocket_utils; // WebSocket helper functions
pub mod worker; // Out-of-process model worker

// Re-exports removed — import specific types where needed
