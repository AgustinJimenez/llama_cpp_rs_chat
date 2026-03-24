//! Desktop automation tools: mouse, keyboard, and scroll input simulation.
//!
//! Uses the `enigo` crate for cross-platform input and `xcap` for post-action screenshots.
//! Each action tool optionally captures a screenshot after the action, returning it through
//! the vision pipeline so the LLM can see what happened.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use serde_json::Value;

use super::native_tools::NativeToolResult;

/// Default timeout for thread-spawned operations (OCR, UI Automation, etc.).
const DEFAULT_THREAD_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct DesktopCancellationContext {
    cancelled: Arc<AtomicBool>,
    deadline: std::time::Instant,
}

#[allow(dead_code)]
impl DesktopCancellationContext {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: std::time::Instant::now() + timeout,
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || std::time::Instant::now() >= self.deadline
    }

    fn remaining(&self) -> Option<Duration> {
        self.deadline.checked_duration_since(std::time::Instant::now())
    }
}

thread_local! {
    static CURRENT_CANCEL_CONTEXT: RefCell<Option<DesktopCancellationContext>> = const { RefCell::new(None) };
}

pub fn with_desktop_cancellation_context<T>(
    context: DesktopCancellationContext,
    f: impl FnOnce() -> T,
) -> T {
    CURRENT_CANCEL_CONTEXT.with(|cell| {
        let previous = cell.replace(Some(context));
        let result = f();
        cell.replace(previous);
        result
    })
}

pub(super) fn current_desktop_cancellation_context() -> Option<DesktopCancellationContext> {
    CURRENT_CANCEL_CONTEXT.with(|cell| cell.borrow().clone())
}

pub(super) fn desktop_call_cancelled() -> bool {
    current_desktop_cancellation_context()
        .map(|ctx| ctx.is_cancelled())
        .unwrap_or(false)
}

pub(super) fn desktop_cancel_error() -> String {
    if let Some(ctx) = current_desktop_cancellation_context() {
        if ctx.cancelled.load(Ordering::Relaxed) {
            "Operation cancelled".to_string()
        } else if std::time::Instant::now() >= ctx.deadline {
            "Operation timed out".to_string()
        } else {
            "Operation cancelled".to_string()
        }
    } else {
        "Operation cancelled".to_string()
    }
}

pub(super) fn ensure_desktop_not_cancelled() -> Result<(), String> {
    if desktop_call_cancelled() {
        Err(desktop_cancel_error())
    } else {
        Ok(())
    }
}

pub(super) fn interruptible_sleep(duration: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + duration;
    let slice = Duration::from_millis(50);
    loop {
        ensure_desktop_not_cancelled()?;
        let now = std::time::Instant::now();
        if now >= deadline {
            return Ok(());
        }
        let remaining = deadline.duration_since(now).min(slice);
        std::thread::sleep(remaining);
    }
}

/// Spawn a closure on a new thread and wait up to `timeout` for it to finish.
/// Returns Err if the thread panics or times out.
pub(super) fn spawn_with_timeout<F, T>(timeout: Duration, f: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    ensure_desktop_not_cancelled()?;

    let context = current_desktop_cancellation_context();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = if let Some(context) = context {
            with_desktop_cancellation_context(context, f)
        } else {
            f()
        };
        let _ = tx.send(result);
    });

    let timeout_deadline = std::time::Instant::now() + timeout;
    let poll = Duration::from_millis(50);

    loop {
        match rx.recv_timeout(poll) {
            Ok(result) => return Ok(result),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(context) = current_desktop_cancellation_context() {
                    if context.is_cancelled() {
                        return Err(desktop_cancel_error());
                    }
                }
                if std::time::Instant::now() >= timeout_deadline {
                    if let Some(context) = current_desktop_cancellation_context() {
                        context.cancel();
                    }
                    return Err(format!("Operation timed out after {}ms", timeout.as_millis()));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Thread panicked".to_string())
            }
        }
    }
}

/// Format a desktop tool error consistently.
#[allow(dead_code)]
pub(super) fn tool_error(tool: &str, msg: impl std::fmt::Display) -> NativeToolResult {
    NativeToolResult::text_only(format!("Error [{tool}]: {msg}"))
}

/// Format a platform-not-supported error.
#[allow(dead_code)]
pub(super) fn tool_not_supported(tool: &str) -> NativeToolResult {
    NativeToolResult::text_only(format!("Error [{tool}]: not available on this platform"))
}

/// Wrapper to make `Vec<xcap::Monitor>` safe for static storage.
/// HMONITOR handles are global Win32 system handles, safe across threads.
struct MonitorCache(Option<(Vec<xcap::Monitor>, std::time::Instant)>);
// Safety: HMONITOR is a global system handle; sharing across threads is safe.
unsafe impl Send for MonitorCache {}

/// Cached monitor list with 1-second TTL to avoid repeated FFI calls.
static MONITOR_CACHE: std::sync::Mutex<MonitorCache> =
    std::sync::Mutex::new(MonitorCache(None));
const MONITOR_CACHE_TTL_MS: u128 = 1000;

