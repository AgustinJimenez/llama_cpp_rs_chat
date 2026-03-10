//! Screen recording tools: video capture via ffmpeg and GIF capture via screenshots.
//!
//! Three tools:
//! - `start_screen_recording` — spawn ffmpeg to record screen to a video file
//! - `stop_screen_recording` — gracefully stop the ffmpeg recording
//! - `capture_gif` — take rapid screenshots and assemble into an animated GIF (pure Rust)

use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::NativeToolResult;
use super::{parse_int, tool_error};

// ─── Recording state ─────────────────────────────────────────────────────────

struct RecordingState {
    child: Option<std::process::Child>,
    output_path: String,
    started_at: Instant,
}

impl Drop for RecordingState {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

lazy_static::lazy_static! {
    static ref RECORDING_STATE: Mutex<Option<RecordingState>> = Mutex::new(None);
}

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
            crate::log_warn!("system", "Mutex poisoned in RECORDING_STATE, recovering");
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
        let size = format!("{}x{}", width, height);
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
        crate::log_warn!("system", "Mutex poisoned in RECORDING_STATE, recovering");
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
            crate::log_warn!("system", "Mutex poisoned in RECORDING_STATE, recovering");
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

// ─── Shared types ────────────────────────────────────────────────────────────

/// A single captured frame as raw RGBA pixel data.
struct CapturedFrame {
    rgba: Vec<u8>,
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

// ─── Minimal GIF encoder ─────────────────────────────────────────────────────
//
// Encodes an animated GIF89a with a global 256-color palette.
// Uses median-cut quantization to reduce RGBA frames to 256 colors, then
// LZW-compresses each frame. This is self-contained — no external crate needed
// since the `image` crate doesn't have the `gif` feature enabled.

/// Encode multiple RGBA frames into an animated GIF file.
fn encode_animated_gif(
    path: &str,
    width: u32,
    height: u32,
    frames: &[CapturedFrame],
    delay_cs: u16,
) -> Result<(), String> {
    use std::fs::File;

    let mut file = File::create(path).map_err(|e| format!("creating file: {e}"))?;

    // Build a global palette from the first frame (256 colors via simple quantization)
    let palette = build_palette_median_cut(&frames[0].rgba);

    // GIF89a header
    file.write_all(b"GIF89a").map_err(|e| format!("writing header: {e}"))?;

    // Logical Screen Descriptor
    let w_bytes = (width as u16).to_le_bytes();
    let h_bytes = (height as u16).to_le_bytes();
    file.write_all(&w_bytes).map_err(|e| format!("writing width: {e}"))?;
    file.write_all(&h_bytes).map_err(|e| format!("writing height: {e}"))?;
    // packed: global color table flag=1, color resolution=7 (8 bits), sort=0, size=7 (256 entries)
    file.write_all(&[0xF7, 0x00, 0x00])
        .map_err(|e| format!("writing LSD: {e}"))?;

    // Global Color Table (256 * 3 = 768 bytes)
    let mut gct = [0u8; 768];
    for (i, color) in palette.iter().enumerate() {
        gct[i * 3] = color[0];
        gct[i * 3 + 1] = color[1];
        gct[i * 3 + 2] = color[2];
    }
    file.write_all(&gct).map_err(|e| format!("writing GCT: {e}"))?;

    // NETSCAPE2.0 Application Extension (for looping)
    file.write_all(&[
        0x21, 0xFF, 0x0B, // extension introducer, app extension label, block size
        b'N', b'E', b'T', b'S', b'C', b'A', b'P', b'E', b'2', b'.', b'0', // "NETSCAPE2.0"
        0x03, // sub-block size
        0x01, // sub-block ID
        0x00, 0x00, // loop count (0 = infinite)
        0x00, // block terminator
    ])
    .map_err(|e| format!("writing NETSCAPE ext: {e}"))?;

    // Write each frame
    for frame in frames {
        // Graphic Control Extension
        file.write_all(&[
            0x21, 0xF9, // extension introducer, GCE label
            0x04, // block size
            0x00, // packed: disposal=none, no user input, no transparent color
        ])
        .map_err(|e| format!("writing GCE: {e}"))?;
        file.write_all(&delay_cs.to_le_bytes())
            .map_err(|e| format!("writing delay: {e}"))?;
        file.write_all(&[0x00, 0x00]) // transparent color index, block terminator
            .map_err(|e| format!("writing GCE end: {e}"))?;

        // Image Descriptor
        file.write_all(&[0x2C]) // image separator
            .map_err(|e| format!("writing image sep: {e}"))?;
        file.write_all(&[0x00, 0x00, 0x00, 0x00]) // left, top (both 0)
            .map_err(|e| format!("writing image pos: {e}"))?;
        file.write_all(&w_bytes)
            .map_err(|e| format!("writing image width: {e}"))?;
        file.write_all(&h_bytes)
            .map_err(|e| format!("writing image height: {e}"))?;
        file.write_all(&[0x00]) // packed: no local color table, not interlaced
            .map_err(|e| format!("writing image desc packed: {e}"))?;

        // Quantize frame pixels to palette indices
        let indices = quantize_to_palette(&frame.rgba, &palette);

        // LZW compress and write image data
        let min_code_size = 8u8; // for 256 colors
        file.write_all(&[min_code_size])
            .map_err(|e| format!("writing min code size: {e}"))?;
        let compressed = lzw_compress(&indices, min_code_size);
        write_sub_blocks(&mut file, &compressed)?;
        file.write_all(&[0x00]) // block terminator
            .map_err(|e| format!("writing block terminator: {e}"))?;
    }

    // GIF Trailer
    file.write_all(&[0x3B]).map_err(|e| format!("writing trailer: {e}"))?;

    Ok(())
}

/// A simple RGB color.
type Rgb = [u8; 3];

/// Build a 256-color palette using median-cut quantization.
fn build_palette_median_cut(rgba: &[u8]) -> Vec<Rgb> {
    // Sample pixels (every 4th pixel for speed on large images)
    let pixels: Vec<Rgb> = rgba
        .chunks_exact(4)
        .step_by(4)
        .map(|c| [c[0], c[1], c[2]])
        .collect();

    if pixels.is_empty() {
        // Fallback: grayscale palette
        return (0..=255u8).map(|i| [i, i, i]).collect();
    }

    // Median-cut: recursively split the largest-range box
    let mut boxes: Vec<Vec<Rgb>> = vec![pixels];

    while boxes.len() < 256 {
        // Find the box with the largest color range
        let split_idx = match boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .max_by_key(|(_, b)| {
                let (mut min_r, mut min_g, mut min_b) = (255u8, 255u8, 255u8);
                let (mut max_r, mut max_g, mut max_b) = (0u8, 0u8, 0u8);
                for p in b.iter() {
                    min_r = min_r.min(p[0]);
                    min_g = min_g.min(p[1]);
                    min_b = min_b.min(p[2]);
                    max_r = max_r.max(p[0]);
                    max_g = max_g.max(p[1]);
                    max_b = max_b.max(p[2]);
                }
                let range_r = (max_r - min_r) as u32;
                let range_g = (max_g - min_g) as u32;
                let range_b = (max_b - min_b) as u32;
                range_r.max(range_g).max(range_b)
            }) {
            Some((idx, _)) => idx,
            None => break, // no boxes with > 1 pixel left to split
        };

        let mut box_to_split = boxes.swap_remove(split_idx);

        // Find which channel has the largest range
        let (mut min_r, mut min_g, mut min_b) = (255u8, 255u8, 255u8);
        let (mut max_r, mut max_g, mut max_b) = (0u8, 0u8, 0u8);
        for p in box_to_split.iter() {
            min_r = min_r.min(p[0]);
            min_g = min_g.min(p[1]);
            min_b = min_b.min(p[2]);
            max_r = max_r.max(p[0]);
            max_g = max_g.max(p[1]);
            max_b = max_b.max(p[2]);
        }
        let range_r = max_r - min_r;
        let range_g = max_g - min_g;
        let range_b = max_b - min_b;

        let channel = if range_r >= range_g && range_r >= range_b {
            0
        } else if range_g >= range_b {
            1
        } else {
            2
        };

        box_to_split.sort_unstable_by_key(|p| p[channel]);
        let mid = box_to_split.len() / 2;
        let right = box_to_split.split_off(mid);
        boxes.push(box_to_split);
        boxes.push(right);
    }

    // Average each box to get the palette color
    let mut palette: Vec<Rgb> = boxes
        .iter()
        .map(|b| {
            if b.is_empty() {
                return [0, 0, 0];
            }
            let (mut sr, mut sg, mut sb) = (0u64, 0u64, 0u64);
            for p in b {
                sr += p[0] as u64;
                sg += p[1] as u64;
                sb += p[2] as u64;
            }
            let n = b.len() as u64;
            [(sr / n) as u8, (sg / n) as u8, (sb / n) as u8]
        })
        .collect();

    // Pad to 256 entries if needed
    while palette.len() < 256 {
        palette.push([0, 0, 0]);
    }

    palette
}

/// Map each RGBA pixel to the closest palette index.
fn quantize_to_palette(rgba: &[u8], palette: &[Rgb]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .map(|px| {
            let r = px[0] as i32;
            let g = px[1] as i32;
            let b = px[2] as i32;
            let mut best = 0u8;
            let mut best_dist = i32::MAX;
            for (i, pal) in palette.iter().enumerate() {
                let dr = r - pal[0] as i32;
                let dg = g - pal[1] as i32;
                let db = b - pal[2] as i32;
                let dist = dr * dr + dg * dg + db * db;
                if dist < best_dist {
                    best_dist = dist;
                    best = i as u8;
                    if dist == 0 {
                        break;
                    }
                }
            }
            best
        })
        .collect()
}

/// LZW compress a stream of palette indices for GIF.
/// Uses variable-width codes starting at (min_code_size + 1) bits.
fn lzw_compress(indices: &[u8], min_code_size: u8) -> Vec<u8> {
    let clear_code = 1u16 << min_code_size;
    let eoi_code = clear_code + 1;
    let mut next_code = clear_code + 2;
    let mut code_size = min_code_size as u32 + 1;

    // Dictionary: maps (prefix_code, byte) -> code
    // Using a HashMap for simplicity. For GIF LZW this is fine.
    let mut dict = std::collections::HashMap::<(u16, u8), u16>::new();

    // Single-byte entries (0..clear_code) are implicit — they map to themselves.
    // The dictionary only stores multi-byte sequences discovered during compression.

    let mut output = BitWriter::new();

    // Write clear code
    output.write_bits(clear_code as u32, code_size);

    if indices.is_empty() {
        output.write_bits(eoi_code as u32, code_size);
        return output.finish();
    }

    let mut prefix = indices[0] as u16;

    for &byte in &indices[1..] {
        let key = (prefix, byte);
        if let Some(&code) = dict.get(&key) {
            prefix = code;
        } else {
            // Output the prefix code
            output.write_bits(prefix as u32, code_size);

            // Add new entry to dictionary
            if next_code < 4096 {
                dict.insert(key, next_code);
                next_code += 1;

                // Increase code size if needed
                if next_code > (1 << code_size) && code_size < 12 {
                    code_size += 1;
                }
            } else {
                // Dictionary full — emit clear code and reset
                output.write_bits(clear_code as u32, code_size);
                dict.clear();
                next_code = clear_code + 2;
                code_size = min_code_size as u32 + 1;
            }

            prefix = byte as u16;
        }
    }

    // Output remaining prefix
    output.write_bits(prefix as u32, code_size);

    // End of information
    output.write_bits(eoi_code as u32, code_size);

    output.finish()
}

/// Bit-level writer for LZW output (LSB first, as required by GIF).
struct BitWriter {
    buf: Vec<u8>,
    current: u32,
    bits_in_current: u32,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            current: 0,
            bits_in_current: 0,
        }
    }

