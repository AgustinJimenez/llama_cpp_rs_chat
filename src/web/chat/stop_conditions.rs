// Stop condition checking for token generation

/// Result of stop condition check
#[derive(Debug)]
pub struct StopConditionResult {
    /// Whether generation should stop
    pub should_stop: bool,
    /// Number of characters to remove from the end of the response (for partial matches)
    pub partial_to_remove: usize,
}

impl StopConditionResult {
    pub fn no_stop() -> Self {
        Self {
            should_stop: false,
            partial_to_remove: 0,
        }
    }

    pub fn stop_now() -> Self {
        Self {
            should_stop: true,
            partial_to_remove: 0,
        }
    }

    pub fn stop_with_removal(chars_to_remove: usize) -> Self {
        Self {
            should_stop: true,
            partial_to_remove: chars_to_remove,
        }
    }
}

/// Check if we're inside a SYSTEM.EXEC block
/// Returns true if there's an opening tag without a closing tag
fn is_inside_exec_block(response: &str) -> bool {
    const EXEC_OPEN: &str = "<||SYSTEM.EXEC>";
    const EXEC_CLOSE: &str = "<SYSTEM.EXEC||>";

    let has_exec_open = response.contains("SYSTEM.EXEC>") || response.contains(EXEC_OPEN);
    let has_exec_close = response.contains(EXEC_CLOSE) || response.contains("<SYSTEM.EXEC|");
    has_exec_open && !has_exec_close
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
) -> StopConditionResult {
    // Test response with the new token appended
    let test_response = format!("{}{}", response, new_token);

    // Don't stop if we're inside an exec block - let it complete
    let in_exec_block = is_inside_exec_block(response);

    for stop_token in stop_tokens {
        // Skip stop token checking if inside exec block
        if in_exec_block {
            continue;
        }

        // Check for exact match
        if test_response.contains(stop_token) {
            return StopConditionResult::stop_now();
        }

        // Handle partial matches
        // Skip partial matching for "</s>" as it matches too many HTML/XML tags
        if stop_token == "</s>" {
            continue;
        }

        // Check for partial matches at the end of the response
        if stop_token.len() > 2 {
            let trimmed = test_response.trim_end();
            for i in 2..stop_token.len() {
                if trimmed.ends_with(&stop_token[..i]) {
                    // Check if this partial match existed before the new token
                    // If so, we need to remove the partial from previous tokens
                    if response.trim_end().ends_with(&stop_token[..i - new_token.len()])
                        && i > new_token.len() {
                        let partial_to_remove = i - new_token.len();
                        return StopConditionResult::stop_with_removal(partial_to_remove);
                    }
                    return StopConditionResult::stop_now();
                }
            }
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

        let result = check_stop_conditions(response, new_token, &stop_tokens);
        assert!(result.should_stop);
        assert_eq!(result.partial_to_remove, 0);
    }

    #[test]
    fn test_no_match() {
        let stop_tokens = vec!["</ASSISTANT>".to_string()];
        let response = "Hello, I am";
        let new_token = " here";

        let result = check_stop_conditions(response, new_token, &stop_tokens);
        assert!(!result.should_stop);
    }

    #[test]
    fn test_inside_exec_block() {
        let stop_tokens = vec!["</ASSISTANT>".to_string()];
        let response = "<||SYSTEM.EXEC>Some command output</ASSISTANT>";
        let new_token = "more";

        // Should not stop because we're inside exec block (no closing tag yet)
        let result = check_stop_conditions(response, new_token, &stop_tokens);
        assert!(!result.should_stop);
    }

    #[test]
    fn test_outside_exec_block() {
        let stop_tokens = vec!["</ASSISTANT>".to_string()];
        let response = "<||SYSTEM.EXEC>command<SYSTEM.EXEC||> Done";
        let new_token = "</ASSISTANT>";

        // Should stop because exec block is closed
        let result = check_stop_conditions(response, new_token, &stop_tokens);
        assert!(result.should_stop);
    }
}
