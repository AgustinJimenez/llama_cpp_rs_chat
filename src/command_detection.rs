/// Command detection and extraction module
/// 
/// This module handles finding and parsing commands from AI responses.
/// Commands are expected to be in the format: <|EXEC|>command<|/EXEC|>

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct ExtractedCommand {
    pub command: String,
    pub raw_response: String,
    pub start_position: usize,
    pub end_position: Option<usize>,
}

/// Extract command from AI response
/// 
/// Looks for the LAST occurrence of <|EXEC|> to avoid confusion when
/// the AI explains the command format before using it.
pub fn extract_command_from_response(response: &str) -> Option<ExtractedCommand> {
    println!("🔍 COMMAND_DETECTION: Checking response for commands");
    println!("🔍 COMMAND_DETECTION: Response length: {}", response.len());
    println!("🔍 COMMAND_DETECTION: Contains <|EXEC|>: {}", response.contains("<|EXEC|>"));
    
    // Find the LAST occurrence of the opening tag
    let start = response.rfind("<|EXEC|>")?;
    println!("🔍 COMMAND_DETECTION: Found LAST <|EXEC|> at position: {}", start);
    
    // Get everything after the opening tag
    let after_start = &response[start + 8..]; // Skip "<|EXEC|>"
    println!("🔍 COMMAND_DETECTION: After start tag: '{}'", after_start);
    
    // Look for closing tag
    let command_text = if let Some(end) = after_start.find("<|/EXEC|>") {
        println!("🔍 COMMAND_DETECTION: Found <|/EXEC|> at position: {} (relative to after start)", end);
        &after_start[..end]
    } else {
        println!("🔍 COMMAND_DETECTION: No closing <|/EXEC|> tag found, using line-based parsing");
        // If no closing tag, take first line and clean up markdown artifacts
        let first_line = after_start.lines().next().unwrap_or(after_start);
        println!("🔍 COMMAND_DETECTION: First line after tag: '{}'", first_line);
        
        // Remove markdown artifacts like ``` or backticks
        let cleaned = first_line.split("```").next()
                                .unwrap_or(first_line)
                                .split('`').next()
                                .unwrap_or(first_line);
        println!("🔍 COMMAND_DETECTION: After cleaning markdown: '{}'", cleaned);
        cleaned
    }.trim();
    
    println!("🔍 COMMAND_DETECTION: Final extracted command: '{}'", command_text);
    println!("🔍 COMMAND_DETECTION: Command length: {} characters", command_text.len());
    
    if command_text.is_empty() {
        println!("❌ COMMAND_DETECTION: Command is empty after extraction");
        return None;
    }
    
    println!("✅ COMMAND_DETECTION: Successfully extracted command");
    
    Some(ExtractedCommand {
        command: command_text.to_string(),
        raw_response: response.to_string(),
        start_position: start,
        end_position: after_start.find("<|/EXEC|>").map(|pos| start + 8 + pos + 9),
    })
}

/// Check if response contains any command markers
pub fn response_contains_commands(response: &str) -> bool {
    let has_commands = response.contains("<|EXEC|>");
    println!("🔍 COMMAND_DETECTION: Response contains commands: {}", has_commands);
    has_commands
}