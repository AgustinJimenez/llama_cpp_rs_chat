// ── Command output sanitization ──────────────────────────────────────────────

/// Maximum lines of command output to keep for model context.
const MAX_COMMAND_OUTPUT_LINES: usize = 80;
/// Maximum characters of command output to keep for model context.
const MAX_COMMAND_OUTPUT_CHARS: usize = 8000;
/// Lines to keep from the beginning of truncated output.
const HEAD_LINES: usize = 15;
/// Lines to keep from the end of truncated output.
const TAIL_LINES: usize = 25;

/// Remove ANSI escape codes (colors, cursor movement, OSC sequences) from text.
/// Uses a simple state-machine parser instead of regex to avoid potential segfaults
/// from lazy_static regex compilation in the worker process.
pub fn strip_ansi_codes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] != 0x1b {
            // Batch-copy the run of non-ESC bytes as a proper UTF-8 slice.
            // ESC (0x1B) is ASCII so the stop position is always a char boundary.
            let start = i;
            while i < len && bytes[i] != 0x1b {
                i += 1;
            }
            result.push_str(&text[start..i]);
            continue;
        }

        // ESC character — start of escape sequence
        i += 1;
        if i >= len { break; }

        if bytes[i] == b'[' {
            // CSI sequence: ESC [ ... final_byte (letter)
            i += 1;
            while i < len && (bytes[i] == b';' || bytes[i].is_ascii_digit()) {
                i += 1;
            }
            // Skip the final byte (letter)
            if i < len && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
        } else if bytes[i] == b']' {
            // OSC sequence: ESC ] ... (BEL or ESC \)
            i += 1;
            while i < len {
                if bytes[i] == 0x07 {
                    i += 1;
                    break;
                }
                if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == b'\\' {
                    i += 2;
                    break;
                }
                i += 1;
            }
        } else {
            // Other escape: skip ESC + next char
            i += 1;
        }
    }

    result
}

/// Returns true if a line is a compiler/linter diagnostic (error, warning, note, hint).
/// Matches cargo, rustc, clippy, tsc, eslint, pytest, gcc/clang patterns.
fn is_diagnostic_line(line: &str) -> bool {
    let t = line.trim_start();
    // rustc/cargo: "error[E0308]:", "warning:", "error:", "note:", "  --> src/..."
    // tsc: "src/foo.ts(10,5): error TS2322:"
    // eslint: "  10:5  error  ..."
    // pytest: "FAILED", "ERROR", "AssertionError"
    // gcc/clang: "foo.c:10:5: error:"
    t.starts_with("error") || t.starts_with("warning") || t.starts_with("note:")
        || t.starts_with("hint:") || t.starts_with("  --> ")
        || t.starts_with("= note:") || t.starts_with("= help:")
        || t.contains(": error ") || t.contains(": warning ")
        || t.starts_with("FAILED") || t.starts_with("ERROR ")
        || t.starts_with("AssertionError") || t.starts_with("thread '") // Rust panic
        || t.starts_with("panicked at")
}

/// Truncate long output: keep first HEAD_LINES + last TAIL_LINES.
/// For outputs with diagnostics (errors/warnings), also extracts all diagnostic lines
/// from the omitted middle section so none are silently dropped.
/// Also enforces a hard character limit.
pub fn truncate_command_output(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();

    let mut result = if lines.len() > MAX_COMMAND_OUTPUT_LINES {
        let omitted_range = HEAD_LINES..lines.len().saturating_sub(TAIL_LINES);
        let omitted_count = omitted_range.len();
        let head = lines[..HEAD_LINES].join("\n");
        let tail = lines[lines.len() - TAIL_LINES..].join("\n");

        // Count errors/warnings in the omitted middle for the truncation banner.
        let mid_errors = lines[omitted_range.clone()]
            .iter()
            .filter(|l| {
                let t = l.trim_start();
                t.starts_with("error") || t.contains(": error ") || t.starts_with("ERROR ")
                    || t.starts_with("FAILED") || t.starts_with("panicked at")
            })
            .count();
        let mid_warnings = lines[omitted_range.clone()]
            .iter()
            .filter(|l| {
                let t = l.trim_start();
                t.starts_with("warning") || t.contains(": warning ")
            })
            .count();
        let diag_hint = match (mid_errors, mid_warnings) {
            (0, 0) => String::new(),
            (e, 0) => format!(" — {e} error(s) in omitted lines"),
            (0, w) => format!(" — {w} warning(s) in omitted lines"),
            (e, w) => format!(" — {e} error(s), {w} warning(s) in omitted lines"),
        };

        // Collect diagnostic lines from the omitted middle that aren't already in tail.
        let tail_set: std::collections::HashSet<&str> =
            lines[lines.len() - TAIL_LINES..].iter().copied().collect();
        let mid_diagnostics: Vec<&str> = lines[omitted_range]
            .iter()
            .copied()
            .filter(|l| is_diagnostic_line(l) && !tail_set.contains(l))
            .collect();

        if mid_diagnostics.is_empty() {
            format!("{head}\n\n... ({omitted_count} lines omitted{diag_hint}) ...\n\n{tail}")
        } else {
            let diag_block = mid_diagnostics.join("\n");
            let rescued = mid_diagnostics.len();
            // Rescued diagnostics go AFTER tail so Stage 2 char truncation (75% head + 25% tail)
            // never drops them — the tail section ends just before them.
            format!(
                "{head}\n\n... ({omitted_count} lines omitted{diag_hint}) ...\n\n{tail}\n\n[{rescued} diagnostics rescued from omitted section:]\n{diag_block}"
            )
        }
    } else {
        text.to_string()
    };

    // Hard character limit — show total line count so model knows what was cut.
    if result.len() > MAX_COMMAND_OUTPUT_CHARS {
        let total_lines = text.lines().count();
        let mut end = MAX_COMMAND_OUTPUT_CHARS;
        while end < result.len() && !result.is_char_boundary(end) { end += 1; }
        result.truncate(end);
        result.push_str(&format!("\n... (output truncated at {MAX_COMMAND_OUTPUT_CHARS} chars; {total_lines} total lines)"));
    }

    result
}

/// Strip ANSI codes and truncate long command output for model context injection.
/// The raw output is still streamed to the frontend; this only affects what the model sees.
pub fn sanitize_command_output(text: &str) -> String {
    let clean = strip_ansi_codes(text);
    truncate_command_output(&clean)
}
