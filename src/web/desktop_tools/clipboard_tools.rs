//! Clipboard image read/write tools.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

#[cfg(windows)]
use super::win32;

/// Read or write an image from/to the clipboard.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_clipboard_image(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => clipboard_image_read(),
        "write" => {
            let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
            clipboard_image_write(monitor_idx)
        }
        other => NativeToolResult::text_only(format!("Unknown action '{other}'. Use 'read' or 'write'.")),
    }
}

#[cfg(windows)]
fn clipboard_image_read() -> NativeToolResult {
    unsafe {
        if win32::OpenClipboard(0) == 0 {
            return super::tool_error("clipboard_image", "could not open clipboard");
        }

        let handle = win32::GetClipboardData(win32::CF_DIB);
        if handle == 0 {
            win32::CloseClipboard();
            return super::tool_error("clipboard_image", "no image in clipboard (CF_DIB format)");
        }

        let size = win32::GlobalSize(handle);
        if size == 0 {
            win32::CloseClipboard();
            return super::tool_error("clipboard_image", "GlobalSize returned 0");
        }

        let ptr = win32::GlobalLock(handle);
        if ptr.is_null() {
            win32::CloseClipboard();
            return super::tool_error("clipboard_image", "GlobalLock failed");
        }

        let data = std::slice::from_raw_parts(ptr, size);

        // Parse BITMAPINFOHEADER (first 40 bytes minimum)
        if data.len() < 40 {
            win32::GlobalUnlock(handle);
            win32::CloseClipboard();
            return super::tool_error("clipboard_image", "DIB data too small");
        }

        let width = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let height_raw = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let bit_count = u16::from_le_bytes([data[14], data[15]]);
        let compression = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);

        if compression != 0 {
            // BI_RGB = 0
            win32::GlobalUnlock(handle);
            win32::CloseClipboard();
            return super::tool_error("clipboard_image", format!("unsupported DIB compression type {compression} (only BI_RGB supported)"));
        }

        let height = height_raw.unsigned_abs() as u32;
        let width = width.unsigned_abs() as u32;
        let top_down = height_raw < 0;

        // Calculate pixel data offset (after header + optional color table)
        let header_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let pixel_offset = header_size;
        let bytes_per_pixel = (bit_count / 8) as usize;
        let row_size = ((width as usize * bytes_per_pixel + 3) / 4) * 4; // DWORD-aligned rows

        if pixel_offset + row_size * height as usize > data.len() {
            win32::GlobalUnlock(handle);
            win32::CloseClipboard();
            return super::tool_error("clipboard_image", "DIB pixel data exceeds buffer size");
        }

        // Convert BGR(A) to RGBA
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            let src_row = if top_down { y } else { height - 1 - y };
            let src_start = pixel_offset + src_row as usize * row_size;
            for x in 0..width {
                let src_idx = src_start + x as usize * bytes_per_pixel;
                let dst_idx = (y * width + x) as usize * 4;
                rgba[dst_idx] = data[src_idx + 2];     // R (from B)
                rgba[dst_idx + 1] = data[src_idx + 1]; // G
                rgba[dst_idx + 2] = data[src_idx];     // B (from R)
                rgba[dst_idx + 3] = if bytes_per_pixel == 4 { data[src_idx + 3] } else { 255 }; // A
            }
        }

        win32::GlobalUnlock(handle);
        win32::CloseClipboard();

        // Encode to PNG
        let mut png_data = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
        if let Err(e) = image::ImageEncoder::write_image(
            encoder,
            &rgba,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        ) {
            return super::tool_error("clipboard_image", format!("encoding PNG: {e}"));
        }

        NativeToolResult::with_image(
            format!("Clipboard image: {width}x{height} ({bit_count}bpp)"),
            png_data,
        )
    }
}

