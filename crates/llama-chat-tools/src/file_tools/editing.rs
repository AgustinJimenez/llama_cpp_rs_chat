use super::*;

fn normalize_quotes(s: &str) -> String {
    s.replace(['\u{2018}', '\u{2019}'], "'")
        .replace(['\u{201C}', '\u{201D}'], "\"")
}

fn simple_diff(old: &str, new: &str, path: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut diff = format!("--- a/{}\n+++ b/{}\n", path, path);
    let mut has_changes = false;
    let max_lines = old_lines.len().max(new_lines.len());
    let mut i = 0;
    while i < max_lines {
        let old_line = old_lines.get(i).copied().unwrap_or("");
        let new_line = new_lines.get(i).copied().unwrap_or("");

        if old_line != new_line {
            has_changes = true;
            let ctx_start = i.saturating_sub(2);
            if ctx_start < i {
                for j in ctx_start..i {
                    if let Some(l) = old_lines.get(j) {
                        diff.push_str(&format!(" {}\n", l));
                    }
                }
            }

            let mut change_end = i;
            while change_end < max_lines {
                let ol = old_lines.get(change_end).copied().unwrap_or("");
                let nl = new_lines.get(change_end).copied().unwrap_or("");
                if ol == nl && change_end > i {
                    break;
                }
                change_end += 1;
            }

            for line in &old_lines[i..change_end.min(old_lines.len())] {
                diff.push_str(&format!("-{line}\n"));
            }
            for line in &new_lines[i..change_end.min(new_lines.len())] {
                diff.push_str(&format!("+{line}\n"));
            }
            for line in &new_lines[change_end..change_end.saturating_add(2).min(new_lines.len())] {
                diff.push_str(&format!(" {line}\n"));
            }

            i = change_end;
        } else {
            i += 1;
        }
    }

    if !has_changes {
        return "No visible changes in diff".to_string();
    }
    if diff.len() > 2000 {
        let mut end = 1800;
        while end < diff.len() && !diff.is_char_boundary(end) {
            end += 1;
        }
        diff.truncate(end);
        diff.push_str("\n... (diff truncated)\n");
    }

    diff
}

pub fn tool_edit_file(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };
    let old_string = match args.get("old_string").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return "Error: 'old_string' argument is required".to_string(),
    };
    let new_string = args.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

    if old_string.is_empty() {
        return "Error: 'old_string' cannot be empty".to_string();
    }

    if let Some(cached_mtime) = file_mtime_cache().lock().ok().and_then(|c| c.get(path).copied()) {
        if let Some(current_mtime) = get_file_mtime(path) {
            if current_mtime > cached_mtime {
                eprintln!("[EDIT_FILE] File {} modified since last read (cached={}s, current={}s)", path, cached_mtime, current_mtime);
            }
        }
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return format!("Error reading '{path}': {e}"),
    };

    let match_count = content.matches(old_string).count();
    if match_count == 0 {
        let norm_content = normalize_quotes(&content);
        let norm_old = normalize_quotes(old_string);
        let norm_count = norm_content.matches(&norm_old).count();
        if norm_count == 1 {
            let norm_new = normalize_quotes(new_string);
            let new_content = norm_content.replacen(&norm_old, &norm_new, 1);
            let match_pos = norm_content.find(&norm_old).unwrap();
            let line_num = norm_content[..match_pos].lines().count().max(1);
            let backup_path = format!("{path}.llama_bak");
            let _ = std::fs::write(&backup_path, &content);
            return match std::fs::write(path, &new_content) {
                Ok(()) => {
                    if let Some(mtime) = get_file_mtime(path) {
                        if let Ok(mut cache) = file_mtime_cache().lock() {
                            cache.insert(path.to_string(), mtime);
                        }
                    }
                    invalidate_read_cache(path);
                    invalidate_file_cache(path);
                    let diff = simple_diff(&norm_content, &new_content, path);
                    format!("Edited {path} (curly quotes normalized) at line {line_num}:\n{diff}")
                }
                Err(e) => format!("Error writing '{path}': {e}"),
            };
        }
        if norm_count > 1 {
            return format!("Error: old_string found {norm_count} times in {path} (after curly quote normalization). Include more surrounding context to make it unique.");
        }
        return format!("Error: old_string not found in {path}. Make sure the text matches exactly (including whitespace and newlines).");
    }
    if match_count > 1 {
        return format!("Error: old_string found {match_count} times in {path}. Include more surrounding context to make it unique.");
    }

    let match_pos = content.find(old_string).unwrap();
    let line_num = content[..match_pos].lines().count().max(1);
    let new_content = content.replacen(old_string, new_string, 1);
    let backup_path = format!("{path}.llama_bak");
    let _ = std::fs::write(&backup_path, &content);

    match std::fs::write(path, &new_content) {
        Ok(()) => {
            if let Some(mtime) = get_file_mtime(path) {
                if let Ok(mut cache) = file_mtime_cache().lock() {
                    cache.insert(path.to_string(), mtime);
                }
            }
            invalidate_read_cache(path);
            invalidate_file_cache(path);
            let diff = simple_diff(&content, &new_content, path);
            format!("Edited {path} at line {line_num}:\n{diff}")
        }
        Err(e) => format!("Error writing '{path}': {e}"),
    }
}

