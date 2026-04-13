//! Shared helper functions for desktop automation tools.
//!
//! Includes coordinate validation, image encoding/resizing, monitor caching,
//! input simulation (enigo), key parsing, and screenshot caching.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use enigo::{Enigo, Key, Settings};
use serde_json::Value;

use super::NativeToolResult;

/// Default timeout for thread-spawned operations (OCR, UI Automation, etc.).
pub(crate) const DEFAULT_THREAD_TIMEOUT: Duration = Duration::from_secs(20);

/// Maximum dimension (width or height) for screenshots sent to vision models.
/// Keeps token cost manageable while preserving enough detail for UI analysis.
pub(crate) const SCREENSHOT_MAX_DIM: u32 = 1280;

// ─── Monitor cache ──────────────────────────────────────────────────────────

/// Wrapper to make `Vec<xcap::Monitor>` safe for static storage.
/// HMONITOR handles are global Win32 system handles, safe across threads.
struct MonitorCache(Option<(Vec<xcap::Monitor>, std::time::Instant)>);
// Safety: HMONITOR is a global system handle; sharing across threads is safe.
unsafe impl Send for MonitorCache {}

/// Cached monitor list with 1-second TTL to avoid repeated FFI calls.
static MONITOR_CACHE: std::sync::Mutex<MonitorCache> =
    std::sync::Mutex::new(MonitorCache(None));
const MONITOR_CACHE_TTL_MS: u128 = 1000;

/// Format a desktop tool error consistently.
#[allow(dead_code)]
pub(crate) fn tool_error(tool: &str, msg: impl std::fmt::Display) -> NativeToolResult {
    NativeToolResult::text_only(format!("Error [{tool}]: {msg}"))
}

/// Format a platform-not-supported error.
#[allow(dead_code)]
pub(crate) fn tool_not_supported(tool: &str) -> NativeToolResult {
    NativeToolResult::text_only(format!("Error [{tool}]: not available on this platform"))
}

/// Return cached monitors or re-enumerate if stale/empty.
pub(crate) fn cached_monitors() -> Result<Vec<xcap::Monitor>, String> {
    let mut lock = MONITOR_CACHE.lock().unwrap_or_else(|p| {
        crate::log_warn!("system", "Mutex poisoned in MONITOR_CACHE, recovering");
        p.into_inner()
    });
    if let Some((ref monitors, ref ts)) = lock.0 {
        if ts.elapsed().as_millis() < MONITOR_CACHE_TTL_MS && !monitors.is_empty() {
            return Ok(monitors.clone());
        }
    }
    let monitors = xcap::Monitor::all().map_err(|e| format!("{e}"))?;
    lock.0 = Some((monitors.clone(), std::time::Instant::now()));
    Ok(monitors)
}

/// Validate monitor index and return all monitors + validated index, or a tool_error.
pub(crate) fn validated_monitors(
    tool: &str,
    monitor_idx: usize,
) -> Result<Vec<xcap::Monitor>, NativeToolResult> {
    let monitors = cached_monitors()
        .map_err(|e| tool_error(tool, format!("enumerate monitors: {e}")))?;
    if monitors.is_empty() {
        return Err(tool_error(tool, "no monitors detected"));
    }
    if monitor_idx >= monitors.len() {
        return Err(tool_error(
            tool,
            format!("monitor {} out of range (0..{})", monitor_idx, monitors.len()),
        ));
    }
    Ok(monitors)
}

/// Compare two raw RGBA buffers, sampling every 16th pixel for speed.
/// Returns the percentage of sampled pixels that differ significantly.
pub(crate) fn pixel_diff_pct(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() {
        return 100.0;
    }
    let step = 32 * 4; // every 32nd pixel, 4 bytes per pixel (RGBA)
    let mut diff = 0u64;
    let mut total = 0u64;
    let mut i = 0;
    while i + 3 < a.len() {
        total += 1;
        let dr = (a[i] as i32 - b[i] as i32).abs();
        let dg = (a[i + 1] as i32 - b[i + 1] as i32).abs();
        let db = (a[i + 2] as i32 - b[i + 2] as i32).abs();
        if dr > 10 || dg > 10 || db > 10 {
            diff += 1;
        }
        i += step;
    }
    if total == 0 {
        return 100.0;
    }
    diff as f64 / total as f64 * 100.0
}

