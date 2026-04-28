/// Result from a native tool, carrying text output and optional image data.
/// Image data is used by vision-capable models to "see" tool outputs (e.g., screenshots).
#[derive(Debug)]
pub struct NativeToolResult {
    pub text: String,
    /// Raw image bytes (PNG/JPEG) for vision pipeline injection.
    /// Only populated by tools like `take_screenshot` when capture succeeds.
    pub images: Vec<Vec<u8>>,
}

impl NativeToolResult {
    pub fn text_only(text: String) -> Self {
        Self { text, images: Vec::new() }
    }
    pub fn with_image(text: String, image_bytes: Vec<u8>) -> Self {
        Self { text, images: vec![image_bytes] }
    }
}