pub fn tool_multi_edit(args: &Value) -> String {
    let edits = match args.get("edits").and_then(|v| v.as_array()) {
        Some(e) => e,
        None => return "Error: 'edits' argument is required and must be an array".to_string(),
    };

    if edits.is_empty() {
        return "Error: 'edits' array is empty".to_string();
    }

    let mut results: Vec<String> = Vec::new();

    for (i, edit) in edits.iter().enumerate() {
        let path = match edit.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                results.push(format!("Edit {}: Error: missing 'path'", i + 1));
                break;
            }
        };
        let old_string = match edit.get("old_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                results.push(format!("Edit {}: Error: missing 'old_string'", i + 1));
                break;
            }
        };
        let new_string = edit.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

        let result = tool_edit_file(&serde_json::json!({
            "path": path,
            "old_string": old_string,
            "new_string": new_string,
        }));

        let ok = !result.starts_with("Error:");
        results.push(format!("Edit {} ({}): {}", i + 1, path, result));
        if !ok {
            break;
        }
    }

    results.join("\n\n")
}

pub fn tool_undo_edit(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };

    let backup_path = format!("{path}.llama_bak");
    let backup_content = match std::fs::read_to_string(&backup_path) {
        Ok(c) => c,
        Err(_) => return format!("Error: no backup found for {path}. Only the most recent edit_file can be undone."),
    };

    match std::fs::write(path, &backup_content) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup_path);
            format!("Restored {path} to its state before the last edit")
        }
        Err(e) => format!("Error restoring '{path}': {e}"),
    }
}

pub fn tool_insert_text(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return "Error: 'text' argument is required".to_string(),
    };
    let line = args.get("line").and_then(|v| {
        v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
    }).unwrap_or(0) as usize;
    if line == 0 {
        return "Error: 'line' argument is required and must be >= 1".to_string();
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return format!("Error reading '{path}': {e}"),
    };

    let mut lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let insert_idx = (line - 1).min(total_lines);
    let new_lines: Vec<&str> = text.lines().collect();
    let inserted_count = new_lines.len();

    for (i, new_line) in new_lines.into_iter().enumerate() {
        lines.insert(insert_idx + i, new_line);
    }

    let mut new_content = lines.join("\n");
    if content.ends_with('\n') {
        new_content.push('\n');
    }

    match std::fs::write(path, &new_content) {
        Ok(()) => format!("Inserted {inserted_count} line(s) at line {line} in {path}"),
        Err(e) => format!("Error writing '{path}': {e}"),
    }
}
