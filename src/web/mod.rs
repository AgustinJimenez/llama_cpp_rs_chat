// Web server modules for LLaMA Chat

pub mod chat; // New modular chat implementation
pub mod chat_handler; // Legacy re-exports for backward compatibility
pub mod command;
pub mod config;
pub mod database;
pub mod filename_patterns; // Model filename pattern matching
pub mod generation_queue; // Generation request queue with cancellation
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

// Re-export commonly used types
pub use models::*;
// Removed unused re-exports to fix compiler warnings
// pub use config::*;
// pub use conversation::*;
// pub use model_manager::*;
// pub use chat_handler::*;
// pub use websocket::*;
// pub use response::*;
// pub use request::*;
