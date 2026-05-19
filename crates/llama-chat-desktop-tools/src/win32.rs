//! Win32 FFI declarations and helpers for desktop automation tools.
#![allow(dead_code)] // FFI module: declarations are often added ahead of use

mod types;
mod window_search;
mod window;
mod process;
mod input;
mod clipboard;
mod registry;

// Re-export everything for backward compatibility
pub use types::*;
pub use window::*;
pub use process::*;
pub use input::*;
pub use clipboard::*;
pub use registry::*;
