//! Clipboard tools: image, file paths, HTML, and clear.

mod clipboard_clear;
mod clipboard_files;
mod clipboard_html;
mod clipboard_image;

pub use clipboard_clear::tool_clear_clipboard;
pub use clipboard_files::tool_clipboard_file_paths;
pub use clipboard_html::tool_clipboard_html;
pub use clipboard_image::tool_clipboard_image;
