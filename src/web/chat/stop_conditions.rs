// Stop condition checking for token generation

/// Result of stop condition check
#[derive(Debug)]
pub struct StopConditionResult {
    /// Whether generation should stop
    pub should_stop: bool,
    /// Number of characters to remove from the end of the response (for partial matches)
    pub partial_to_remove: usize,
    /// Stop token that triggered the stop (for debugging/telemetry)
    pub matched_token: Option<String>,
}

impl StopConditionResult {
    pub fn no_stop() -> Self {
        Self {
            should_stop: false,
            partial_to_remove: 0,
            matched_token: None,
        }
    }

    pub fn stop_now(matched: String) -> Self {
        Self {
            should_stop: true,
            partial_to_remove: 0,
            matched_token: Some(matched),
        }
    }

    pub fn stop_with_removal(chars_to_remove: usize, matched: String) -> Self {
        Self {
            should_stop: true,
            partial_to_remove: chars_to_remove,
            matched_token: Some(matched),
        }
    }
}

/// Maximum characters after an unclosed exec open tag before we give up
/// and allow stop tokens to fire. Prevents infinite generation when a model
/// opens an exec block but never properly closes it.
const MAX_EXEC_BLOCK_LEN: usize = 1000;

/// Tracks exec block state incrementally instead of scanning the full response
/// with 6x rfind() on every token.
pub struct ExecBlockTracker {
    /// Whether we're currently inside an exec block
    in_block: bool,
    /// Position where the current block was opened
    block_open_pos: usize,
}

impl ExecBlockTracker {
    pub fn new() -> Self {
        Self {
            in_block: false,
            block_open_pos: 0,
        }
    }

    /// Update state based on the new token. Call this after appending token to response.
    /// `response_len` is the total response length after appending.
    pub fn update(&mut self, token: &str, response_len: usize) {
        if self.in_block {
            // Check if the new token contains a close tag
            if token.contains("SYSTEM.EXEC|")
                || token.contains("</tool_call>")
                || token.contains("[/TOOL_CALLS]")
            {
                self.in_block = false;
            }
            // Safety: give up if block is too long
            if response_len - self.block_open_pos > MAX_EXEC_BLOCK_LEN {
                self.in_block = false;
            }
        } else {
            // Check if the new token opens a block
            if token.contains("SYSTEM.EXEC>")
                || token.contains("<tool_call>")
                || token.contains("[TOOL_CALLS]")
            {
                self.in_block = true;
                self.block_open_pos = response_len.saturating_sub(token.len());
            }
        }
    }

    pub fn is_inside(&self) -> bool {
        self.in_block
    }
}

/// Check if the response should stop based on stop tokens
///
/// # Arguments
/// * `response` - The current response text
/// * `new_token` - The newly generated token to append
/// * `stop_tokens` - List of stop token sequences to check for
///
/// # Returns
/// `StopConditionResult` indicating whether to stop and how many chars to remove
pub fn check_stop_conditions(
    response: &str,
    new_token: &str,
    stop_tokens: &[String],
    in_exec_block: bool,
) -> StopConditionResult {
    // Early out: if inside an exec block, never stop
    if in_exec_block {
        return StopConditionResult::no_stop();
    }

    for stop_token in stop_tokens {
        if stop_token.is_empty() {
            continue;
        }

        // Quick check: the stop token can only match at the end if response+token ends with it.
        // Avoid format! allocation by checking the tail of response + new_token directly.
        let combined_len = response.len() + new_token.len();
        if combined_len >= stop_token.len() {
            // Check exact match: does response+new_token end with stop_token?
            let st_bytes = stop_token.as_bytes();
            let st_len = st_bytes.len();
            let nt_bytes = new_token.as_bytes();
            let r_bytes = response.as_bytes();

            // Build the tail from response + new_token without allocating
            let mut matches = true;
            for j in (0..st_len).rev() {
                let pos_from_end = st_len - 1 - j;
                let ch = if pos_from_end < nt_bytes.len() {
                    nt_bytes[nt_bytes.len() - 1 - pos_from_end]
                } else {
                    let r_offset = pos_from_end - nt_bytes.len();
                    if r_offset < r_bytes.len() {
                        r_bytes[r_bytes.len() - 1 - r_offset]
                    } else {
                        matches = false;
                        break;
                    }
                };
                if ch != st_bytes[j] {
                    matches = false;
                    break;
                }
            }

            if matches {
                return StopConditionResult::stop_now(stop_token.clone());
            }
        }

        // Handle partial matches (stop token prefix at end of response+token)
        // Skip for "</s>" as it matches too many HTML/XML tags
        if stop_token == "</s>" || stop_token.len() <= 2 {
            continue;
        }

        // Only check partial matches if the new token could start or extend a match.
        // Quick heuristic: check if any character in new_token appears in the stop token.
        let could_match = new_token.bytes().any(|b| stop_token.as_bytes().contains(&b));
        if !could_match {
            continue;
        }

        // Fallback: allocate and check partial matches
        let test_response = format!("{response}{new_token}");
        let trimmed = test_response.trim_end();
        let max_prefix = stop_token.len().min(trimmed.len());
        for i in 2..=max_prefix {
            let prefix = &stop_token[..i];
            if !trimmed.ends_with(prefix) {
                continue;
            }

            if i > new_token.len()
                && response
                    .trim_end()
                    .ends_with(&stop_token[..i - new_token.len()])
            {
                let partial_to_remove = i - new_token.len();
                return StopConditionResult::stop_with_removal(
                    partial_to_remove,
                    stop_token.clone(),
                );
            }

            return StopConditionResult::stop_now(stop_token.clone());
        }
    }

    StopConditionResult::no_stop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_stop_token_match() {
        let stop_tokens = vec!["</ASSISTANT>".to_string()];
        let response = "Hello, I am an assistant";
        let new_token = "</ASSISTANT>";

        let result = check_stop_conditions(response, new_token, &stop_tokens, false);
        assert!(result.should_stop);
        assert_eq!(result.partial_to_remove, 0);
        assert_eq!(result.matched_token.as_deref(), Some("</ASSISTANT>"));
    }

    #[test]
    fn test_no_match() {
        let stop_tokens = vec!["</ASSISTANT>".to_string()];
        let response = "Hello, I am";
        let new_token = " here";

        let result = check_stop_conditions(response, new_token, &stop_tokens, false);
        assert!(!result.should_stop);
        assert!(result.matched_token.is_none());
    }

    #[test]
    fn test_inside_exec_block() {
        let stop_tokens = vec!["</ASSISTANT>".to_string()];
        let new_token = "more";

        // Should not stop because we're inside exec block
        let result = check_stop_conditions("anything", new_token, &stop_tokens, true);
        assert!(!result.should_stop);
        assert!(result.matched_token.is_none());
    }

    #[test]
    fn test_outside_exec_block() {
        let stop_tokens = vec!["</ASSISTANT>".to_string()];
        let response = "<||SYSTEM.EXEC>command<SYSTEM.EXEC||> Done";
        let new_token = "</ASSISTANT>";

        // Should stop because exec block is closed (in_exec_block = false)
        let result = check_stop_conditions(response, new_token, &stop_tokens, false);
        assert!(result.should_stop);
        assert_eq!(result.matched_token.as_deref(), Some("</ASSISTANT>"));
    }
}
