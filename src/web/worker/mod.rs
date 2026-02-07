//! Out-of-process model worker.
//!
//! The model runs in a separate child process for:
//! - Memory reclaim: kill the process to free all VRAM/RAM
//! - Crash isolation: model crash doesn't kill the web server

pub mod ipc_types;
pub mod process_manager;
pub mod worker_bridge;
pub mod worker_main;
