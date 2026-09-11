//! Incremental UTF-8 decoding across token boundaries.
//!
//! Detokenising each token independently corrupts any character whose bytes span two
//! tokens: the first piece holds a truncated prefix, the second an orphaned continuation,
//! and each decodes to `U+FFFD` on its own. Measured on Qwen3.5-9B: 5 occurrences in a
//! short emoji answer, 70 in a box-drawing directory tree, 59 in a 60 KB agent run
//! (AGENT_TASKS/013). It is position-dependent, which is the tell — `│` (U+2502) and `├`
//! (U+251C) survived while `└` (U+2514) did not, all three 3-byte sequences differing only
//! in the last byte.
//!
//! The fix is to treat the model's output as a byte stream and only emit text at complete
//! character boundaries, carrying any incomplete tail into the next token.

/// Accumulates raw token bytes and yields only well-formed UTF-8.
#[derive(Default)]
pub struct Utf8TokenDecoder {
    /// Bytes of a character that has not finished arriving yet. At most 3 bytes.
    tail: Vec<u8>,
}

impl Utf8TokenDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one token's bytes; returns the text that is now complete.
    ///
    /// An incomplete trailing sequence is retained for the next call rather than being
    /// emitted as `U+FFFD`. Genuinely invalid bytes — as opposed to merely unfinished ones —
    /// are replaced once and dropped, so a malformed stream cannot wedge the decoder.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.tail.extend_from_slice(bytes);
        let mut out = String::new();

        loop {
            match std::str::from_utf8(&self.tail) {
                Ok(s) => {
                    out.push_str(s);
                    self.tail.clear();
                    return out;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    if valid > 0 {
                        // SAFETY-free: re-validate the prefix rather than using unchecked.
                        if let Ok(s) = std::str::from_utf8(&self.tail[..valid]) {
                            out.push_str(s);
                        }
                    }
                    match e.error_len() {
                        // Incomplete tail — wait for the rest of the character.
                        None => {
                            self.tail.drain(..valid);
                            return out;
                        }
                        // Actually invalid — emit one replacement char and resynchronise.
                        Some(bad) => {
                            out.push('\u{FFFD}');
                            self.tail.drain(..valid + bad);
                            if self.tail.is_empty() {
                                return out;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Emit whatever is left at end of generation.
    ///
    /// A non-empty tail here means the stream ended mid-character, which is a real defect in
    /// the input rather than a boundary artifact, so it becomes a single replacement char.
    pub fn flush(&mut self) -> String {
        if self.tail.is_empty() {
            return String::new();
        }
        self.tail.clear();
        "\u{FFFD}".to_string()
    }

    /// True when a partial character is being held back.
    pub fn has_pending(&self) -> bool {
        !self.tail.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Utf8TokenDecoder;

    /// `└` (U+2514, E2 94 94) split across two tokens — the exact corruption from the
    /// directory-tree reproduction in AGENT_TASKS/013.
    #[test]
    fn box_drawing_char_split_across_tokens() {
        let mut d = Utf8TokenDecoder::new();
        assert_eq!(d.push(&[0xE2, 0x94]), "");
        assert_eq!(d.push(&[0x94]), "└");
        assert!(!d.has_pending());
    }

    /// A 4-byte emoji split 1/3 — the `Hi! �` case from the desktop screenshot.
    #[test]
    fn emoji_split_across_tokens() {
        let mut d = Utf8TokenDecoder::new();
        assert_eq!(d.push(&[0xF0]), "");
        assert_eq!(d.push(&[0x9F, 0x91, 0x8B]), "👋");
    }

    /// Split at every possible offset of a multi-byte char must round-trip.
    #[test]
    fn every_split_point_round_trips() {
        let s = "a└b👋c│d";
        let bytes = s.as_bytes();
        for cut in 0..bytes.len() {
            let mut d = Utf8TokenDecoder::new();
            let mut got = String::new();
            got.push_str(&d.push(&bytes[..cut]));
            got.push_str(&d.push(&bytes[cut..]));
            got.push_str(&d.flush());
            assert_eq!(got, s, "failed at cut {cut}");
        }
    }

    /// One byte at a time is the worst case and must still reconstruct exactly.
    #[test]
    fn byte_at_a_time_round_trips() {
        let s = "tree:\n└── app/\n│   ├── Http/ 👋";
        let mut d = Utf8TokenDecoder::new();
        let mut got = String::new();
        for b in s.as_bytes() {
            got.push_str(&d.push(&[*b]));
        }
        got.push_str(&d.flush());
        assert_eq!(got, s);
    }

    /// Plain ASCII must pass through untouched and hold nothing back.
    #[test]
    fn ascii_passes_through_immediately() {
        let mut d = Utf8TokenDecoder::new();
        assert_eq!(d.push(b"Hello! How can I help you today?"), "Hello! How can I help you today?");
        assert!(!d.has_pending());
        assert_eq!(d.flush(), "");
    }

    /// Genuinely invalid bytes must not wedge the decoder or swallow later text.
    #[test]
    fn invalid_bytes_resynchronise() {
        let mut d = Utf8TokenDecoder::new();
        let out = d.push(&[0x41, 0xFF, 0x42]); // 'A', invalid, 'B'
        assert_eq!(out, "A\u{FFFD}B");
        assert!(!d.has_pending());
    }

    /// A stream ending mid-character reports exactly one replacement char.
    #[test]
    fn truncated_tail_flushes_once() {
        let mut d = Utf8TokenDecoder::new();
        assert_eq!(d.push(&[0xE2, 0x94]), "");
        assert_eq!(d.flush(), "\u{FFFD}");
        assert!(!d.has_pending());
    }
}