#[cfg(windows)]
fn clipboard_image_write(monitor_idx: usize) -> NativeToolResult {
    // Capture the screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return super::tool_error("clipboard_image", format!("listing monitors: {e}")),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return super::tool_error("clipboard_image", format!("monitor {monitor_idx} not found")),
    };
    let img = match monitor.capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("clipboard_image", format!("capturing screen: {e}")),
    };

    let width = img.width();
    let height = img.height();
    let raw = img.into_raw();

    // Build DIB: BITMAPINFOHEADER (40 bytes) + pixel data (BGR, bottom-up, DWORD-aligned)
    let bytes_per_pixel = 3usize;
    let row_size = ((width as usize * bytes_per_pixel + 3) / 4) * 4;
    let pixel_data_size = row_size * height as usize;
    let total_size = 40 + pixel_data_size;

    unsafe {
        let hmem = win32::GlobalAlloc(win32::GMEM_MOVEABLE, total_size);
        if hmem == 0 {
            return super::tool_error("clipboard_image", "GlobalAlloc failed");
        }

        let ptr = win32::GlobalLock(hmem);
        if ptr.is_null() {
            return super::tool_error("clipboard_image", "GlobalLock failed");
        }

        let buf = std::slice::from_raw_parts_mut(ptr, total_size);

        // BITMAPINFOHEADER
        buf[0..4].copy_from_slice(&40u32.to_le_bytes());        // biSize
        buf[4..8].copy_from_slice(&(width as i32).to_le_bytes()); // biWidth
        buf[8..12].copy_from_slice(&(height as i32).to_le_bytes()); // biHeight (positive = bottom-up)
        buf[12..14].copy_from_slice(&1u16.to_le_bytes());       // biPlanes
        buf[14..16].copy_from_slice(&24u16.to_le_bytes());      // biBitCount
        buf[16..20].copy_from_slice(&0u32.to_le_bytes());       // biCompression = BI_RGB
        buf[20..24].copy_from_slice(&(pixel_data_size as u32).to_le_bytes()); // biSizeImage
        buf[24..40].fill(0); // remaining fields = 0

        // Convert RGBA top-down to BGR bottom-up
        for y in 0..height {
            let src_row = y as usize;
            let dst_row = (height - 1 - y) as usize;
            let dst_start = 40 + dst_row * row_size;
            for x in 0..width {
                let src_idx = (src_row * width as usize + x as usize) * 4;
                let dst_idx = dst_start + x as usize * bytes_per_pixel;
                buf[dst_idx] = raw[src_idx + 2];     // B
                buf[dst_idx + 1] = raw[src_idx + 1]; // G
                buf[dst_idx + 2] = raw[src_idx];     // R
            }
        }

        win32::GlobalUnlock(hmem);

        if win32::OpenClipboard(0) == 0 {
            return super::tool_error("clipboard_image", "could not open clipboard");
        }
        win32::EmptyClipboard();
        let result = win32::SetClipboardData(win32::CF_DIB, hmem);
        win32::CloseClipboard();

        if result == 0 {
            super::tool_error("clipboard_image", "SetClipboardData failed")
        } else {
            NativeToolResult::text_only(format!("Screenshot ({width}x{height}) copied to clipboard"))
        }
    }
}

// ─── macOS / Linux clipboard image via arboard ──────────────────────────────

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn clipboard_image_read() -> NativeToolResult {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(cb) => cb,
        Err(e) => return super::tool_error("clipboard_image", format!("could not open clipboard: {e}")),
    };

    let img_data = match clipboard.get_image() {
        Ok(data) => data,
        Err(e) => return super::tool_error("clipboard_image", format!("no image in clipboard: {e}")),
    };

    let width = img_data.width as u32;
    let height = img_data.height as u32;

    // arboard returns RGBA bytes
    let rgba_img = match image::RgbaImage::from_raw(width, height, img_data.bytes.into_owned()) {
        Some(img) => img,
        None => return super::tool_error("clipboard_image", "failed to create image from clipboard data"),
    };

    // Encode to PNG
    let mut png_data = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
    if let Err(e) = image::ImageEncoder::write_image(
        encoder,
        rgba_img.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    ) {
        return super::tool_error("clipboard_image", format!("encoding PNG: {e}"));
    }

    NativeToolResult::with_image(
        format!("Clipboard image: {width}x{height}"),
        png_data,
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn clipboard_image_write(monitor_idx: usize) -> NativeToolResult {
    // Capture the screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return super::tool_error("clipboard_image", format!("listing monitors: {e}")),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return super::tool_error("clipboard_image", format!("monitor {monitor_idx} not found")),
    };
    let img = match monitor.capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("clipboard_image", format!("capturing screen: {e}")),
    };

    let width = img.width() as usize;
    let height = img.height() as usize;
    let rgba_bytes = img.into_raw();

    let mut clipboard = match arboard::Clipboard::new() {
        Ok(cb) => cb,
        Err(e) => return super::tool_error("clipboard_image", format!("could not open clipboard: {e}")),
    };

    let img_data = arboard::ImageData {
        width,
        height,
        bytes: std::borrow::Cow::Owned(rgba_bytes),
    };

    match clipboard.set_image(img_data) {
        Ok(()) => NativeToolResult::text_only(format!("Screenshot ({width}x{height}) copied to clipboard")),
        Err(e) => super::tool_error("clipboard_image", format!("failed to set clipboard image: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_clipboard_image(_args: &Value) -> NativeToolResult {
    super::tool_error("clipboard_image", "not available on this platform")
}
