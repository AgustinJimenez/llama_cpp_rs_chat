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
        other => super::tool_error("clipboard_image", format!("unknown action '{other}', use 'read' or 'write'")),
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
    let monitors = match super::validated_monitors("clipboard_image", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let img = match monitors[monitor_idx].capture_image() {
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
        if total_size == 0 {
            win32::GlobalUnlock(hmem);
            return super::tool_error("clipboard_image", "zero-size allocation");
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
    let monitors = match super::validated_monitors("clipboard_image", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let img = match monitors[monitor_idx].capture_image() {
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

// ─── clear_clipboard ─────────────────────────────────────────────────────────

/// Clear all clipboard content.
#[cfg(windows)]
pub fn tool_clear_clipboard(_args: &Value) -> NativeToolResult {
    unsafe {
        if win32::OpenClipboard(0) == 0 {
            return super::tool_error("clear_clipboard", "Failed to open clipboard");
        }
        win32::EmptyClipboard();
        win32::CloseClipboard();
    }
    NativeToolResult::text_only("Clipboard cleared".to_string())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn tool_clear_clipboard(_args: &Value) -> NativeToolResult {
    match arboard::Clipboard::new() {
        Ok(mut cb) => {
            let _ = cb.set_text(String::new());
            NativeToolResult::text_only("Clipboard cleared".to_string())
        }
        Err(e) => super::tool_error("clear_clipboard", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_clear_clipboard(_args: &Value) -> NativeToolResult {
    super::tool_error("clear_clipboard", "not available on this platform")
}

// ─── clipboard_file_paths ────────────────────────────────────────────────────

/// Read or write file paths from/to clipboard.
/// Params: `action` ("read" or "write"), `paths` (array of strings, for write).
#[cfg(windows)]
pub fn tool_clipboard_file_paths(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            // Use existing win32::read_clipboard_files (CF_HDROP + DragQueryFileW)
            match win32::read_clipboard_files() {
                Ok(files) => {
                    if files.is_empty() {
                        NativeToolResult::text_only("No file paths in clipboard".to_string())
                    } else {
                        let mut output = format!("{} file path(s) in clipboard:\n", files.len());
                        for f in &files {
                            output.push_str(&format!("  {f}\n"));
                        }
                        NativeToolResult::text_only(output)
                    }
                }
                Err(e) => super::tool_error("clipboard_file_paths", e),
            }
        }
        "write" => {
            // Writing CF_HDROP requires building a DROPFILES struct + null-terminated wide string list.
            let paths = match args.get("paths").and_then(|v| v.as_array()) {
                Some(arr) => arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
                None => return super::tool_error("clipboard_file_paths", "'paths' array is required for write"),
            };
            if paths.is_empty() {
                return super::tool_error("clipboard_file_paths", "'paths' array must not be empty");
            }

            // DROPFILES struct is 20 bytes:
            //   DWORD pFiles (offset to file list)
            //   POINT pt (unused, {0,0})
            //   BOOL  fNC (0)
            //   BOOL  fWide (1 = Unicode)
            const DROPFILES_SIZE: usize = 20;

            // Build the wide string file list: each path null-terminated, double-null at end
            let mut wide_data: Vec<u16> = Vec::new();
            for p in &paths {
                wide_data.extend(p.encode_utf16());
                wide_data.push(0); // null terminator per path
            }
            wide_data.push(0); // double-null terminator

            let file_data_bytes = wide_data.len() * 2;
            let total_size = DROPFILES_SIZE + file_data_bytes;

            unsafe {
                let hmem = win32::GlobalAlloc(win32::GMEM_MOVEABLE, total_size);
                if hmem == 0 {
                    return super::tool_error("clipboard_file_paths", "GlobalAlloc failed");
                }
                let ptr = win32::GlobalLock(hmem);
                if ptr.is_null() {
                    return super::tool_error("clipboard_file_paths", "GlobalLock failed");
                }
                if total_size == 0 {
                    win32::GlobalUnlock(hmem);
                    return super::tool_error("clipboard_file_paths", "zero-size allocation");
                }
                let buf = std::slice::from_raw_parts_mut(ptr, total_size);
                buf.fill(0);

                // DROPFILES header
                buf[0..4].copy_from_slice(&(DROPFILES_SIZE as u32).to_le_bytes()); // pFiles offset
                // pt.x, pt.y, fNC are 0 (already zeroed)
                buf[16..20].copy_from_slice(&1u32.to_le_bytes()); // fWide = TRUE

                // Copy wide string data after header
                let src_bytes = std::slice::from_raw_parts(
                    wide_data.as_ptr() as *const u8,
                    file_data_bytes,
                );
                buf[DROPFILES_SIZE..].copy_from_slice(src_bytes);

                win32::GlobalUnlock(hmem);

                if win32::OpenClipboard(0) == 0 {
                    return super::tool_error("clipboard_file_paths", "Failed to open clipboard");
                }
                win32::EmptyClipboard();
                let result = win32::SetClipboardData(win32::CF_HDROP, hmem);
                win32::CloseClipboard();

                if result == 0 {
                    super::tool_error("clipboard_file_paths", "SetClipboardData failed")
                } else {
                    NativeToolResult::text_only(format!("Wrote {} file path(s) to clipboard", paths.len()))
                }
            }
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(target_os = "macos")]
pub fn tool_clipboard_file_paths(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            // Try to read file URLs from clipboard via osascript
            let output = std::process::Command::new("osascript")
                .args(["-e", "POSIX path of (the clipboard as «class furl»)"])
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if text.is_empty() {
                        NativeToolResult::text_only("No file paths in clipboard".to_string())
                    } else {
                        let paths: Vec<&str> = text.lines().collect();
                        let mut result = format!("{} file path(s) in clipboard:\n", paths.len());
                        for p in &paths {
                            result.push_str(&format!("  {p}\n"));
                        }
                        NativeToolResult::text_only(result)
                    }
                }
                _ => NativeToolResult::text_only("No file paths in clipboard".to_string()),
            }
        }
        "write" => {
            let paths = match args.get("paths").and_then(|v| v.as_array()) {
                Some(p) => p,
                None => return super::tool_error("clipboard_file_paths", "'paths' array required for write"),
            };
            let path_strs: Vec<&str> = paths.iter().filter_map(|v| v.as_str()).collect();
            if path_strs.is_empty() {
                return super::tool_error("clipboard_file_paths", "no valid paths provided");
            }
            let posix_files: Vec<String> = path_strs.iter()
                .map(|p| format!("POSIX file \"{}\"", p.replace('\\', "\\\\").replace('"', "\\\"")))
                .collect();
            let script = format!("set the clipboard to {{{}}}", posix_files.join(", "));
            match std::process::Command::new("osascript").args(["-e", &script]).output() {
                Ok(out) if out.status.success() => {
                    NativeToolResult::text_only(format!("{} file path(s) written to clipboard", path_strs.len()))
                }
                Ok(out) => super::tool_error("clipboard_file_paths",
                    format!("osascript: {}", String::from_utf8_lossy(&out.stderr).trim())),
                Err(e) => super::tool_error("clipboard_file_paths", format!("osascript: {e}")),
            }
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(target_os = "linux")]
pub fn tool_clipboard_file_paths(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            let output = std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/uri-list", "-o"])
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    let paths: Vec<String> = text
                        .lines()
                        .filter(|l| l.starts_with("file://"))
                        .map(|l| l.strip_prefix("file://").unwrap_or(l).to_string())
                        .collect();
                    if paths.is_empty() {
                        NativeToolResult::text_only("No file paths in clipboard".to_string())
                    } else {
                        let mut result = format!("{} file path(s) in clipboard:\n", paths.len());
                        for p in &paths {
                            result.push_str(&format!("  {p}\n"));
                        }
                        NativeToolResult::text_only(result)
                    }
                }
                _ => NativeToolResult::text_only("No file paths in clipboard (xclip not available or no URI data)".to_string()),
            }
        }
        "write" => {
            let paths = match args.get("paths").and_then(|v| v.as_array()) {
                Some(arr) => arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
                None => return super::tool_error("clipboard_file_paths", "'paths' array is required for write"),
            };
            if paths.is_empty() {
                return super::tool_error("clipboard_file_paths", "'paths' array must not be empty");
            }
            let uri_list: Vec<String> = paths.iter().map(|p| format!("file://{p}")).collect();
            let data = uri_list.join("\n");
            let mut child = match std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/uri-list"])
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => return super::tool_error("clipboard_file_paths", format!("xclip: {e}")),
            };
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                let _ = stdin.write_all(data.as_bytes());
            }
            let _ = child.wait();
            NativeToolResult::text_only(format!("Wrote {} file path(s) to clipboard", paths.len()))
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_clipboard_file_paths(_args: &Value) -> NativeToolResult {
    super::tool_error("clipboard_file_paths", "not available on this platform")
}

// ─── clipboard_html ──────────────────────────────────────────────────────────

/// Read or write HTML from/to clipboard.
/// Params: `action` ("read" or "write"), `html` (string, for write).
#[cfg(windows)]
pub fn tool_clipboard_html(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    // Register the "HTML Format" clipboard format
    let format_name: Vec<u16> = "HTML Format".encode_utf16().chain(std::iter::once(0)).collect();
    let cf_html = unsafe { win32::RegisterClipboardFormatW(format_name.as_ptr()) };
    if cf_html == 0 {
        return super::tool_error("clipboard_html", "Failed to register HTML Format clipboard format");
    }

    match action {
        "read" => {
            unsafe {
                if win32::OpenClipboard(0) == 0 {
                    return super::tool_error("clipboard_html", "Failed to open clipboard");
                }
                let handle = win32::GetClipboardData(cf_html);
                if handle == 0 {
                    win32::CloseClipboard();
                    return super::tool_error("clipboard_html", "No HTML data in clipboard");
                }
                let size = win32::GlobalSize(handle);
                if size == 0 {
                    win32::CloseClipboard();
                    return super::tool_error("clipboard_html", "GlobalSize returned 0");
                }
                let ptr = win32::GlobalLock(handle);
                if ptr.is_null() {
                    win32::CloseClipboard();
                    return super::tool_error("clipboard_html", "GlobalLock failed");
                }
                let data = std::slice::from_raw_parts(ptr, size);
                let raw_str = String::from_utf8_lossy(data);

                win32::GlobalUnlock(handle);
                win32::CloseClipboard();

                // Parse CF_HTML header to extract the fragment
                // Look for StartFragment/EndFragment byte offsets
                let start_frag = raw_str
                    .lines()
                    .find(|l| l.starts_with("StartFragment:"))
                    .and_then(|l| l.trim_start_matches("StartFragment:").trim().parse::<usize>().ok());
                let end_frag = raw_str
                    .lines()
                    .find(|l| l.starts_with("EndFragment:"))
                    .and_then(|l| l.trim_start_matches("EndFragment:").trim().parse::<usize>().ok());

                match (start_frag, end_frag) {
                    (Some(start), Some(end)) if start < end && end <= data.len() => {
                        let fragment = String::from_utf8_lossy(&data[start..end]).to_string();
                        let summary = if fragment.len() > 500 {
                            format!("HTML fragment ({} chars): {}...", fragment.len(), &fragment[..500])
                        } else {
                            format!("HTML fragment: {fragment}")
                        };
                        NativeToolResult::text_only(summary)
                    }
                    _ => {
                        // Fallback: return raw content (trimmed)
                        let trimmed = raw_str.trim_end_matches('\0').to_string();
                        let summary = if trimmed.len() > 500 {
                            format!("HTML clipboard ({} chars): {}...", trimmed.len(), &trimmed[..500])
                        } else {
                            format!("HTML clipboard: {trimmed}")
                        };
                        NativeToolResult::text_only(summary)
                    }
                }
            }
        }
        "write" => {
            let html = match args.get("html").and_then(|v| v.as_str()) {
                Some(h) => h,
                None => return super::tool_error("clipboard_html", "'html' argument is required for write"),
            };

            // Build CF_HTML format with proper header
            let fragment = html;
            let html_body = format!(
                "<html>\r\n<body>\r\n<!--StartFragment-->{fragment}<!--EndFragment-->\r\n</body>\r\n</html>"
            );

            // Calculate offsets — header fields are fixed-width with zero-padded numbers
            // Header format: Version + StartHTML + EndHTML + StartFragment + EndFragment
            let header_template = "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
            let header_len = header_template.len();
            let start_html = header_len;
            let start_fragment = header_len + html_body.find("<!--StartFragment-->").unwrap_or(0) + "<!--StartFragment-->".len();
            let end_fragment = header_len + html_body.find("<!--EndFragment-->").unwrap_or(html_body.len());
            let end_html = header_len + html_body.len();

            let header = format!(
                "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n",
                start_html, end_html, start_fragment, end_fragment
            );

            let cf_html_data = format!("{header}{html_body}");
            let bytes = cf_html_data.as_bytes();

            unsafe {
                let hmem = win32::GlobalAlloc(win32::GMEM_MOVEABLE, bytes.len() + 1);
                if hmem == 0 {
                    return super::tool_error("clipboard_html", "GlobalAlloc failed");
                }
                let ptr = win32::GlobalLock(hmem);
                if ptr.is_null() {
                    return super::tool_error("clipboard_html", "GlobalLock failed");
                }
                if bytes.is_empty() {
                    win32::GlobalUnlock(hmem);
                    return super::tool_error("clipboard_html", "empty HTML content");
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
                *ptr.add(bytes.len()) = 0; // null terminate
                win32::GlobalUnlock(hmem);

                if win32::OpenClipboard(0) == 0 {
                    return super::tool_error("clipboard_html", "Failed to open clipboard");
                }
                win32::EmptyClipboard();
                let result = win32::SetClipboardData(cf_html, hmem);
                win32::CloseClipboard();

                if result == 0 {
                    super::tool_error("clipboard_html", "SetClipboardData failed")
                } else {
                    NativeToolResult::text_only(format!("Wrote {} chars of HTML to clipboard", html.len()))
                }
            }
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(target_os = "macos")]
pub fn tool_clipboard_html(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            let output = std::process::Command::new("osascript")
                .args(["-e", "the clipboard as «class HTML»"])
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if text.is_empty() {
                        NativeToolResult::text_only("No HTML data in clipboard".to_string())
                    } else {
                        let summary = if text.len() > 500 {
                            format!("HTML clipboard ({} chars): {}...", text.len(), &text[..500])
                        } else {
                            format!("HTML clipboard: {text}")
                        };
                        NativeToolResult::text_only(summary)
                    }
                }
                _ => NativeToolResult::text_only("No HTML data in clipboard".to_string()),
            }
        }
        "write" => {
            let html = match args.get("html").and_then(|v| v.as_str()) {
                Some(h) => h,
                None => return super::tool_error("clipboard_html", "'html' is required for write"),
            };
            // Use AppleScriptObjC to set HTML data on NSPasteboard
            let escaped = html.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!(
                concat!(
                    "use framework \"AppKit\"\n",
                    "set htmlData to (current application's NSString's stringWithString:\"{}\")",
                    "'s dataUsingEncoding:(current application's NSUTF8StringEncoding)\n",
                    "set pb to current application's NSPasteboard's generalPasteboard()\n",
                    "pb's clearContents()\n",
                    "pb's setData:htmlData forType:\"public.html\""
                ),
                escaped
            );
            match std::process::Command::new("osascript").args(["-l", "AppleScriptObjC", "-e", &script]).output() {
                Ok(out) if out.status.success() => {
                    NativeToolResult::text_only(format!("HTML written to clipboard ({} chars)", html.len()))
                }
                Ok(out) => super::tool_error("clipboard_html",
                    format!("osascript: {}", String::from_utf8_lossy(&out.stderr).trim())),
                Err(e) => super::tool_error("clipboard_html", format!("osascript: {e}")),
            }
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(target_os = "linux")]
pub fn tool_clipboard_html(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            let output = std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/html", "-o"])
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    if text.is_empty() {
                        NativeToolResult::text_only("No HTML data in clipboard".to_string())
                    } else {
                        let summary = if text.len() > 500 {
                            format!("HTML clipboard ({} chars): {}...", text.len(), &text[..500])
                        } else {
                            format!("HTML clipboard: {text}")
                        };
                        NativeToolResult::text_only(summary)
                    }
                }
                _ => NativeToolResult::text_only("No HTML data in clipboard (xclip not available or no HTML data)".to_string()),
            }
        }
        "write" => {
            let html = match args.get("html").and_then(|v| v.as_str()) {
                Some(h) => h,
                None => return super::tool_error("clipboard_html", "'html' argument is required for write"),
            };
            let mut child = match std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/html"])
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => return super::tool_error("clipboard_html", format!("xclip: {e}")),
            };
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                let _ = stdin.write_all(html.as_bytes());
            }
            let _ = child.wait();
            NativeToolResult::text_only(format!("Wrote {} chars of HTML to clipboard", html.len()))
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_clipboard_html(_args: &Value) -> NativeToolResult {
    super::tool_error("clipboard_html", "not available on this platform")
}