/// Encode an `image::RgbaImage` to PNG bytes in memory.
pub(crate) fn encode_image_to_png(img: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(buf.into_inner())
}

/// Resize screenshot PNG bytes so the longest side is at most `max_dim` pixels.
/// Returns the original bytes unchanged if the image is already small enough.
pub(crate) fn resize_screenshot_for_vision(png_bytes: &[u8], max_dim: u32) -> Vec<u8> {
    use image::GenericImageView;
    let img = match image::load_from_memory(png_bytes) {
        Ok(i) => i,
        Err(_) => return png_bytes.to_vec(),
    };
    let (w, h) = img.dimensions();
    if w <= max_dim && h <= max_dim {
        return png_bytes.to_vec(); // Already small enough
    }
    let ratio = max_dim as f32 / w.max(h) as f32;
    let new_w = (w as f32 * ratio) as u32;
    let new_h = (h as f32 * ratio) as u32;
    let resized = img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3);
    let mut buf = Vec::new();
    if resized
        .write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .is_ok()
        && !buf.is_empty()
    {
        eprintln!(
            "[SCREENSHOT] Resized {}x{} → {}x{} and JPEG-compressed: {} → {} bytes ({:.0}% reduction)",
            w, h, new_w, new_h,
            png_bytes.len(), buf.len(),
            (1.0 - buf.len() as f64 / png_bytes.len() as f64) * 100.0
        );
        buf
    } else {
        png_bytes.to_vec()
    }
}

/// Encode PNG bytes as JPEG with the given quality (0-100).
/// Falls back to the original PNG bytes on any error.
#[allow(dead_code)]
pub(crate) fn encode_as_jpeg(png_bytes: &[u8], quality: u8) -> Vec<u8> {
    let img = match image::load_from_memory(png_bytes) {
        Ok(i) => i,
        Err(_) => return png_bytes.to_vec(),
    };
    // Convert to RGB8 (JPEG doesn't support alpha)
    let rgb = img.to_rgb8();
    let mut buf = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
        std::io::Cursor::new(&mut buf),
        quality,
    );
    if image::ImageEncoder::write_image(
        encoder,
        &rgb,
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )
    .is_ok()
        && !buf.is_empty()
    {
        buf
    } else {
        png_bytes.to_vec()
    }
}

/// Convenience: resize + JPEG-compress a screenshot for vision models.
/// Uses SCREENSHOT_MAX_DIM and 75% JPEG quality (similar to Claude Code).
pub(crate) fn optimize_screenshot_for_vision(png_bytes: &[u8]) -> Vec<u8> {
    resize_screenshot_for_vision(png_bytes, SCREENSHOT_MAX_DIM)
}

