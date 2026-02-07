// Web server modules for LLaMA Chat

pub mod chat; // New modular chat implementation
pub mod chat_handler; // Legacy re-exports for backward compatibility
pub mod command;
pub mod config;
pub mod database;
pub mod filename_patterns; // Model filename pattern matching
// generation_queue removed — replaced by out-of-process worker bridge
pub mod gguf_utils; // GGUF metadata utilities
pub mod logger;
pub mod model_manager;
pub mod models;
pub mod request;
pub mod request_parsing; // Request body parsing utilities
pub mod response_helpers; // Reusable HTTP response builders
pub mod routes;
pub mod utils;
pub mod vram_calculator; // GPU/VRAM calculations
pub mod websocket;
pub mod websocket_utils; // WebSocket helper functions
pub mod worker; // Out-of-process model worker

// Re-exports removed — import specific types where needed
