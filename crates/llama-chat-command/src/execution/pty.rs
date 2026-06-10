//! PTY-based command executor.
//!
//! Spawns a command inside a pseudo-terminal so that programs which check
//! `isatty()` see a real TTY. This prevents stdout buffering in Python
//! (even without PYTHONUNBUFFERED), Node.js, and other runtimes.
//!
//! ANSI escape sequences are stripped before returning output to the model
//! since the model doesn't need coloured terminal output.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::background::{register_streaming_process, unregister_streaming_process};

const INACTIVITY_TIMEOUT_SECS: u64 = 120;
const MAX_WALL_CLOCK_SECS: u64 = 120;
const MAX_OUTPUT_BYTES: usize = 64 * 1024; // 64 KiB — truncate very large outputs

/// Strip ANSI escape sequences from a byte slice.
fn strip_ansi(bytes: &[u8]) -> String {
    let clean = strip_ansi_escapes::strip(bytes);
    String::from_utf8_lossy(&clean).into_owned()
}

/// Execute a command inside a PTY.
///
/// Returns the (ANSI-stripped) combined stdout+stderr output as a String,
/// with the same timeout semantics as the streaming executor.
pub fn execute_command_pty(
    cmd: &str,
    cancel: Option<Arc<AtomicBool>>,
    mut on_line: impl FnMut(&str),
) -> String {
    let trimmed = cmd.trim();

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 220,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => return format!("PTY open failed: {e}"),
    };

    // Build the command. On Windows we wrap in cmd /C; on Unix use sh -c.
    let mut builder;
    #[cfg(windows)]
    {
        builder = CommandBuilder::new("cmd");
        builder.arg("/C");
        builder.arg(trimmed);
    }
    #[cfg(not(windows))]
    {
        builder = CommandBuilder::new("sh");
        builder.arg("-c");
        builder.arg(trimmed);
    }

    builder.env("PYTHONUNBUFFERED", "1");
    builder.env("FORCE_COLOR", "0");
    builder.env("NO_COLOR", "1");

    let child_result = pair.slave.spawn_command(builder);
    let mut child = match child_result {
        Ok(c) => c,
        Err(e) => return format!("PTY spawn failed: {e}"),
    };

    // The slave end must be dropped so EOF propagates to the master when the
    // child exits (otherwise master.read() blocks forever).
    drop(pair.slave);

    // We can't get the child PID from portable-pty's trait object directly,
    // but we can try on supported platforms.
    let child_pid: Option<u32> = child.process_id();

    // Register for crash-recovery if we have a PID.
    if let Some(pid) = child_pid {
        register_streaming_process(pid, &trimmed[..trimmed.len().min(200)]);
    }

    // Read from master in a background thread and send lines via channel.
    let mut master_reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            if let Some(pid) = child_pid { unregister_streaming_process(pid); }
            return format!("PTY reader error: {e}");
        }
    };

    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match master_reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut raw_output: Vec<u8> = Vec::new();
    let mut line_buf = Vec::<u8>::new();
    let wall_start = Instant::now();
    let mut last_data = Instant::now();
    let mut inactivity_killed = false;
    let mut was_cancelled = false;
    let mut truncated = false;

    loop {
        // Check cancel flag
        if let Some(ref flag) = cancel {
            if flag.load(Ordering::Relaxed) {
                was_cancelled = true;
                if let Some(pid) = child_pid { crate::kill_process_tree(pid); }
                let _ = child.kill();
                break;
            }
        }

        // Wall-clock limit
        if wall_start.elapsed().as_secs() >= MAX_WALL_CLOCK_SECS {
            if let Some(pid) = child_pid { crate::kill_process_tree(pid); }
            let _ = child.kill();
            let msg = format!(
                "\n[PTY command still running after {}s (PID {}). Use \"background\": true for servers/daemons.]\n",
                MAX_WALL_CLOCK_SECS,
                child_pid.unwrap_or(0)
            );
            raw_output.extend_from_slice(msg.as_bytes());
            break;
        }

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                last_data = Instant::now();
                // Accumulate for line-by-line callbacks
                for &byte in &chunk {
                    if byte == b'\n' || byte == b'\r' {
                        if !line_buf.is_empty() {
                            let line = strip_ansi(&line_buf);
                            on_line(&line);
                            line_buf.clear();
                        }
                    } else {
                        line_buf.push(byte);
                    }
                }
                raw_output.extend_from_slice(&chunk);
                if raw_output.len() > MAX_OUTPUT_BYTES {
                    truncated = true;
                    // Keep last 32 KiB
                    let keep = raw_output.len() - 32 * 1024;
                    raw_output.drain(..keep);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if last_data.elapsed().as_secs() >= INACTIVITY_TIMEOUT_SECS {
                    if let Some(pid) = child_pid { crate::kill_process_tree(pid); }
                    let _ = child.kill();
                    inactivity_killed = true;
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Reader thread finished — process likely exited
                break;
            }
        }
    }

    // Flush remaining line buffer
    if !line_buf.is_empty() {
        let line = strip_ansi(&line_buf);
        on_line(&line);
    }

    // Wait for child to fully exit (short timeout)
    let reap_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < reap_deadline {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => break,
        }
    }

    if let Some(pid) = child_pid {
        unregister_streaming_process(pid);
    }

    let mut output = if truncated {
        let clean = strip_ansi(&raw_output);
        format!("[...earlier output truncated...]\n{clean}")
    } else {
        strip_ansi(&raw_output)
    };

    if was_cancelled {
        output.push_str("\n[Cancelled by user]\n");
    } else if inactivity_killed {
        output.push_str(&format!(
            "\n[Process killed: no output for {INACTIVITY_TIMEOUT_SECS}s. TIP: Use \"background\": true for servers/daemons.]\n"
        ));
    }

    output
}
