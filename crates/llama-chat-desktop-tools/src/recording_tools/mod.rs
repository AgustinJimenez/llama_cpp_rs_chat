//! Screen recording tools: video capture via ffmpeg and GIF capture via screenshots.
//!
//! Three tools:
//! - `start_screen_recording` — spawn ffmpeg to record screen to a video file
//! - `stop_screen_recording` — gracefully stop the ffmpeg recording
//! - `capture_gif` — take rapid screenshots and assemble into an animated GIF (pure Rust)

use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{parse_int, tool_error, NativeToolResult};

pub(crate) mod gif_encoder;
mod state;

use gif_encoder::{encode_animated_gif, CapturedFrame};
use state::{RecordingState, RECORDING_STATE};

#[cfg(test)]
mod tests;

// ─── ffmpeg availability check ───────────────────────────────────────────────

fn check_ffmpeg() -> Result<(), String> {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    match cmd.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(_) => Err("ffmpeg found but returned an error. Please reinstall ffmpeg.".to_string()),
        Err(_) => Err(
            "ffmpeg not found. Install it:\n\
             - Windows: winget install ffmpeg / choco install ffmpeg\n\
             - macOS: brew install ffmpeg\n\
             - Linux: sudo apt install ffmpeg"
                .to_string(),
        ),
    }
}

// ─── Tool: start_screen_recording ────────────────────────────────────────────

/// Start recording the screen using ffmpeg.
///
/// Params:
/// - `output_path` (string, required): path for the output video file
/// - `fps` (integer, default 15): frames per second
/// - `monitor` (integer, default 0): monitor index
pub fn tool_start_screen_recording(args: &Value) -> NativeToolResult {
    let output_path = match args.get("output_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return tool_error("start_screen_recording", "'output_path' is required"),
    };
    let fps = args.get("fps").and_then(parse_int).unwrap_or(15).clamp(1, 60);
    #[allow(unused_variables)]
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    // Check if already recording
    {
        let lock = RECORDING_STATE.lock().unwrap_or_else(|p| {
            log_warn!("system", "Mutex poisoned in RECORDING_STATE, recovering");
            p.into_inner()
        });
        if lock.is_some() {
            return tool_error(
                "start_screen_recording",
                "A recording is already in progress. Stop it first with stop_screen_recording.",
            );
        }
    }

    // Verify ffmpeg is available
    if let Err(e) = check_ffmpeg() {
        return tool_error("start_screen_recording", e);
    }

    // Build platform-specific ffmpeg command
    let mut cmd = Command::new("ffmpeg");

    #[cfg(target_os = "windows")]
    {
        // gdigrab captures the desktop on Windows
        cmd.args([
            "-f", "gdigrab",
            "-framerate", &fps.to_string(),
            "-i", "desktop",
            "-c:v", "libx264",
            "-preset", "ultrafast",
            "-crf", "28",
            "-y",
            &output_path,
        ]);
    }

    #[cfg(target_os = "macos")]
    {
        // avfoundation with monitor index, no audio
        let input = format!("{}:none", monitor_idx);
        cmd.args([
            "-f", "avfoundation",
            "-framerate", &fps.to_string(),
            "-i", &input,
            "-c:v", "libx264",
            "-preset", "ultrafast",
            "-crf", "28",
            "-y",
            &output_path,
        ]);
    }

    #[cfg(target_os = "linux")]
    {
        // x11grab needs screen dimensions
        let (width, height) = get_linux_screen_size(monitor_idx);
        let size = format!("{width}x{height}");
        cmd.args([
            "-f", "x11grab",
            "-framerate", &fps.to_string(),
            "-video_size", &size,
            "-i", ":0.0+0,0",
            "-c:v", "libx264",
            "-preset", "ultrafast",
            "-crf", "28",
            "-y",
            &output_path,
        ]);
    }

    // stdin piped so we can send 'q' to stop, stdout/stderr silenced
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return tool_error("start_screen_recording", format!("failed to start ffmpeg: {e}")),
    };

    let mut lock = RECORDING_STATE.lock().unwrap_or_else(|p| {
        log_warn!("system", "Mutex poisoned in RECORDING_STATE, recovering");
        p.into_inner()
    });
    *lock = Some(RecordingState {
        child: Some(child),
        output_path: output_path.clone(),
        started_at: Instant::now(),
    });

    NativeToolResult::text_only(format!(
        "Recording started to {output_path} at {fps}fps (monitor {monitor_idx})"
    ))
}

// ─── Tool: stop_screen_recording ─────────────────────────────────────────────