/// Helper: parse integer from JSON value (handles both number and string).
pub(crate) fn parse_int(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Helper: parse float from JSON value (handles both number and string).
pub(crate) fn parse_float(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Helper: parse bool from JSON value.
pub(crate) fn parse_bool(v: &Value, default: bool) -> bool {
    v.as_bool()
        .or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true")))
        .unwrap_or(default)
}

/// Parse optional timeout_ms from tool args, clamped to 1000..60000ms.
#[allow(dead_code)]
pub(crate) fn parse_timeout(args: &Value) -> std::time::Duration {
    match args.get("timeout_ms").and_then(parse_int) {
        Some(ms) => std::time::Duration::from_millis((ms.max(1000).min(60000)) as u64),
        None => DEFAULT_THREAD_TIMEOUT,
    }
}

// ─── Reusable Enigo instance ─────────────────────────────────────────────────

thread_local! {
    static ENIGO: RefCell<Option<Enigo>> = RefCell::new(None);
}

/// Run a closure with a cached per-thread Enigo instance.
pub(crate) fn with_enigo<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce(&mut Enigo) -> Result<T, String>,
{
    ENIGO.with(|cell| {
        let mut opt = cell.borrow_mut();
        if opt.is_none() {
            *opt = Some(
                Enigo::new(&Settings::default())
                    .map_err(|e| format!("Failed to init input simulation: {e}"))?,
            );
        }
        f(opt.as_mut().unwrap())
    })
}

/// Validate that coordinates fall within any connected monitor.
pub(crate) fn validate_coordinates(x: i32, y: i32) -> Result<(), String> {
    let monitors =
        xcap::Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    if monitors.is_empty() {
        return Ok(());
    }
    for mon in &monitors {
        let mx = mon.x().unwrap_or(0);
        let my = mon.y().unwrap_or(0);
        let mw = mon.width().unwrap_or(0) as i32;
        let mh = mon.height().unwrap_or(0) as i32;
        if x >= mx && x < mx + mw && y >= my && y < my + mh {
            return Ok(());
        }
    }
    Err(format!(
        "Coordinates ({x}, {y}) outside all monitors. Available: {}",
        monitors
            .iter()
            .enumerate()
            .map(|(i, m)| format!(
                "#{}: {}x{} at ({},{})",
                i,
                m.width().unwrap_or(0),
                m.height().unwrap_or(0),
                m.x().unwrap_or(0),
                m.y().unwrap_or(0)
            ))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Snap off-screen coordinates to the nearest monitor edge.
/// If the point is already on-screen (passes `validate_coordinates`), returns it unchanged.
/// Otherwise, clamps to the nearest edge of the closest monitor.
pub(crate) fn snap_coordinates(x: i32, y: i32) -> (i32, i32) {
    // If already valid, return as-is
    if validate_coordinates(x, y).is_ok() {
        return (x, y);
    }
    let monitors = match xcap::Monitor::all() {
        Ok(m) if !m.is_empty() => m,
        _ => return (x, y), // can't snap without monitor info
    };
    // Find the monitor whose bounding box is closest and clamp to its edges
    let mut best_x = x;
    let mut best_y = y;
    let mut best_dist = i64::MAX;
    for mon in &monitors {
        let mx = mon.x().unwrap_or(0);
        let my = mon.y().unwrap_or(0);
        let mw = mon.width().unwrap_or(0) as i32;
        let mh = mon.height().unwrap_or(0) as i32;
        let cx = x.max(mx).min(mx + mw - 1);
        let cy = y.max(my).min(my + mh - 1);
        let dx = (x as i64 - cx as i64).abs();
        let dy = (y as i64 - cy as i64).abs();
        let dist = dx * dx + dy * dy;
        if dist < best_dist {
            best_dist = dist;
            best_x = cx;
            best_y = cy;
        }
    }
    (best_x, best_y)
}

/// Apply DPI scaling to coordinates if dpi_aware flag is set.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) fn apply_dpi_scaling(x: i32, y: i32, dpi_aware: bool) -> (i32, i32) {
    if dpi_aware {
        #[cfg(windows)]
        let scale = super::win32::get_system_dpi_scale();
        #[cfg(target_os = "macos")]
        let scale = super::macos::get_system_dpi_scale();
        #[cfg(target_os = "linux")]
        let scale = super::linux::get_system_dpi_scale();
        ((x as f64 * scale) as i32, (y as f64 * scale) as i32)
    } else {
        (x, y)
    }
}

/// Check if the foreground window is blocked by a modal dialog.
/// Returns a warning string if blocked, None otherwise.
#[cfg(windows)]
pub(crate) fn check_modal_dialog() -> Option<String> {
    let fg = unsafe { super::win32::GetForegroundWindow() };
    if fg != 0 {
        if let Some(_popup) = super::win32::is_window_blocked(fg) {
            return Some(
                "Warning: foreground window is blocked by a modal dialog. ".to_string(),
            );
        }
    }
    None
}

#[cfg(not(windows))]
pub(crate) fn check_modal_dialog() -> Option<String> {
    None // Modal dialog detection not available on macOS/Linux
}

/// Parse a key string like "ctrl+shift+s" into (modifiers, main_key).
pub(crate) fn parse_key_combo(key_str: &str) -> Result<(Vec<Key>, Key), String> {
    let lower = key_str.to_lowercase();
    let parts: Vec<&str> = lower.split('+').map(|s| s.trim()).collect();

    if parts.is_empty() {
        return Err("Empty key string".to_string());
    }

    let mut modifiers = Vec::new();

    for part in &parts[..parts.len().saturating_sub(1)] {
        modifiers.push(match *part {
            "ctrl" | "control" => Key::Control,
            "alt" => Key::Alt,
            "shift" => Key::Shift,
            "meta" | "win" | "super" | "cmd" | "command" => Key::Meta,
            other => return Err(format!("Unknown modifier: '{other}'")),
        });
    }

    let main = parts.last().ok_or("Empty key string")?;
    let key = str_to_key(main)?;

    Ok((modifiers, key))
}

/// Convert a string key name to an enigo Key.
pub(crate) fn str_to_key(s: &str) -> Result<Key, String> {
    match s {
        "enter" | "return" => Ok(Key::Return),
        "tab" => Ok(Key::Tab),
        "escape" | "esc" => Ok(Key::Escape),
        "backspace" => Ok(Key::Backspace),
        "delete" | "del" => Ok(Key::Delete),
        "space" => Ok(Key::Space),
        "up" => Ok(Key::UpArrow),
        "down" => Ok(Key::DownArrow),
        "left" => Ok(Key::LeftArrow),
        "right" => Ok(Key::RightArrow),
        "home" => Ok(Key::Home),
        "end" => Ok(Key::End),
        "pageup" => Ok(Key::PageUp),
        "pagedown" => Ok(Key::PageDown),
        "insert" => Ok(Key::Other(0x2D)),
        "capslock" => Ok(Key::CapsLock),
        "ctrl" | "control" => Ok(Key::Control),
        "alt" => Ok(Key::Alt),
        "shift" => Ok(Key::Shift),
        "meta" | "win" | "super" | "cmd" => Ok(Key::Meta),
        s if s.starts_with('f') && s.len() <= 3 => match s[1..].parse::<u8>() {
            Ok(1) => Ok(Key::F1),
            Ok(2) => Ok(Key::F2),
            Ok(3) => Ok(Key::F3),
            Ok(4) => Ok(Key::F4),
            Ok(5) => Ok(Key::F5),
            Ok(6) => Ok(Key::F6),
            Ok(7) => Ok(Key::F7),
            Ok(8) => Ok(Key::F8),
            Ok(9) => Ok(Key::F9),
            Ok(10) => Ok(Key::F10),
            Ok(11) => Ok(Key::F11),
            Ok(12) => Ok(Key::F12),
            Ok(n) => Err(format!("Unsupported function key: F{n}")),
            Err(_) => Err(format!("Invalid key: '{s}'")),
        },
        s if s.len() == 1 => Ok(Key::Unicode(s.chars().next().unwrap())),
        other => Err(format!("Unknown key: '{other}'")),
    }
}

// ─── Screenshot cache ────────────────────────────────────────────────────────

thread_local! {
    /// (timestamp, raw_rgba_bytes, png_bytes)
    static LAST_SCREENSHOT: RefCell<Option<(std::time::Instant, Arc<Vec<u8>>, Arc<Vec<u8>>)>> = RefCell::new(None);
}

/// Get a cached screenshot if it was taken within `max_age_ms` milliseconds.
/// Returns (raw_rgba_bytes, png_bytes).
pub(crate) fn get_cached_screenshot(max_age_ms: u64) -> Option<(Arc<Vec<u8>>, Arc<Vec<u8>>)> {
    LAST_SCREENSHOT.with(|cell| {
        let cache = cell.borrow();
        if let Some((when, raw, png)) = cache.as_ref() {
            if when.elapsed().as_millis() < max_age_ms as u128 {
                return Some((Arc::clone(raw), Arc::clone(png)));
            }
        }
        None
    })
}

/// Store a screenshot in the cache (raw RGBA + encoded PNG).
pub(crate) fn update_screenshot_cache(raw: Vec<u8>, png: Vec<u8>) {
    LAST_SCREENSHOT.with(|cell| {
        *cell.borrow_mut() = Some((std::time::Instant::now(), Arc::new(raw), Arc::new(png)));
    });
}

/// Wait for the screen to stop changing (useful after animations/transitions).
/// Returns true if screen settled within timeout, false if timed out.
#[allow(dead_code)]
pub(crate) fn wait_for_screen_settle(timeout_ms: u64, poll_ms: u64) -> bool {
    let start = std::time::Instant::now();
    let mut prev: Option<Vec<u8>> = None;
    while start.elapsed().as_millis() < timeout_ms as u128 {
        if super::desktop_call_cancelled() {
            return false;
        }
        // Capture a small region (center of primary monitor) for quick comparison
        if let Ok(monitors) = xcap::Monitor::all() {
            if let Some(mon) = monitors.first() {
                if let Ok(img) = mon.capture_image() {
                    let raw = img.as_raw().to_vec();
                    if let Some(ref p) = prev {
                        if *p == raw {
                            return true; // screen settled
                        }
                    }
                    prev = Some(raw);
                }
            }
        }
        if super::interruptible_sleep(std::time::Duration::from_millis(poll_ms)).is_err() {
            return false;
        }
    }
    false
}
