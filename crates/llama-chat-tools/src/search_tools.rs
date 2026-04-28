//! File search and find tools.

use serde_json::Value;

const MAX_SEARCH_MATCHES: usize = 50;
const MAX_SEARCH_OUTPUT_CHARS: usize = 8000;
const MAX_SEARCH_FILE_SIZE: u64 = 2 * 1024 * 1024; // 2MB — skip large files

/// Check if a file is binary by looking at first 512 bytes for null bytes.
fn is_binary_file(path: &std::path::Path) -> bool {
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 512];
        if let Ok(n) = f.read(&mut buf) {
            return buf[..n].contains(&0);
        }
    }
    false
}

/// Truncate a string to max chars.
fn truncate_line(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { s.chars().take(max).collect() }
}

/// Build an ignore::WalkBuilder with .gitignore support and optional include/exclude globs.
fn build_walker(
    search_path: &str,
    include: &str,
    exclude: &str,
) -> ignore::Walk {
    let mut builder = ignore::WalkBuilder::new(search_path);
    builder
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);

    // Build overrides: plain patterns = whitelist, !patterns = blacklist
    let has_overrides = !include.is_empty() || !exclude.is_empty();
    if has_overrides {
        let mut overrides = ignore::overrides::OverrideBuilder::new(search_path);
        for pat in include.split(',') {
            let pat = pat.trim();
            if !pat.is_empty() {
                let _ = overrides.add(pat);
            }
        }
        for pat in exclude.split(',') {
            let pat = pat.trim();
            if !pat.is_empty() {
                let _ = overrides.add(&format!("!{pat}"));
            }
        }
        if let Ok(ov) = overrides.build() {
            builder.overrides(ov);
        }
    }

    builder.build()
}

/// Search file contents by pattern (literal or regex) across a directory.
/// Uses the `ignore` crate for .gitignore-aware traversal.
pub fn tool_search_files(args: &Value) -> String {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'pattern' argument is required".to_string(),
    };
    let search_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let include = args.get("include").and_then(|v| v.as_str()).unwrap_or("");
    let exclude = args.get("exclude").and_then(|v| v.as_str()).unwrap_or("");
    let context = args.get("context").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    // Try as regex first, fall back to literal
    let re = match regex::Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => match regex::Regex::new(&regex::escape(pattern)) {
            Ok(r) => r,
            Err(e) => return format!("Error: invalid pattern: {e}"),
        },
    };

    let walker = build_walker(search_path, include, exclude);

    let mut results = Vec::new();
    let mut total_matches = 0;
    let mut files_matched = 0;

    for entry in walker.filter_map(|e| e.ok()) {
        if total_matches >= MAX_SEARCH_MATCHES { break; }
        if !entry.file_type().map_or(false, |ft| ft.is_file()) { continue; }

        let path = entry.path();

        // Skip large files (>2MB) to avoid memory issues
        if std::fs::metadata(path).map_or(false, |m| m.len() > MAX_SEARCH_FILE_SIZE) { continue; }
        if is_binary_file(path) { continue; }

        if let Ok(content) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = content.lines().collect();
            let display_path = path.to_string_lossy();
            let mut file_had_match = false;
            // Track last emitted line to merge overlapping context
            let mut last_emitted: usize = 0;

            for (i, line) in lines.iter().enumerate() {
                if total_matches >= MAX_SEARCH_MATCHES { break; }
                if !re.is_match(line) { continue; }

                if !file_had_match {
                    file_had_match = true;
                    files_matched += 1;
                }
                total_matches += 1;

                if context > 0 {
                    // Before context — skip lines already emitted
                    let ctx_start = i.saturating_sub(context).max(last_emitted);
                    if ctx_start > last_emitted && last_emitted > 0 {
                        results.push("--".to_string()); // gap separator
                    }
                    for ci in ctx_start..i {
                        results.push(format!(
                            "{display_path}-{}: {}", ci + 1, truncate_line(lines[ci], 200)
                        ));
                    }
                }

                // Match line
                results.push(format!(
                    "{display_path}:{}: {}", i + 1, truncate_line(line, 200)
                ));

                if context > 0 {
                    // After context
                    let end = (i + context).min(lines.len().saturating_sub(1));
                    for ci in (i + 1)..=end {
                        results.push(format!(
                            "{display_path}-{}: {}", ci + 1, truncate_line(lines[ci], 200)
                        ));
                    }
                    last_emitted = end + 1;
                } else {
                    last_emitted = i + 1;
                }
            }
        }
    }

    if results.is_empty() {
        return format!("No matches found for '{pattern}' in {search_path}");
    }

    let mut output = format!(
        "Found {total_matches} match(es) across {files_matched} file(s) for '{pattern}':\n\n"
    );
    let mut chars = output.len();
    for line in &results {
        if chars + line.len() > MAX_SEARCH_OUTPUT_CHARS {
            output.push_str(&format!(
                "\n... (output truncated, {total_matches} total matches across {files_matched} files)"
            ));
            break;
        }
        output.push_str(line);
        output.push('\n');
        chars += line.len() + 1;
    }
    output
}

const MAX_FIND_RESULTS: usize = 100;

/// Find files by glob-like pattern recursively.
/// Uses the `ignore` crate for .gitignore-aware traversal.
pub fn tool_find_files(args: &Value) -> String {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'pattern' argument is required".to_string(),
    };
    let search_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let exclude = args.get("exclude").and_then(|v| v.as_str()).unwrap_or("");

    let walker = build_walker(search_path, pattern, exclude);

    let mut results = Vec::new();
    for entry in walker.filter_map(|e| e.ok()) {
        if results.len() >= MAX_FIND_RESULTS { break; }
        if !entry.file_type().map_or(false, |ft| ft.is_file()) { continue; }
        results.push(entry.path().to_string_lossy().to_string());
    }

    if results.is_empty() {
        return format!("No files matching '{pattern}' found in {search_path}");
    }

    let total = results.len();
    let truncated = total >= MAX_FIND_RESULTS;
    let mut output = format!("Found {total} file(s) matching '{pattern}':\n");
    for path in &results {
        output.push_str(path);
        output.push('\n');
    }
    if truncated {
        output.push_str(&format!("... (results capped at {MAX_FIND_RESULTS})\n"));
    }
    output
}