/// Stop the current screen recording.
///
/// No required params.
pub fn tool_stop_screen_recording(args: &Value) -> NativeToolResult {
    let _ = args; // no required params

    let state = {
        let mut lock = RECORDING_STATE.lock().unwrap_or_else(|p| {
            log_warn!("system", "Mutex poisoned in RECORDING_STATE, recovering");
            p.into_inner()
        });
        lock.take()
    };

    let mut state = match state {
        Some(s) => s,
        None => return tool_error("stop_screen_recording", "No recording is currently in progress."),
    };

    let duration = state.started_at.elapsed();
    let output_path = state.output_path.clone();

    // Take child out of Option so Drop won't also try to kill it
    let mut child = match state.child.take() {
        Some(c) => c,
        None => return tool_error("stop_screen_recording", "recording child process already consumed"),
    };

    // Send 'q' to ffmpeg's stdin for graceful stop (finalizes the file)
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"q");
        let _ = stdin.flush();
    }

    // Wait for process to exit with a 10-second timeout
    let exited_cleanly = {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut child = child;
        std::thread::spawn(move || {
            let status = child.wait();
            let _ = tx.send(status);
        });
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(_status)) => true,
            Ok(Err(_)) => false,
            Err(_) => {
                // Timeout — ffmpeg didn't exit; it was moved into the thread
                // so we cannot kill it here, but the thread will eventually reap it.
                false
            }
        }
    };

    // Get file info
    let file_info = match std::fs::metadata(&output_path) {
        Ok(meta) => format!("{} bytes", meta.len()),
        Err(_) => "file not found (ffmpeg may have failed)".to_string(),
    };

    let duration_secs = duration.as_secs_f64();
    let exit_note = if exited_cleanly { "" } else { " (ffmpeg did not exit cleanly)" };

    NativeToolResult::text_only(format!(
        "Recording stopped. Duration: {duration_secs:.1}s, File: {output_path} ({file_info}){exit_note}"
    ))
}

// ─── Tool: capture_gif ───────────────────────────────────────────────────────

/// Capture a short animated GIF by taking rapid screenshots.
/// Pure Rust implementation — no ffmpeg needed.
///
/// Params:
/// - `output_path` (string, required): path for the output GIF file
/// - `duration_ms` (integer, default 3000, max 30000): capture duration
/// - `fps` (integer, default 10, max 30): frames per second
/// - `monitor` (integer, default 0): monitor index
pub fn tool_capture_gif(args: &Value) -> NativeToolResult {
    let output_path = match args.get("output_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return tool_error("capture_gif", "'output_path' is required"),
    };
    let duration_ms = args
        .get("duration_ms")
        .and_then(parse_int)
        .unwrap_or(3000)
        .clamp(100, 30000) as u64;
    let fps = args
        .get("fps")
        .and_then(parse_int)
        .unwrap_or(10)
        .clamp(1, 30) as u32;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let frame_interval = Duration::from_millis(1000 / fps as u64);
    let frame_count = ((duration_ms as f64 / 1000.0) * fps as f64).ceil() as usize;

    // Capture frames
    let monitors = match super::validated_monitors("capture_gif", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let monitor = &monitors[monitor_idx];

    // Determine downscale dimensions (max 640px wide for reasonable GIF size)
    let first_img = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => return tool_error("capture_gif", format!("capturing first frame: {e}")),
    };
    let src_w = first_img.width();
    let src_h = first_img.height();
    let max_width = 640u32;
    let (dst_w, dst_h) = if src_w > max_width {
        let scale = max_width as f64 / src_w as f64;
        (max_width, (src_h as f64 * scale) as u32)
    } else {
        (src_w, src_h)
    };

    // Collect frames as downscaled RGBA images
    let mut frames: Vec<CapturedFrame> = Vec::with_capacity(frame_count);

    // First frame is already captured
    let scaled = image::imageops::resize(
        &first_img,
        dst_w,
        dst_h,
        image::imageops::FilterType::Nearest,
    );
    frames.push(CapturedFrame {
        rgba: scaled.into_raw(),
    });

    let capture_start = Instant::now();
    for _ in 1..frame_count {
        std::thread::sleep(frame_interval);

        if capture_start.elapsed().as_millis() > duration_ms as u128 + 500 {
            break; // safety: don't run longer than requested + margin
        }

        let img = match monitor.capture_image() {
            Ok(img) => img,
            Err(_) => continue, // skip failed frames
        };
        let scaled = image::imageops::resize(
            &img,
            dst_w,
            dst_h,
            image::imageops::FilterType::Nearest,
        );
        frames.push(CapturedFrame {
            rgba: scaled.into_raw(),
        });
    }

    let actual_duration = capture_start.elapsed();
    let actual_frames = frames.len();

    if actual_frames == 0 {
        return tool_error("capture_gif", "failed to capture any frames");
    }

    // Encode as GIF using our minimal encoder
    let delay_cs = (frame_interval.as_millis() as u16 + 5) / 10; // centiseconds
    match encode_animated_gif(&output_path, dst_w, dst_h, &frames, delay_cs) {
        Ok(()) => {}
        Err(e) => return tool_error("capture_gif", format!("encoding GIF: {e}")),
    }

    // Get output file size
    let file_size = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    NativeToolResult::text_only(format!(
        "GIF captured: {actual_frames} frames, {:.1}s, {}x{}, file: {output_path} ({file_size} bytes)",
        actual_duration.as_secs_f64(),
        dst_w,
        dst_h,
    ))
}

// ─── Platform helpers ────────────────────────────────────────────────────────

/// Get screen size for Linux x11grab. Falls back to 1920x1080.
#[cfg(target_os = "linux")]
fn get_linux_screen_size(monitor_idx: usize) -> (u32, u32) {
    if let Ok(monitors) = xcap::Monitor::all() {
        if let Some(mon) = monitors.get(monitor_idx) {
            let w = mon.width().unwrap_or(1920);
            let h = mon.height().unwrap_or(1080);
            return (w, h);
        }
    }
    (1920, 1080)
}
