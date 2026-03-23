//! Provider abstraction layer for multiple AI backends.
//!
//! Supports:
//! - Local (llama.cpp via worker process)
//! - Claude Code (CLI subprocess using user's subscription)
//! - Future: OpenAI-compatible, Qwen, Gemini, etc.

pub mod claude_code;
