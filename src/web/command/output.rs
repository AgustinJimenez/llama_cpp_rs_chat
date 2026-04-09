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
        if bytes[i] == 0x1b {
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
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// Truncate long output: keep first HEAD_LINES + last TAIL_LINES, omit middle.
/// Also enforces a hard character limit.
pub fn truncate_command_output(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();

    let mut result = if lines.len() > MAX_COMMAND_OUTPUT_LINES {
        let omitted = lines.len() - HEAD_LINES - TAIL_LINES;
        let head = lines[..HEAD_LINES].join("\n");
        let tail = lines[lines.len() - TAIL_LINES..].join("\n");
        format!("{}\n\n... ({} lines omitted) ...\n\n{}", head, omitted, tail)
    } else {
        text.to_string()
    };

    // Hard character limit
    if result.len() > MAX_COMMAND_OUTPUT_CHARS {
        result.truncate(MAX_COMMAND_OUTPUT_CHARS);
        result.push_str("\n... (output truncated)");
    }

    result
}

/// Strip ANSI codes and truncate long command output for model context injection.
/// The raw output is still streamed to the frontend; this only affects what the model sees.
pub fn sanitize_command_output(text: &str) -> String {
    let clean = strip_ansi_codes(text);
    truncate_command_output(&clean)
}