    fn write_bits(&mut self, value: u32, num_bits: u32) {
        self.current |= value << self.bits_in_current;
        self.bits_in_current += num_bits;
        while self.bits_in_current >= 8 {
            self.buf.push((self.current & 0xFF) as u8);
            self.current >>= 8;
            self.bits_in_current -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits_in_current > 0 {
            self.buf.push((self.current & 0xFF) as u8);
        }
        self.buf
    }
}

/// Write data as GIF sub-blocks (max 255 bytes each).
fn write_sub_blocks(file: &mut impl Write, data: &[u8]) -> Result<(), String> {
    for chunk in data.chunks(255) {
        file.write_all(&[chunk.len() as u8])
            .map_err(|e| format!("writing sub-block size: {e}"))?;
        file.write_all(chunk)
            .map_err(|e| format!("writing sub-block data: {e}"))?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_writer_basic() {
        let mut bw = BitWriter::new();
        bw.write_bits(0b101, 3);
        bw.write_bits(0b1100, 4);
        bw.write_bits(0b1, 1);
        // bits: 101 1100 1 = 1_1100_101 = 0xE5
        let result = bw.finish();
        assert_eq!(result, vec![0xE5]);
    }

    #[test]
    fn test_bit_writer_multi_byte() {
        let mut bw = BitWriter::new();
        bw.write_bits(0xFF, 8);
        bw.write_bits(0x01, 8);
        let result = bw.finish();
        assert_eq!(result, vec![0xFF, 0x01]);
    }

    #[test]
    fn test_quantize_exact_match() {
        let palette: Vec<Rgb> = vec![[255, 0, 0], [0, 255, 0], [0, 0, 255]];
        let rgba = vec![0, 255, 0, 255]; // green pixel
        let indices = quantize_to_palette(&rgba, &palette);
        assert_eq!(indices, vec![1]);
    }

    #[test]
    fn test_quantize_closest() {
        let palette: Vec<Rgb> = vec![[0, 0, 0], [255, 255, 255]];
        let rgba = vec![200, 200, 200, 255]; // close to white
        let indices = quantize_to_palette(&rgba, &palette);
        assert_eq!(indices, vec![1]);
    }

    #[test]
    fn test_build_palette_non_empty() {
        let rgba: Vec<u8> = (0..256)
            .flat_map(|i| vec![i as u8, 0, 0, 255])
            .collect();
        let palette = build_palette_median_cut(&rgba);
        assert_eq!(palette.len(), 256);
    }

    #[test]
    fn test_lzw_compress_produces_output() {
        let indices = vec![0u8, 0, 0, 1, 1, 1, 0, 0];
        let compressed = lzw_compress(&indices, 8);
        assert!(!compressed.is_empty());
    }

    #[test]
    fn test_write_sub_blocks_small() {
        let data = vec![1u8, 2, 3];
        let mut out = Vec::new();
        write_sub_blocks(&mut out, &data).unwrap();
        assert_eq!(out, vec![3, 1, 2, 3]); // size=3, then data
    }

    #[test]
    fn test_write_sub_blocks_large() {
        let data = vec![0xAA; 300];
        let mut out = Vec::new();
        write_sub_blocks(&mut out, &data).unwrap();
        // First block: 255 bytes, second block: 45 bytes
        assert_eq!(out[0], 255);
        assert_eq!(out[256], 45);
        assert_eq!(out.len(), 1 + 255 + 1 + 45);
    }

    // ─── Round 7: RecordingState Drop ───────────────────────────────────

    #[test]
    fn test_recording_state_drop_without_child() {
        // Drop with None child should not panic
        let state = RecordingState {
            child: None,
            output_path: "/tmp/test.gif".to_string(),
            started_at: Instant::now(),
        };
        drop(state); // should not panic
    }

    #[test]
    fn test_recording_state_drop_with_child() {
        // Spawn a short-lived process, wrap in RecordingState, drop should kill it
        let child = Command::new(if cfg!(windows) { "timeout" } else { "sleep" })
            .args(if cfg!(windows) { &["/t", "60"][..] } else { &["60"][..] })
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(child) = child {
            let state = RecordingState {
                child: Some(child),
                output_path: "/tmp/test.gif".to_string(),
                started_at: Instant::now(),
            };
            drop(state); // Drop should kill + wait, not panic
        }
    }
}