/// Return cached monitors or re-enumerate if stale/empty.
pub(super) fn cached_monitors() -> Result<Vec<xcap::Monitor>, String> {
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
pub(super) fn validated_monitors(
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
pub(super) fn pixel_diff_pct(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() {
        return 100.0;
    }
    let step = 16 * 4; // every 16th pixel, 4 bytes per pixel (RGBA)
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
fn encode_image_to_png(img: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(buf.into_inner())
}

fn desktop_trace_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("LLAMA_CHAT_DESKTOP_TRACE_PATH") {
        if !path.trim().is_empty() {
            return std::path::PathBuf::from(path);
        }
    }
    std::env::temp_dir().join("llama_cpp_chat_desktop_trace.jsonl")
}

fn classify_desktop_result_status(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if lower.starts_with("error") {
        if lower.contains("timed out") || lower.contains("timeout") {
            "timed_out"
        } else if lower.contains("cancelled") || lower.contains("canceled") {
            "cancelled"
        } else {
            "error"
        }
    } else if lower.starts_with("timeout:") {
        "timed_out"
    } else if lower.contains("verification failed:") {
        "verification_failed"
    } else {
        "completed"
    }
}

struct ScreenVerificationContext {
    baseline_raw: Vec<u8>,
    threshold_pct: f64,
    timeout_ms: u64,
    poll_ms: u64,
    region: Option<VerificationRegion>,
    expected_text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VerificationRegion {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn normalize_verification_region(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Option<VerificationRegion> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(VerificationRegion {
        x,
        y,
        width,
        height,
    })
}

fn action_verification_region_from_args(args: &Value) -> Option<VerificationRegion> {
    if let (Some(x), Some(y), Some(width), Some(height)) = (
        args.get("verify_x").and_then(parse_int),
        args.get("verify_y").and_then(parse_int),
        args.get("verify_width").and_then(parse_int),
        args.get("verify_height").and_then(parse_int),
    ) {
        return normalize_verification_region(x as i32, y as i32, width as u32, height as u32);
    }
    None
}

fn capture_screen_state(
    region: Option<VerificationRegion>,
) -> Result<(Vec<u8>, Vec<u8>, u32, u32, String), String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    let monitor = if let Some(region) = region {
        let center_x = region.x + region.width as i32 / 2;
        let center_y = region.y + region.height as i32 / 2;
        monitors
            .iter()
            .find(|m| {
                let mx = m.x().unwrap_or(0);
                let my = m.y().unwrap_or(0);
                let mw = m.width().unwrap_or(0) as i32;
                let mh = m.height().unwrap_or(0) as i32;
                center_x >= mx && center_x < mx + mw && center_y >= my && center_y < my + mh
            })
            .or_else(|| monitors.first())
    } else {
        monitors
            .iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .or(monitors.first())
    }
    .ok_or_else(|| "No monitors detected".to_string())?;
    let img = monitor
        .capture_image()
        .map_err(|e| format!("Screenshot capture failed: {e}"))?;

    if let Some(region) = region {
        let mx = monitor.x().unwrap_or(0);
        let my = monitor.y().unwrap_or(0);
        let rel_x = (region.x - mx).max(0) as u32;
        let rel_y = (region.y - my).max(0) as u32;
        if rel_x >= img.width() || rel_y >= img.height() {
            return Err("Verification region is outside the selected monitor".to_string());
        }
        let width = region.width.min(img.width().saturating_sub(rel_x)).max(1);
        let height = region.height.min(img.height().saturating_sub(rel_y)).max(1);
        let cropped = image::imageops::crop_imm(&img, rel_x, rel_y, width, height).to_image();
        let raw = cropped.as_raw().to_vec();
        let png = encode_image_to_png(&cropped)?;
        return Ok((
            raw,
            png,
            cropped.width(),
            cropped.height(),
            format!(
                " region ({},{}) {}x{}",
                mx + rel_x as i32,
                my + rel_y as i32,
                cropped.width(),
                cropped.height()
            ),
        ));
    }

    let width = img.width();
    let height = img.height();
    let raw = img.as_raw().to_vec();
    let png = encode_image_to_png(&img)?;
    Ok((raw, png, width, height, String::new()))
}

fn prepare_screen_verification(
    args: &Value,
    region: Option<VerificationRegion>,
) -> Result<Option<ScreenVerificationContext>, NativeToolResult> {
    let expected_text = args.get("verify_text").and_then(|v| v.as_str()).map(|s| s.to_string());
    let verify = args
        .get("verify_screen_change")
        .map(|v| parse_bool(v, false))
        .unwrap_or(false)
        || expected_text.is_some(); // verify_text implicitly enables verification
    if !verify {
        return Ok(None);
    }

    let threshold_pct = args
        .get("verify_threshold_pct")
        .and_then(parse_float)
        .unwrap_or(0.5)
        .clamp(0.01, 100.0);
    let timeout_ms = args
        .get("verify_timeout_ms")
        .and_then(parse_int)
        .unwrap_or(1200)
        .clamp(100, 10_000) as u64;
    let poll_ms = args
        .get("verify_poll_ms")
        .and_then(parse_int)
        .unwrap_or(150)
        .clamp(50, 1000) as u64;
    let verification_region = action_verification_region_from_args(args).or(region);

    let (baseline_raw, baseline_png, _, _, _) = match capture_screen_state(verification_region) {
        Ok(state) => state,
        Err(e) => return Err(tool_error("desktop_verification", e)),
    };
    update_screenshot_cache(baseline_raw.clone(), baseline_png);

    Ok(Some(ScreenVerificationContext {
        baseline_raw,
        threshold_pct,
        timeout_ms,
        poll_ms,
        region: verification_region,
        expected_text,
    }))
}

fn finalize_action_result(
    summary: String,
    delay_ms: u64,
    do_screenshot: bool,
    verification: Option<ScreenVerificationContext>,
) -> NativeToolResult {
    if let Some(verification) = verification {
        if let Err(e) = interruptible_sleep(std::time::Duration::from_millis(delay_ms)) {
            return tool_error("verification", e);
        }

        let start = std::time::Instant::now();
        let (raw, png, width, height, diff_pct, elapsed_ms, region_desc) = loop {
            match capture_screen_state(verification.region) {
                Ok((raw, png, width, height, region_desc)) => {
                    let diff_pct = pixel_diff_pct(&verification.baseline_raw, &raw);
                    let elapsed_ms = start.elapsed().as_millis();
                    let passed = diff_pct >= verification.threshold_pct;
                    if passed || elapsed_ms as u64 >= verification.timeout_ms {
                        break (raw, png, width, height, diff_pct, elapsed_ms, region_desc);
                    }
                }
                Err(e) => return tool_error("desktop_verification", e),
            }

            if let Err(e) =
                interruptible_sleep(std::time::Duration::from_millis(verification.poll_ms))
            {
                return tool_error("desktop_verification", e);
            }
        };
        // Clone raw pixels before cache takes ownership (needed for OCR verification below)
        let raw_for_ocr = if verification.expected_text.is_some() {
            Some(raw.clone())
        } else {
            None
        };
        update_screenshot_cache(raw, png.clone());

        let verification_text = if diff_pct >= verification.threshold_pct {
            format!(
                "Verified screen change: {:.2}% observed after {}ms (threshold {:.2}%).",
                diff_pct, elapsed_ms, verification.threshold_pct
            )
        } else {
            format!(
                "Verification failed: expected screen change >= {:.2}%, observed {:.2}% after {}ms.",
                verification.threshold_pct, diff_pct, elapsed_ms
            )
        };
        let mut text = format!(
            "{summary}. {verification_text} Screenshot {width}x{height}{region_desc}"
        );

        // OCR verification: if expected_text was set, OCR the final screenshot to confirm
        if let Some(ref expected) = verification.expected_text {
            let img = raw_for_ocr.and_then(|raw| image::RgbaImage::from_raw(width, height, raw));
            if let Some(img) = img {
                let search = expected.to_lowercase();
                let found = spawn_with_timeout(std::time::Duration::from_secs(10), move || {
                    ocr_tools::ocr_png_and_search(&img, &search)
                });
                match found {
                    Ok(true) => {
                        text.push_str(&format!(" Text '{}' confirmed via OCR.", expected));
                    }
                    Ok(false) => {
                        text.push_str(&format!(" WARNING: text '{}' not found via OCR.", expected));
                    }
                    Err(e) => {
                        text.push_str(&format!(" WARNING: OCR verification failed: {e}"));
                    }
                }
            }
        }

        if do_screenshot || verification.threshold_pct > 0.0 {
            NativeToolResult::with_image(text, png)
        } else {
            NativeToolResult::text_only(text)
        }
    } else if do_screenshot {
        let mut result = capture_post_action_screenshot(delay_ms);
        result.text = format!("{summary}. {}", result.text);
        result
    } else {
        NativeToolResult::text_only(summary)
    }
}

fn summarize_value_for_trace(value: &Value) -> Value {
    match value {
        Value::String(s) => {
            const MAX_LEN: usize = 200;
            if s.len() > MAX_LEN {
                Value::String(format!("{}...", &s[..MAX_LEN]))
            } else {
                Value::String(s.clone())
            }
        }
        Value::Array(items) => Value::Array(items.iter().take(10).map(summarize_value_for_trace).collect()),
        Value::Object(map) => {
            let summarized = map
                .iter()
                .map(|(k, v)| (k.clone(), summarize_value_for_trace(v)))
                .collect();
            Value::Object(summarized)
        }
        _ => value.clone(),
    }
}

fn write_desktop_trace_line(entry: &Value) {
    let path = desktop_trace_path();
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    use std::io::Write;
    let _ = writeln!(file, "{line}");
}

fn finalize_desktop_result(name: &str, args: &Value, started_at: std::time::Instant, mut result: NativeToolResult) -> NativeToolResult {
    let status = classify_desktop_result_status(&result.text);
    let duration_ms = started_at.elapsed().as_millis() as u64;
    let summary = serde_json::json!({
        "tool": name,
        "status": status,
        "duration_ms": duration_ms,
        "image_count": result.images.len(),
    });

    if !result.text.is_empty() {
        result.text.push_str("\n\n[desktop_result] ");
        result.text.push_str(&summary.to_string());
    } else {
        result.text = format!("[desktop_result] {}", summary);
    }

    write_desktop_trace_line(&serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "tool": name,
        "status": status,
        "duration_ms": duration_ms,
        "image_count": result.images.len(),
        "args": summarize_value_for_trace(args),
        "text_preview": result.text.lines().take(8).collect::<Vec<_>>().join("\n"),
    }));

    result
}

/// Helper: take a screenshot after an action with configurable cache parameters.
/// `cache_max_age_ms` — how old a cached screenshot can be before it's considered stale.
/// `cache_threshold_pct` — pixel-diff percentage below which the screen is "unchanged".
fn capture_post_action_screenshot_ext(
    delay_ms: u64,
    cache_max_age_ms: u64,
    cache_threshold_pct: f64,
) -> NativeToolResult {
    if delay_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }

    // Capture via xcap directly so we can access raw pixels
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => {
            return tool_error("screenshot", format!("Failed to enumerate monitors: {e}"))
        }
    };
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or(monitors.first());
    let monitor = match monitor {
        Some(m) => m,
        None => return tool_error("screenshot", "No monitors detected"),
    };
    let img = match monitor.capture_image() {
        Ok(i) => i,
        Err(e) => {
            return tool_error("screenshot", format!("Screenshot capture failed: {e}"))
        }
    };

    let width = img.width();
    let height = img.height();
    let new_raw = img.as_raw().to_vec();

    // Check cache: if recent screenshot exists and screen is basically unchanged, reuse it
    if let Some((cached_raw, cached_png)) = get_cached_screenshot(cache_max_age_ms) {
        let diff = pixel_diff_pct(&cached_raw, &new_raw);
        if diff < cache_threshold_pct {
            return NativeToolResult::with_image(
                format!("Screenshot {}x{} (unchanged)", width, height),
                (*cached_png).clone(),
            );
        }
    }

    // Screen changed — encode new PNG and update cache
    let png_bytes = match encode_image_to_png(&img) {
        Ok(b) => b,
        Err(e) => return tool_error("screenshot", e),
    };
    update_screenshot_cache(new_raw, png_bytes.clone());

    NativeToolResult::with_image(
        format!("Screenshot {}x{}", width, height),
        png_bytes,
    )
}

