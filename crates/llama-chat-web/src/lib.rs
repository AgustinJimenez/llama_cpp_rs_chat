//! Web server components for LLaMA Chat.
//!
//! Route handlers, WebSocket management, request/response helpers,
//! provider backends, and skills system.

// Import logging macros from types crate (used throughout)
#[allow(unused_imports)]
#[macro_use]
extern crate llama_chat_types;

pub mod native_tools_bridge;
pub mod request;
pub mod request_parsing;
pub mod response_helpers;
pub mod routes;
pub mod skills;
pub mod providers;
pub mod websocket;
pub mod websocket_utils;
