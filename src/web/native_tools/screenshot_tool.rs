//! Screenshot tool handler.

use serde_json::Value;
use super::NativeToolResult;

pub fn tool_take_screenshot_with_image(args: &Value) -> NativeToolResult {
    let monitor_idx = args
        .get("monitor")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(0);

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: Failed to enumerate monitors: {e}")),
    };

    if monitors.is_empty() {
        return NativeToolResult::text_only("Error: No monitors detected".to_string());
    }

    // List monitors mode (no image captured)
    if monitor_idx == -1 {
        let mut result = format!("Available monitors ({}):\n", monitors.len());
        for (i, mon) in monitors.iter().enumerate() {
            let name = mon.name().unwrap_or_else(|_| "Unknown".to_string());
            let w = mon.width().unwrap_or(0);
            let h = mon.height().unwrap_or(0);
            let primary = mon.is_primary().unwrap_or(false);
            result.push_str(&format!(
                "  [{}] {} - {}x{}{}\n",
                i, name, w, h,
                if primary { " (primary)" } else { "" }
            ));
        }
        return NativeToolResult::text_only(result);
    }

    // Select monitor
    let monitor = if monitor_idx == 0 {
        monitors
            .iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .unwrap_or(&monitors[0])
    } else {
        let idx = monitor_idx as usize;
        if idx >= monitors.len() {
            return NativeToolResult::text_only(format!(
                "Error: Monitor index {} out of range (0-{})",
                idx,
                monitors.len() - 1
            ));
        }
        &monitors[idx]
    };

    // Capture
    let image = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => return NativeToolResult::text_only(format!("Error: Screenshot capture failed: {e}")),
    };

    let width = image.width();
    let height = image.height();
    let mon_name = monitor.name().unwrap_or_else(|_| "Unknown".to_string());
    let is_primary = monitor.is_primary().unwrap_or(false);

    // Save to temp directory
    let screenshots_dir = std::env::temp_dir().join("llama_screenshots");
    if let Err(e) = std::fs::create_dir_all(&screenshots_dir) {
        return NativeToolResult::text_only(format!("Error: Failed to create screenshots directory: {e}"));
    }

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("screenshot_{timestamp}.png");
    let filepath = screenshots_dir.join(&filename);

    if let Err(e) = image.save(&filepath) {
        return NativeToolResult::text_only(format!("Error: Failed to save screenshot: {e}"));
    }

    let text = format!(
        "Screenshot saved: {}\nResolution: {}x{}\nMonitor: {} (primary: {})",
        filepath.display(),
        width,
        height,
        mon_name,
        if is_primary { "yes" } else { "no" }
    );

    // Also encode the image as PNG bytes for vision pipeline injection
    let png_bytes = std::fs::read(&filepath).unwrap_or_default();
    if png_bytes.is_empty() {
        NativeToolResult::text_only(text)
    } else {
        // Resize + JPEG-compress for vision models (saves tokens)
        let optimized = crate::web::desktop_tools::optimize_screenshot_for_vision(&png_bytes);
        NativeToolResult::with_image(text, optimized)
    }
}