/// Helper: take a screenshot after an action with optional delay.
/// Uses a smart cache: if the screen hasn't changed significantly (<5% pixel diff),
/// returns the cached image with an "(unchanged)" note to save tokens.
fn capture_post_action_screenshot(delay_ms: u64) -> NativeToolResult {
    capture_post_action_screenshot_ext(delay_ms, 2000, 5.0)
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
pub(super) fn parse_timeout(args: &Value) -> std::time::Duration {
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
pub(super) fn with_enigo<F, T>(f: F) -> Result<T, String>
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
pub(super) fn validate_coordinates(x: i32, y: i32) -> Result<(), String> {
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
pub(super) fn snap_coordinates(x: i32, y: i32) -> (i32, i32) {
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
pub(super) fn apply_dpi_scaling(x: i32, y: i32, dpi_aware: bool) -> (i32, i32) {
    if dpi_aware {
        #[cfg(windows)]
        let scale = win32::get_system_dpi_scale();
        #[cfg(target_os = "macos")]
        let scale = macos::get_system_dpi_scale();
        #[cfg(target_os = "linux")]
        let scale = linux::get_system_dpi_scale();
        ((x as f64 * scale) as i32, (y as f64 * scale) as i32)
    } else {
        (x, y)
    }
}

/// Check if the foreground window is blocked by a modal dialog.
/// Returns a warning string if blocked, None otherwise.
#[cfg(windows)]
pub(super) fn check_modal_dialog() -> Option<String> {
    let fg = unsafe { win32::GetForegroundWindow() };
    if fg != 0 {
        if let Some(_popup) = win32::is_window_blocked(fg) {
            return Some(
                "Warning: foreground window is blocked by a modal dialog. ".to_string(),
            );
        }
    }
    None
}

#[cfg(not(windows))]
pub(super) fn check_modal_dialog() -> Option<String> {
    None // Modal dialog detection not available on macOS/Linux
}

/// Type text using Win32 SendInput with KEYEVENTF_UNICODE (IME/Unicode fallback).
#[cfg(windows)]
fn type_text_via_send_input(text: &str) -> Result<(), String> {
    for ch in text.chars() {
        let code = ch as u16;
        let down = win32::INPUT {
            input_type: win32::INPUT_KEYBOARD,
            ki: win32::KEYBDINPUT {
                w_vk: 0,
                w_scan: code,
                dw_flags: win32::KEYEVENTF_UNICODE,
                time: 0,
                dw_extra_info: 0,
            },
            _pad: [0; 8],
        };
        let up = win32::INPUT {
            input_type: win32::INPUT_KEYBOARD,
            ki: win32::KEYBDINPUT {
                w_vk: 0,
                w_scan: code,
                dw_flags: win32::KEYEVENTF_UNICODE | win32::KEYEVENTF_KEYUP,
                time: 0,
                dw_extra_info: 0,
            },
            _pad: [0; 8],
        };
        let inputs = [down, up];
        let sent = unsafe {
            win32::SendInput(2, inputs.as_ptr(), std::mem::size_of::<win32::INPUT>() as i32)
        };
        if sent != 2 {
            return Err(format!("SendInput failed for character '{ch}'"));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Ok(())
}

#[cfg(windows)]
fn windows_keyboard_lang_id() -> u32 {
    extern "system" {
        fn GetKeyboardLayout(thread_id: u32) -> isize;
    }

    let layout = unsafe { GetKeyboardLayout(0) } as u32;
    layout & 0xFFFF
}

#[cfg(windows)]
fn should_prefer_unicode_input(args: &Value, text: &str) -> bool {
    match args.get("method").and_then(|v| v.as_str()) {
        Some("unicode") => return true,
        Some("enigo") => return false,
        _ => {}
    }

    !text.is_ascii() || windows_keyboard_lang_id() != 0x0409
}

// ─── Screenshot cache ────────────────────────────────────────────────────────

thread_local! {
    /// (timestamp, raw_rgba_bytes, png_bytes)
    static LAST_SCREENSHOT: RefCell<Option<(std::time::Instant, Arc<Vec<u8>>, Arc<Vec<u8>>)>> = RefCell::new(None);
}

/// Get a cached screenshot if it was taken within `max_age_ms` milliseconds.
/// Returns (raw_rgba_bytes, png_bytes).
pub(super) fn get_cached_screenshot(max_age_ms: u64) -> Option<(Arc<Vec<u8>>, Arc<Vec<u8>>)> {
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
pub(super) fn update_screenshot_cache(raw: Vec<u8>, png: Vec<u8>) {
    LAST_SCREENSHOT.with(|cell| {
        *cell.borrow_mut() = Some((std::time::Instant::now(), Arc::new(raw), Arc::new(png)));
    });
}

/// Wait for the screen to stop changing (useful after animations/transitions).
/// Returns true if screen settled within timeout, false if timed out.
#[allow(dead_code)]
pub(super) fn wait_for_screen_settle(timeout_ms: u64, poll_ms: u64) -> bool {
    let start = std::time::Instant::now();
    let mut prev: Option<Vec<u8>> = None;
    while start.elapsed().as_millis() < timeout_ms as u128 {
        if desktop_call_cancelled() {
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
        if interruptible_sleep(std::time::Duration::from_millis(poll_ms)).is_err() {
            return false;
        }
    }
    false
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
fn str_to_key(s: &str) -> Result<Key, String> {
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

/// Click the mouse at absolute screen coordinates.
pub fn tool_click_screen(args: &Value) -> NativeToolResult {
    let mut x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("click_screen", "'x' coordinate is required"),
    };
    let mut y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("click_screen", "'y' coordinate is required"),
    };
    // DPI scaling: convert logical coordinates to physical if requested
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    {
        let dpi_aware = args.get("dpi_aware").map(|v| parse_bool(v, false)).unwrap_or(false);
        let scaled = apply_dpi_scaling(x, y, dpi_aware);
        x = scaled.0;
        y = scaled.1;
    }
    let snap = args.get("snap_to_screen").map(|v| parse_bool(v, false)).unwrap_or(false);
    if snap {
        let snapped = snap_coordinates(x, y);
        x = snapped.0;
        y = snapped.1;
    }
    if let Err(e) = validate_coordinates(x, y) {
        return tool_error("click_screen", e);
    }
    // Check for modal dialog blocking the foreground window
    let modal_warning = check_modal_dialog().unwrap_or_default();

    let button_str = args
        .get("button")
        .and_then(|v| v.as_str())
        .unwrap_or("left");
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(500) as u64;
    let verification_region =
        normalize_verification_region(x - 160, y - 160, 320, 320);
    let verification = match prepare_screen_verification(args, verification_region) {
        Ok(v) => v,
        Err(result) => return result,
    };

    if let Err(e) = with_enigo(|enigo| {
        enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| format!("move_mouse failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        match button_str {
            "left" => enigo
                .button(Button::Left, Direction::Click)
                .map_err(|e| format!("click failed: {e}")),
            "right" => enigo
                .button(Button::Right, Direction::Click)
                .map_err(|e| format!("click failed: {e}")),
            "middle" => enigo
                .button(Button::Middle, Direction::Click)
                .map_err(|e| format!("click failed: {e}")),
            "double" => {
                enigo
                    .button(Button::Left, Direction::Click)
                    .map_err(|e| format!("click failed: {e}"))?;
                std::thread::sleep(std::time::Duration::from_millis(50));
                enigo
                    .button(Button::Left, Direction::Click)
                    .map_err(|e| format!("double-click failed: {e}"))
            }
            other => Err(format!(
                "Unknown button '{other}'. Use: left, right, middle, double"
            )),
        }
    }) {
        return tool_error("click_screen", e);
    }

    finalize_action_result(
        format!("{modal_warning}Clicked {button_str} at ({x}, {y})"),
        delay_ms,
        true,
        verification,
    )
}

/// Type text using keyboard simulation.
pub fn tool_type_text(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_owned(),
        None => {
            return tool_error("type_text", "'text' argument is required")
        }
    };
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(300) as u64;
    let retries = args
        .get("retry")
        .and_then(parse_int)
        .unwrap_or(0)
        .max(0)
        .min(3) as u32;
    let verification = match prepare_screen_verification(args, None) {
        Ok(v) => v,
        Err(result) => return result,
    };

    #[cfg(windows)]
    let prefer_unicode_input = should_prefer_unicode_input(args, &text);

    let text_clone = text.clone();
    let type_result = screenshot_tools::retry_on_failure(retries, 200, move || {
        #[cfg(windows)]
        if prefer_unicode_input {
            return type_text_via_send_input(&text_clone);
        }

        let res = with_enigo(|enigo| {
            enigo
                .text(&text_clone)
                .map_err(|e| format!("type_text failed: {e}"))
        });
        #[cfg(windows)]
        let res = res.or_else(|_| type_text_via_send_input(&text_clone));
        res
    });
    if let Err(e) = type_result {
        return tool_error("type_text", e);
    }

    #[allow(unused_mut)]
    let mut summary = if text.len() > 50 {
        format!("Typed {} characters: \"{}...\"", text.len(), &text[..50])
    } else {
        format!("Typed: \"{}\"", text)
    };

    // Warn if non-US keyboard layout detected (characters may differ from intent)
    #[cfg(windows)]
    {
        let lang_id = windows_keyboard_lang_id();
        if lang_id != 0x0409 {
            // 0x0409 = English (United States)
            summary.push_str(&format!(
                " (note: keyboard layout 0x{:04X}, not US-QWERTY)",
                lang_id
            ));
        }
        if prefer_unicode_input {
            summary.push_str(" (typed via Unicode input)");
        }
    }

    finalize_action_result(summary, delay_ms, do_screenshot, verification)
}

/// Press a key or key combination.
pub fn tool_press_key(args: &Value) -> NativeToolResult {
    let key_str = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => {
            return tool_error("press_key", "'key' argument is required")
        }
    };
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(500) as u64;
    let retries = args
        .get("retry")
        .and_then(parse_int)
        .unwrap_or(0)
        .max(0)
        .min(3) as u32;
    let verification = match prepare_screen_verification(args, None) {
        Ok(v) => v,
        Err(result) => return result,
    };

    let (modifiers, main_key) = match parse_key_combo(key_str) {
        Ok(combo) => combo,
        Err(e) => return tool_error("press_key", e),
    };

    let modifiers_clone = modifiers.clone();
    if let Err(e) = screenshot_tools::retry_on_failure(retries, 200, move || {
        with_enigo(|enigo| {
            for modifier in &modifiers_clone {
                enigo
                    .key(*modifier, Direction::Press)
                    .map_err(|e| format!("key press failed: {e}"))?;
            }
            let result = enigo.key(main_key, Direction::Click);
            // Always release modifiers
            for modifier in modifiers_clone.iter().rev() {
                let _ = enigo.key(*modifier, Direction::Release);
            }
            result.map_err(|e| format!("key press failed: {e}"))
        })
    }) {
        return tool_error("press_key", e);
    }

    let summary = format!("Pressed: {key_str}");

    finalize_action_result(summary, delay_ms, do_screenshot, verification)
}

/// Move the mouse cursor without clicking.
pub fn tool_move_mouse(args: &Value) -> NativeToolResult {
    let mut x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => {
            return tool_error("move_mouse", "'x' coordinate is required")
        }
    };
    let mut y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => {
            return tool_error("move_mouse", "'y' coordinate is required")
        }
    };
    #[cfg(windows)]
    {
        let dpi_aware = args.get("dpi_aware").map(|v| parse_bool(v, false)).unwrap_or(false);
        let scaled = apply_dpi_scaling(x, y, dpi_aware);
        x = scaled.0;
        y = scaled.1;
    }
    let snap = args.get("snap_to_screen").map(|v| parse_bool(v, false)).unwrap_or(false);
    if snap {
        let snapped = snap_coordinates(x, y);
        x = snapped.0;
        y = snapped.1;
    }
    if let Err(e) = validate_coordinates(x, y) {
        return tool_error("move_mouse", e);
    }

    if let Err(e) = with_enigo(|enigo| {
        enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| format!("move_mouse failed: {e}"))
    }) {
        return tool_error("move_mouse", e);
    }

    NativeToolResult::text_only(format!("Mouse moved to ({x}, {y})"))
}

/// Scroll repeatedly until OCR finds the target text on screen.
fn scroll_to_text(args: &Value) -> NativeToolResult {
    let target_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return tool_error("scroll_screen", "'text' is required for mode='to_text'"),
    };
    let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("down");
    let max_scrolls = args
        .get("max_scrolls")
        .and_then(parse_int)
        .unwrap_or(20)
        .min(50) as usize;
    let scroll_amount = if direction == "up" { -3 } else { 3 };
    let target_lower = target_text.to_lowercase();

    for i in 0..max_scrolls {
        if let Err(e) = ensure_desktop_not_cancelled() {
            return tool_error("scroll_screen", e);
        }

        // OCR current screen
        let ocr_result = ocr_tools::tool_ocr_screen(&serde_json::json!({"monitor": 0}));
        if ocr_result.text.to_lowercase().contains(&target_lower) {
            let screenshot = capture_post_action_screenshot(0);
            return NativeToolResult {
                text: format!("Found '{}' after {} scroll(s)", target_text, i),
                images: screenshot.images,
            };
        }
        // Scroll
        with_enigo(|enigo| {
            enigo
                .scroll(scroll_amount, enigo::Axis::Vertical)
                .map_err(|e| format!("{e}"))
        })
        .ok();
        if let Err(e) = interruptible_sleep(std::time::Duration::from_millis(400)) {
            return tool_error("scroll_screen", e);
        }
    }

    let screenshot = capture_post_action_screenshot(0);
    NativeToolResult {
        text: format!(
            "Text '{}' not found after {} scrolls {}",
            target_text, max_scrolls, direction
        ),
        images: screenshot.images,
    }
}

/// Scroll the mouse wheel at the current or specified position.
///
/// Supports two modes via the `mode` parameter:
/// - `"amount"` (default): scroll by a fixed number of units.
/// - `"to_text"`: scroll until OCR finds the specified `text` on screen.
pub fn tool_scroll_screen(args: &Value) -> NativeToolResult {
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("amount");
    if mode == "to_text" {
        return scroll_to_text(args);
    }

    let amount = match args.get("amount").and_then(parse_int) {
        Some(v) => v as i32,
        None => {
            return tool_error("scroll_screen", "'amount' argument is required (positive=down, negative=up)")
        }
    };
    let horizontal = args
        .get("horizontal")
        .map(|v| parse_bool(v, false))
        .unwrap_or(false);
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(300) as u64;
    let verification_region = if let (Some(x), Some(y)) = (
        args.get("x").and_then(parse_int),
        args.get("y").and_then(parse_int),
    ) {
        normalize_verification_region(x as i32 - 180, y as i32 - 180, 360, 360)
    } else {
        None
    };
    let verification = match prepare_screen_verification(args, verification_region) {
        Ok(v) => v,
        Err(result) => return result,
    };

    if let Err(e) = with_enigo(|enigo| {
        if let (Some(x), Some(y)) = (
            args.get("x").and_then(parse_int),
            args.get("y").and_then(parse_int),
        ) {
            let (mut x, mut y) = (x as i32, y as i32);
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            {
                let dpi_aware = args
                    .get("dpi_aware")
                    .map(|v| parse_bool(v, false))
                    .unwrap_or(false);
                let scaled = apply_dpi_scaling(x, y, dpi_aware);
                x = scaled.0;
                y = scaled.1;
            }
            if args
                .get("snap_to_screen")
                .map(|v| parse_bool(v, false))
                .unwrap_or(false)
            {
                let snapped = snap_coordinates(x, y);
                x = snapped.0;
                y = snapped.1;
            }
            validate_coordinates(x, y)?;
            enigo
                .move_mouse(x, y, Coordinate::Abs)
                .map_err(|e| format!("move_mouse failed: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let axis = if horizontal {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        enigo
            .scroll(amount, axis)
            .map_err(|e| format!("scroll failed: {e}"))
    }) {
        return tool_error("scroll_screen", e);
    }

    let direction = if horizontal {
        if amount > 0 {
            "right"
        } else {
            "left"
        }
    } else if amount > 0 {
        "down"
    } else {
        "up"
    };
    let summary = format!("Scrolled {direction} by {} units", amount.abs());

    finalize_action_result(summary, delay_ms, do_screenshot, verification)
}

// ─── Submodules ──────────────────────────────────────────────────────────────

#[cfg(windows)]
pub(crate) mod win32;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "linux")]
pub(crate) mod linux;

mod gpu_app_db;
mod window_tools;
pub use window_tools::*;

mod app_script_tools;
pub use app_script_tools::*;

mod screenshot_tools;
pub use screenshot_tools::*;
mod ocr_tools;
pub use ocr_tools::*;
mod ui_automation_tools;
pub use ui_automation_tools::*;
mod clipboard_tools;
pub use clipboard_tools::*;

mod compound_tools;
pub use compound_tools::*;

mod image_tools;
#[allow(unused_imports)]
pub use image_tools::*;
mod input_tools;
pub use input_tools::*;
mod dialog_tools;
pub use dialog_tools::*;
mod form_tools;
pub use form_tools::*;
mod display_tools;
pub use display_tools::*;
mod annotation_tools;
pub use annotation_tools::*;
mod system_tools;
pub use system_tools::*;
mod overlay_tools;
pub use overlay_tools::*;
mod audio_tools;
pub use audio_tools::*;
mod recording_tools;
pub use recording_tools::*;

/// Dispatch a desktop tool by name. Returns `None` if the tool name is not recognized.
/// Used by the MCP server binary to route tool calls to existing implementations.
#[allow(dead_code)]
pub fn dispatch_desktop_tool(name: &str, args: &Value) -> Option<NativeToolResult> {
    let started_at = std::time::Instant::now();
    let result = match name {
        // Core input tools (mod.rs)
        "click_screen" => tool_click_screen(args),
        "type_text" => tool_type_text(args),
        "press_key" => tool_press_key(args),
        "move_mouse" => tool_move_mouse(args),
        "scroll_screen" => tool_scroll_screen(args),
        "mouse_drag" => tool_mouse_drag(args),
        "mouse_button" => tool_mouse_button(args),

        // Input tools (input_tools.rs)
        "paste" => tool_paste(args),
        "clear_field" => tool_clear_field(args),
        "hover_element" => tool_hover_element(args),

        // Screenshot & OCR (ui_tools.rs)
        "take_screenshot" => super::native_tools::tool_take_screenshot_with_image(args),
        "screenshot_region" => tool_screenshot_region(args),
        "screenshot_diff" => tool_screenshot_diff(args),
        "window_screenshot" => tool_window_screenshot(args),
        "wait_for_screen_change" => tool_wait_for_screen_change(args),
        "ocr_screen" => tool_ocr_screen(args),
        "ocr_find_text" => tool_ocr_find_text(args),
        "get_ui_tree" => tool_get_ui_tree(args),
        "click_ui_element" => tool_click_ui_element(args),
        "invoke_ui_action" => tool_invoke_ui_action(args),
        "read_ui_element_value" => tool_read_ui_element_value(args),
        "wait_for_ui_element" => tool_wait_for_ui_element(args),
        "clipboard_image" => tool_clipboard_image(args),
        "find_ui_elements" => tool_find_ui_elements(args),

        // Window tools (window_tools.rs)
        "list_windows" => tool_list_windows(args),
        "get_active_window" => tool_get_active_window(args),
        "focus_window" => tool_focus_window(args),
        "minimize_window" => tool_minimize_window(args),
        "maximize_window" => tool_maximize_window(args),
        "close_window" => tool_close_window(args),
        "resize_window" => tool_resize_window(args),
        "wait_for_window" => tool_wait_for_window(args),
        "click_window_relative" => tool_click_window_relative(args),
        "snap_window" => tool_snap_window(args),
        "set_window_topmost" => tool_set_window_topmost(args),
        "open_application" => tool_open_application(args),
        "list_processes" => tool_list_processes(args),
        "kill_process" => tool_kill_process(args),
        "send_keys_to_window" => tool_send_keys_to_window(args),
        "switch_virtual_desktop" => tool_switch_virtual_desktop(args),
        "get_process_info" => tool_get_process_info(args),
        "read_clipboard" => tool_read_clipboard(args),
        "write_clipboard" => tool_write_clipboard(args),
        "get_cursor_position" => tool_get_cursor_position(args),
        "get_pixel_color" => tool_get_pixel_color(args),
        "list_monitors" => tool_list_monitors(args),

        // Compound tools (compound_tools.rs)
        "find_and_click_text" => tool_find_and_click_text(args),
        "type_into_element" => tool_type_into_element(args),
        "get_window_text" => tool_get_window_text(args),
        "file_dialog_navigate" => tool_file_dialog_navigate(args),
        "drag_and_drop_element" => tool_drag_and_drop_element(args),
        "wait_for_text_on_screen" => tool_wait_for_text_on_screen(args),
        "get_context_menu" => tool_get_context_menu(args),
        "scroll_element" => tool_scroll_element(args),
        "smart_wait" => tool_smart_wait(args),
        "click_and_verify" => tool_click_and_verify(args),

        // Dialog & form tools
        "handle_dialog" => tool_handle_dialog(args),
        "wait_for_element_state" => tool_wait_for_element_state(args),
        "fill_form" => tool_fill_form(args),
        "run_action_sequence" => tool_run_action_sequence(args),

        // Display tools
        "move_to_monitor" => tool_move_to_monitor(args),
        "set_window_opacity" => tool_set_window_opacity(args),
        "highlight_point" => tool_highlight_point(args),

        // Annotation & image tools
        "annotate_screenshot" => tool_annotate_screenshot(args),
        "ocr_region" => tool_ocr_region(args),
        "find_color_on_screen" => tool_find_color_on_screen(args),
        "find_image_on_screen" => tool_find_image_on_screen(args),

        // System tools
        "read_registry" => tool_read_registry(args),
        "click_tray_icon" => tool_click_tray_icon(args),
        "watch_window" => tool_watch_window(args),

        // App scripting
        "execute_app_script" => tool_execute_app_script(args),

        // Notifications
        "send_notification" => tool_send_notification(args),

        // Status overlay
        "show_status_overlay" => tool_show_status_overlay(args),
        "update_status_overlay" => tool_update_status_overlay(args),
        "hide_status_overlay" => tool_hide_status_overlay(args),

        // Audio tools
        "get_system_volume" => tool_get_system_volume(args),
        "set_system_volume" => tool_set_system_volume(args),
        "set_system_mute" => tool_set_system_mute(args),
        "list_audio_devices" => tool_list_audio_devices(args),

        // Extended clipboard
        "clear_clipboard" => tool_clear_clipboard(args),
        "clipboard_file_paths" => tool_clipboard_file_paths(args),
        "clipboard_html" => tool_clipboard_html(args),

        // Window layout
        "save_window_layout" => tool_save_window_layout(args),
        "restore_window_layout" => tool_restore_window_layout(args),

        // Process monitoring
        "wait_for_process_exit" => tool_wait_for_process_exit(args),
        "get_process_tree" => tool_get_process_tree(args),
        "get_system_metrics" => tool_get_system_metrics(args),

        // Notifications
        "wait_for_notification" => tool_wait_for_notification(args),
        "dismiss_all_notifications" => tool_dismiss_all_notifications(args),

        // Screen recording
        "start_screen_recording" => tool_start_screen_recording(args),
        "stop_screen_recording" => tool_stop_screen_recording(args),
        "capture_gif" => tool_capture_gif(args),

        // Dialog auto-handler
        "dialog_handler_start" => tool_dialog_handler_start(args),
        "dialog_handler_stop" => tool_dialog_handler_stop(args),

        _ => return None,
    };

    Some(finalize_desktop_result(name, args, started_at, result))
}

/// Drag the mouse from one position to another.
/// When `steps` > 1, interpolates intermediate positions with linear lerp for smooth dragging.
pub fn tool_mouse_drag(args: &Value) -> NativeToolResult {
    let x1 = match args.get("from_x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("mouse_drag", "'from_x' is required"),
    };
    let y1 = match args.get("from_y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("mouse_drag", "'from_y' is required"),
    };
    let x2 = match args.get("to_x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("mouse_drag", "'to_x' is required"),
    };
    let y2 = match args.get("to_y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("mouse_drag", "'to_y' is required"),
    };
    if let Err(e) = validate_coordinates(x1, y1) {
        return tool_error("mouse_drag", format!("start {e}"));
    }
    if let Err(e) = validate_coordinates(x2, y2) {
        return tool_error("mouse_drag", format!("end {e}"));
    }
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(500) as u64;
    let steps = args
        .get("steps")
        .and_then(parse_int)
        .unwrap_or(1)
        .max(1)
        .min(100) as u32;
    let min_x = x1.min(x2) - 80;
    let min_y = y1.min(y2) - 80;
    let width = (x1.max(x2) - min_x + 80) as u32;
    let height = (y1.max(y2) - min_y + 80) as u32;
    let verification_region = normalize_verification_region(min_x, min_y, width, height);
    let verification = match prepare_screen_verification(args, verification_region) {
        Ok(v) => v,
        Err(result) => return result,
    };

    if let Err(e) = with_enigo(|enigo| {
        enigo
            .move_mouse(x1, y1, Coordinate::Abs)
            .map_err(|e| format!("move to start failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        enigo
            .button(Button::Left, Direction::Press)
            .map_err(|e| format!("mouse down failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(50));

        if steps > 1 {
            // Smooth interpolation: move through intermediate positions
            for i in 1..steps {
                let t = i as f64 / steps as f64;
                let ix = x1 as f64 + (x2 as f64 - x1 as f64) * t;
                let iy = y1 as f64 + (y2 as f64 - y1 as f64) * t;
                let move_result = enigo.move_mouse(ix as i32, iy as i32, Coordinate::Abs);
                if move_result.is_err() {
                    let _ = enigo.button(Button::Left, Direction::Release);
                    return move_result.map_err(|e| format!("interpolated move failed: {e}"));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        // Final move to exact destination
        let move_result = enigo.move_mouse(x2, y2, Coordinate::Abs);
        if move_result.is_err() {
            let _ = enigo.button(Button::Left, Direction::Release);
        }
        move_result.map_err(|e| format!("move to end failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        enigo
            .button(Button::Left, Direction::Release)
            .map_err(|e| format!("mouse up failed: {e}"))
    }) {
        return tool_error("mouse_drag", e);
    }

    let steps_note = if steps > 1 {
        format!(" ({steps} steps)")
    } else {
        String::new()
    };
    finalize_action_result(
        format!("Dragged from ({x1},{y1}) to ({x2},{y2}){steps_note}"),
        delay_ms,
        true,
        verification,
    )
}

/// Press or release a mouse button independently (for hold-and-drag scenarios).
pub fn tool_mouse_button(args: &Value) -> NativeToolResult {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => {
            return tool_error("mouse_button", "'action' is required (press or release)")
        }
    };
    let button_str = args
        .get("button")
        .and_then(|v| v.as_str())
        .unwrap_or("left");
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);

    let button = match button_str {
        "left" => Button::Left,
        "right" => Button::Right,
        "middle" => Button::Middle,
        other => {
            return tool_error("mouse_button", format!("Unknown button '{other}'. Use: left, right, middle"))
        }
    };
    let direction = match action {
        "press" => Direction::Press,
        "release" => Direction::Release,
        other => {
            return tool_error("mouse_button", format!("Unknown action '{other}'. Use: press, release"))
        }
    };

    if let Err(e) = with_enigo(|enigo| {
        enigo
            .button(button, direction)
            .map_err(|e| format!("mouse button failed: {e}"))
    }) {
        return tool_error("mouse_button", e);
    }

    let past = if action == "press" {
        "pressed"
    } else {
        "released"
    };
    let summary = format!("Mouse {button_str} button {past}");
    if do_screenshot {
        let mut result = capture_post_action_screenshot(300);
        result.text = format!("{summary}. {}", result.text);
        result
    } else {
        NativeToolResult::text_only(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_combo_single() {
        let (mods, key) = parse_key_combo("enter").unwrap();
        assert!(mods.is_empty());
        assert!(matches!(key, Key::Return));
    }

    #[test]
    fn test_parse_key_combo_with_modifier() {
        let (mods, key) = parse_key_combo("ctrl+c").unwrap();
        assert_eq!(mods.len(), 1);
        assert!(matches!(mods[0], Key::Control));
        assert!(matches!(key, Key::Unicode('c')));
    }

    #[test]
    fn test_parse_key_combo_multiple_modifiers() {
        let (mods, key) = parse_key_combo("ctrl+shift+s").unwrap();
        assert_eq!(mods.len(), 2);
        assert!(matches!(mods[0], Key::Control));
        assert!(matches!(mods[1], Key::Shift));
        assert!(matches!(key, Key::Unicode('s')));
    }

    #[test]
    fn test_parse_key_combo_fkey() {
        let (mods, key) = parse_key_combo("alt+f4").unwrap();
        assert_eq!(mods.len(), 1);
        assert!(matches!(mods[0], Key::Alt));
        assert!(matches!(key, Key::F4));
    }

    #[test]
    fn test_parse_key_combo_unknown() {
        assert!(parse_key_combo("ctrl+unknownkey").is_err());
    }

    #[test]
    fn test_str_to_key_special() {
        assert!(matches!(str_to_key("tab"), Ok(Key::Tab)));
        assert!(matches!(str_to_key("escape"), Ok(Key::Escape)));
        assert!(matches!(str_to_key("backspace"), Ok(Key::Backspace)));
        assert!(matches!(str_to_key("space"), Ok(Key::Space)));
    }

    #[test]
    fn test_str_to_key_char() {
        assert!(matches!(str_to_key("a"), Ok(Key::Unicode('a'))));
        assert!(matches!(str_to_key("1"), Ok(Key::Unicode('1'))));
    }

    #[test]
    fn test_validate_coordinates_on_screen() {
        // (0,0) should always be valid — it's the top-left of the primary monitor
        assert!(validate_coordinates(0, 0).is_ok());
    }

    #[test]
    fn test_validate_coordinates_off_screen() {
        // Extremely negative coords should be invalid
        assert!(validate_coordinates(-99999, -99999).is_err());
    }

    #[test]
    fn test_with_enigo_caches_instance() {
        // First call creates the instance
        let r1 = with_enigo(|_e| Ok::<_, String>(42));
        assert_eq!(r1.unwrap(), 42);
        // Second call reuses it (no error from re-init)
        let r2 = with_enigo(|_e| Ok::<_, String>(99));
        assert_eq!(r2.unwrap(), 99);
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    #[test]
    fn test_dpi_scaling_noop_when_false() {
        let (x, y) = apply_dpi_scaling(100, 200, false);
        assert_eq!((x, y), (100, 200));
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    #[test]
    fn test_dpi_scaling_applies_when_true() {
        let (x, y) = apply_dpi_scaling(100, 200, true);
        // At any DPI >= 96, the scaled values should be >= input values
        assert!(x >= 100);
        assert!(y >= 200);
    }

    #[test]
    fn test_screenshot_cache_empty() {
        // Cache starts empty
        assert!(get_cached_screenshot(1000).is_none());
    }

    #[test]
    fn test_screenshot_cache_roundtrip() {
        let raw = vec![1, 2, 3, 4];
        let png = vec![5, 6, 7, 8];
        update_screenshot_cache(raw.clone(), png.clone());
        let cached = get_cached_screenshot(5000);
        assert_eq!(cached, Some((Arc::new(raw), Arc::new(png))));
    }

    #[test]
    fn test_pixel_diff_identical() {
        let data = vec![100u8; 256 * 4];
        assert!(pixel_diff_pct(&data, &data) < 0.01);
    }

    #[test]
    fn test_pixel_diff_completely_different() {
        let a = vec![0u8; 256 * 4];
        let b = vec![255u8; 256 * 4];
        assert!(pixel_diff_pct(&a, &b) > 99.0);
    }

    #[test]
    fn test_pixel_diff_different_lengths() {
        let a = vec![0u8; 100];
        let b = vec![0u8; 200];
        assert_eq!(pixel_diff_pct(&a, &b), 100.0);
    }

    #[test]
    fn test_desktop_tool_dispatch_coverage() {
        use crate::web::chat::jinja_templates::DESKTOP_TOOL_NAMES;
        let dummy = serde_json::json!({});
        for name in DESKTOP_TOOL_NAMES {
            assert!(
                dispatch_desktop_tool(name, &dummy).is_some(),
                "Tool '{}' in DESKTOP_TOOL_NAMES has no dispatch handler",
                name
            );
        }
    }

    #[test]
    fn test_classify_desktop_result_status() {
        assert_eq!(classify_desktop_result_status("Pressed: enter"), "completed");
        assert_eq!(classify_desktop_result_status("Timeout: no window"), "timed_out");
        assert_eq!(classify_desktop_result_status("Error [ocr_screen]: Operation timed out"), "timed_out");
        assert_eq!(classify_desktop_result_status("Error [ocr_screen]: Operation cancelled"), "cancelled");
        assert_eq!(classify_desktop_result_status("Error [click_screen]: bad coordinate"), "error");
        assert_eq!(
            classify_desktop_result_status(
                "Pressed: enter. Verification failed: expected screen change >= 0.50%, observed 0.00% after 1200ms."
            ),
            "verification_failed"
        );
    }

    #[test]
    fn test_summarize_value_for_trace_truncates_long_strings() {
        let value = serde_json::json!({
            "text": "x".repeat(250),
            "nested": ["short", "y".repeat(250)]
        });

        let summarized = summarize_value_for_trace(&value);
        let text = summarized.get("text").and_then(|v| v.as_str()).unwrap();
        assert!(text.len() <= 203);
        assert!(text.ends_with("..."));
    }

    #[test]
    fn test_parse_float_handles_strings() {
        assert_eq!(parse_float(&serde_json::json!(1.5)), Some(1.5));
        assert_eq!(parse_float(&serde_json::json!("2.25")), Some(2.25));
        assert_eq!(parse_float(&serde_json::json!("nope")), None);
    }

    #[test]
    fn test_normalize_verification_region_rejects_zero_size() {
        assert_eq!(normalize_verification_region(10, 10, 0, 10), None);
        assert_eq!(normalize_verification_region(10, 10, 10, 0), None);
    }

    #[test]
    fn test_action_verification_region_from_args() {
        let args = serde_json::json!({
            "verify_x": 100,
            "verify_y": 200,
            "verify_width": 300,
            "verify_height": 400
        });
        assert_eq!(
            action_verification_region_from_args(&args),
            Some(VerificationRegion {
                x: 100,
                y: 200,
                width: 300,
                height: 400
            })
        );
    }

    // ─── Round 6: tool_error / tool_not_supported format tests ───────────

    #[test]
    fn test_tool_error_format() {
        let r = tool_error("click_screen", "bad coordinate");
        assert_eq!(r.text, "Error [click_screen]: bad coordinate");
        assert!(r.images.is_empty());
    }

    #[test]
    fn test_tool_error_with_format_string() {
        let r = tool_error("ocr_screen", format!("monitor {} out of range", 3));
        assert_eq!(r.text, "Error [ocr_screen]: monitor 3 out of range");
    }

    #[test]
    fn test_tool_not_supported_format() {
        let r = tool_not_supported("handle_dialog");
        assert_eq!(r.text, "Error [handle_dialog]: not available on this platform");
    }

    // ─── Round 6: validated_monitors tests ───────────────────────────────

    #[test]
    fn test_validated_monitors_index_zero_ok() {
        // Index 0 should always succeed on a machine with a monitor
        let result = validated_monitors("test_tool", 0);
        assert!(result.is_ok());
        let monitors = result.unwrap();
        assert!(!monitors.is_empty());
    }

    #[test]
    fn test_validated_monitors_out_of_range() {
        let result = validated_monitors("test_tool", 999);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.text.contains("monitor 999 out of range"));
        assert!(err.text.starts_with("Error [test_tool]:"));
    }

    // ─── Round 6: verify_text in prepare_screen_verification ─────────────

    #[test]
    fn test_prepare_verification_no_flags_returns_none() {
        let args = serde_json::json!({});
        let result = prepare_screen_verification(&args, None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_prepare_verification_verify_text_enables_verification() {
        // verify_text alone should enable verification (no verify_screen_change needed)
        let args = serde_json::json!({
            "verify_text": "Hello World"
        });
        let result = prepare_screen_verification(&args, None);
        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.is_some(), "verify_text should enable verification context");
        let ctx = ctx.unwrap();
        assert_eq!(ctx.expected_text, Some("Hello World".to_string()));
    }

    #[test]
    fn test_prepare_verification_screen_change_without_text() {
        let args = serde_json::json!({
            "verify_screen_change": true
        });
        let result = prepare_screen_verification(&args, None);
        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.is_some(), "verify_screen_change=true should enable verification");
        assert_eq!(ctx.unwrap().expected_text, None);
    }

    // ─── Round 6: classify_desktop_result_status error format ────────────

    #[test]
    fn test_classify_result_status_new_tool_error_format() {
        // Verify the standardized Error [tool]: msg format is correctly classified
        assert_eq!(
            classify_desktop_result_status("Error [smart_wait]: 'text' is required when mode is 'all'"),
            "error"
        );
        assert_eq!(
            classify_desktop_result_status("Error [click_and_verify]: 'click_text' is required"),
            "error"
        );
    }

    // ─── Round 6: dispatch covers new Round 6 tools ──────────────────────

    #[test]
    fn test_dispatch_smart_wait_exists() {
        let dummy = serde_json::json!({});
        assert!(dispatch_desktop_tool("smart_wait", &dummy).is_some());
    }

    #[test]
    fn test_dispatch_click_and_verify_exists() {
        let dummy = serde_json::json!({});
        assert!(dispatch_desktop_tool("click_and_verify", &dummy).is_some());
    }

    // ─── Round 7: parse_timeout ─────────────────────────────────────────

    #[test]
    fn test_parse_timeout_default() {
        let args = serde_json::json!({});
        let dur = parse_timeout(&args);
        assert_eq!(dur, DEFAULT_THREAD_TIMEOUT);
    }

    #[test]
    fn test_parse_timeout_custom() {
        let args = serde_json::json!({"timeout_ms": 5000});
        let dur = parse_timeout(&args);
        assert_eq!(dur, std::time::Duration::from_millis(5000));
    }

    #[test]
    fn test_parse_timeout_clamp_low() {
        let args = serde_json::json!({"timeout_ms": 100});
        let dur = parse_timeout(&args);
        assert_eq!(dur, std::time::Duration::from_millis(1000));
    }

    #[test]
    fn test_parse_timeout_clamp_high() {
        let args = serde_json::json!({"timeout_ms": 999999});
        let dur = parse_timeout(&args);
        assert_eq!(dur, std::time::Duration::from_millis(60000));
    }

    // ─── Round 7: cached_monitors ───────────────────────────────────────

    #[test]
    fn test_cached_monitors_returns_list() {
        let result = cached_monitors();
        // Should succeed on any system with at least one display
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_cached_monitors_second_call_uses_cache() {
        // First call populates cache
        let _ = cached_monitors();
        // Second call should also succeed (from cache)
        let result = cached_monitors();
        assert!(result.is_ok());
    }
}
