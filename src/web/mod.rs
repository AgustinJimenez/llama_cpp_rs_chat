// Web server modules for LLaMA Chat

pub mod models;
pub mod config;
pub mod command;
pub mod conversation;
pub mod model_manager;
pub mod vram_calculator;  // GPU/VRAM calculations
pub mod gguf_utils;  // GGUF metadata utilities
pub mod websocket_utils;  // WebSocket helper functions
pub mod chat;  // New modular chat implementation
pub mod chat_handler;  // Legacy re-exports for backward compatibility
pub mod websocket;
pub mod utils;
pub mod response;
pub mod request;
pub mod logger;
pub mod routes;
pub mod database;

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
