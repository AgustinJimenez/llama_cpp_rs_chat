//! File system tool handlers: read, write, edit, undo_edit, insert_text, search, find, list.

use tokio::fs;
use tokio::task::spawn_blocking;

use super::helpers::canonicalize_allowed;

pub async fn handle_read_file(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return serde_json::json!({ "success": false, "error": "File path is required" });
    }
    let safe_path = match canonicalize_allowed(path).await {
        Ok(p) => p,
        Err(e) => return serde_json::json!({ "success": false, "error": e }),
    };
    match fs::read_to_string(&safe_path).await {
        Ok(content) => serde_json::json!({
            "success": true,
            "result": content,
            "path": safe_path.to_string_lossy()
        }),
        Err(e) => serde_json::json!({
            "success": false,
            "error": format!("Failed to read file '{}': {}", safe_path.to_string_lossy(), e)
        }),
    }
}

pub async fn handle_write_file(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let content = tool_arguments.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return serde_json::json!({ "success": false, "error": "File path is required" });
    }
    let safe_path = match canonicalize_allowed(path).await {
        Ok(p) => p,
        Err(e) => return serde_json::json!({ "success": false, "error": e }),
    };
    match fs::write(&safe_path, content).await {
        Ok(_) => serde_json::json!({
            "success": true,
            "result": format!("Successfully wrote {} bytes to '{}'", content.len(), safe_path.to_string_lossy()),
            "path": safe_path.to_string_lossy(),
            "bytes_written": content.len()
        }),
        Err(e) => serde_json::json!({
            "success": false,
            "error": format!("Failed to write file '{}': {}", safe_path.to_string_lossy(), e)
        }),
    }
}

pub async fn handle_edit_file(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let old_string = tool_arguments.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
    let new_string = tool_arguments.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return serde_json::json!({ "success": false, "error": "File path is required" });
    }
    if old_string.is_empty() {
        return serde_json::json!({ "success": false, "error": "old_string is required" });
    }
    let safe_path = match canonicalize_allowed(path).await {
        Ok(p) => p,
        Err(e) => return serde_json::json!({ "success": false, "error": e }),
    };
    match fs::read_to_string(&safe_path).await {
        Ok(content) => {
            let match_count = content.matches(old_string).count();
            if match_count == 0 {
                serde_json::json!({
                    "success": false,
                    "error": format!("old_string not found in '{}'", safe_path.to_string_lossy())
                })
            } else if match_count > 1 {
                serde_json::json!({
                    "success": false,
                    "error": format!("old_string found {} times — include more context to make it unique", match_count)
                })
            } else {
                let new_content = content.replacen(old_string, new_string, 1);
                match fs::write(&safe_path, &new_content).await {
                    Ok(_) => serde_json::json!({
                        "success": true,
                        "result": format!("Edited '{}': replaced {} chars with {} chars",
                            safe_path.to_string_lossy(), old_string.len(), new_string.len()),
                        "path": safe_path.to_string_lossy()
                    }),
                    Err(e) => serde_json::json!({
                        "success": false,
                        "error": format!("Failed to write '{}': {}", safe_path.to_string_lossy(), e)
                    }),
                }
            }
        }
        Err(e) => serde_json::json!({
            "success": false,
            "error": format!("Failed to read '{}': {}", safe_path.to_string_lossy(), e)
        }),
    }
}

pub async fn handle_undo_edit(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return serde_json::json!({ "success": false, "error": "File path is required" });
    }
    let safe_path = match canonicalize_allowed(path).await {
        Ok(p) => p,
        Err(e) => return serde_json::json!({ "success": false, "error": e }),
    };
    let backup_path_str = format!("{}.llama_bak", safe_path.to_string_lossy());
    let backup_path = std::path::PathBuf::from(&backup_path_str);
    if !backup_path.exists() {
        return serde_json::json!({
            "success": false,
            "error": format!("No backup found for '{}'", safe_path.to_string_lossy())
        });
    }
    match fs::read(&backup_path).await {
        Ok(backup_content) => match fs::write(&safe_path, &backup_content).await {
            Ok(_) => {
                let _ = fs::remove_file(&backup_path).await;
                serde_json::json!({
                    "success": true,
                    "result": format!("Restored '{}' from backup", safe_path.to_string_lossy())
                })
            }
            Err(e) => serde_json::json!({
                "success": false,
                "error": format!("Failed to restore '{}': {}", safe_path.to_string_lossy(), e)
            }),
        },
        Err(e) => serde_json::json!({
            "success": false,
            "error": format!("Failed to read backup: {}", e)
        }),
    }
}

pub async fn handle_insert_text(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let line = tool_arguments.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let text = tool_arguments.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return serde_json::json!({ "success": false, "error": "File path is required" });
    }
    if line == 0 {
        return serde_json::json!({ "success": false, "error": "Line number is required (1-based)" });
    }
    let safe_path = match canonicalize_allowed(path).await {
        Ok(p) => p,
        Err(e) => return serde_json::json!({ "success": false, "error": e }),
    };
    match fs::read_to_string(&safe_path).await {
        Ok(content) => {
            let mut lines: Vec<&str> = content.lines().collect();
            let insert_idx = if line - 1 > lines.len() { lines.len() } else { line - 1 };
            lines.insert(insert_idx, text);
            let new_content = lines.join("\n");
            let new_content = if content.ends_with('\n') {
                format!("{}\n", new_content)
            } else {
                new_content
            };
            match fs::write(&safe_path, &new_content).await {
                Ok(_) => serde_json::json!({
                    "success": true,
                    "result": format!("Inserted text at line {} in '{}'", line, safe_path.to_string_lossy())
                }),
                Err(e) => serde_json::json!({
                    "success": false,
                    "error": format!("Failed to write '{}': {}", safe_path.to_string_lossy(), e)
                }),
            }
        }
        Err(e) => serde_json::json!({
            "success": false,
            "error": format!("Failed to read '{}': {}", safe_path.to_string_lossy(), e)
        }),
    }
}

pub async fn handle_search_files(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let pattern = tool_arguments.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let include = tool_arguments.get("include").and_then(|v| v.as_str());
    if pattern.is_empty() {
        return serde_json::json!({ "success": false, "error": "Search pattern is required" });
    }
    let safe_path = match canonicalize_allowed(path).await {
        Ok(p) => p,
        Err(e) => return serde_json::json!({ "success": false, "error": e }),
    };
    let pattern_owned = pattern.to_string();
    let include_owned = include.map(|s| s.to_string());
    let result = spawn_blocking(move || {
        use walkdir::WalkDir;
        let re = regex::Regex::new(&pattern_owned).ok();
        let mut output = String::new();
        let mut match_count = 0usize;
        for entry in WalkDir::new(&safe_path).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() { continue; }
            let fname = entry.file_name().to_string_lossy();
            let path_str = entry.path().to_string_lossy();
            if path_str.contains("node_modules") || path_str.contains(".git") || path_str.contains("target") { continue; }
            if let Some(ref inc) = include_owned {
                let pat = inc.trim_start_matches('*');
                if !fname.ends_with(pat) { continue; }
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                for (i, line) in content.lines().enumerate() {
                    let matched = if let Some(ref r) = re {
                        r.is_match(line)
                    } else {
                        line.contains(&pattern_owned)
                    };
                    if matched {
                        output.push_str(&format!("{}:{}: {}\n",
                            entry.path().display(), i + 1,
                            line.chars().take(200).collect::<String>()));
                        match_count += 1;
                        if match_count >= 50 || output.len() >= 8000 { break; }
                    }
                }
            }
            if match_count >= 50 || output.len() >= 8000 { break; }
        }
        if match_count == 0 {
            format!("No matches found for pattern '{}'", pattern_owned)
        } else {
            format!("{} match{}\n{}", match_count, if match_count == 1 { "" } else { "es" }, output)
        }
    }).await;
    match result {
        Ok(text) => serde_json::json!({ "success": true, "result": text }),
        Err(e) => serde_json::json!({ "success": false, "error": format!("Search failed: {}", e) }),
    }
}

pub async fn handle_find_files(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let pattern = tool_arguments.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    if pattern.is_empty() {
        return serde_json::json!({ "success": false, "error": "File pattern is required" });
    }
    let safe_path = match canonicalize_allowed(path).await {
        Ok(p) => p,
        Err(e) => return serde_json::json!({ "success": false, "error": e }),
    };
    let pattern_owned = pattern.to_string();
    let result = spawn_blocking(move || {
        use walkdir::WalkDir;
        let mut matches = Vec::new();
        let pat = pattern_owned.split('/').last().unwrap_or(&pattern_owned);
        let pat = pat.split('\\').last().unwrap_or(pat);
        for entry in WalkDir::new(&safe_path).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() { continue; }
            let path_str = entry.path().to_string_lossy();
            if path_str.contains("node_modules") || path_str.contains(".git") || path_str.contains("target") { continue; }
            let fname = entry.file_name().to_string_lossy();
            let matched = if pat.starts_with('*') && pat.ends_with('*') && pat.len() > 2 {
                fname.contains(&pat[1..pat.len()-1])
            } else if pat.starts_with('*') {
                fname.ends_with(&pat[1..])
            } else if pat.ends_with('*') {
                fname.starts_with(&pat[..pat.len()-1])
            } else {
                fname.as_ref() == pat
            };
            if matched {
                matches.push(entry.path().display().to_string());
                if matches.len() >= 100 { break; }
            }
        }
        if matches.is_empty() {
            format!("No files matching '{}' found", pattern_owned)
        } else {
            format!("{} file{}\n{}", matches.len(), if matches.len() == 1 { "" } else { "s" }, matches.join("\n"))
        }
    }).await;
    match result {
        Ok(text) => serde_json::json!({ "success": true, "result": text }),
        Err(e) => serde_json::json!({ "success": false, "error": format!("Find failed: {}", e) }),
    }
}

pub async fn handle_list_directory(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let recursive = tool_arguments.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);
    let safe_path = match canonicalize_allowed(path).await {
        Ok(p) => p,
        Err(e) => return serde_json::json!({ "success": false, "error": e }),
    };

    if recursive {
        use walkdir::WalkDir;
        let root = safe_path.clone();
        let result = spawn_blocking(move || {
            let entries: Vec<String> = WalkDir::new(root)
                .into_iter()
                .filter_map(|e| e.ok())
                .map(|e| {
                    let metadata = e.metadata().ok();
                    let size = metadata.as_ref().and_then(|m| {
                        if m.is_file() { Some(m.len()) } else { None }
                    });
                    let file_type = if e.file_type().is_dir() { "DIR" } else { "FILE" };
                    format!(
                        "{:>10} {:>15} {}",
                        file_type,
                        size.map(|s| format!("{s} bytes")).unwrap_or_default(),
                        e.path().display()
                    )
                })
                .collect();
            entries
        }).await;
        match result {
            Ok(entries) => serde_json::json!({
                "success": true,
                "result": entries.join("\n"),
                "path": safe_path.to_string_lossy(),
                "count": entries.len(),
                "recursive": true
            }),
            Err(e) => serde_json::json!({
                "success": false,
                "error": format!("Failed to list directory '{}': {}", safe_path.to_string_lossy(), e)
            }),
        }
    } else {
        match fs::read_dir(&safe_path).await {
            Ok(mut entries) => {
                let mut items: Vec<String> = Vec::new();
                while let Ok(Some(e)) = entries.next_entry().await {
                    let metadata = e.metadata().await.ok();
                    let size = metadata.as_ref().and_then(|m| {
                        if m.is_file() { Some(m.len()) } else { None }
                    });
                    let file_type = if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
                        "DIR"
                    } else {
                        "FILE"
                    };
                    items.push(format!(
                        "{:>10} {:>15} {}",
                        file_type,
                        size.map(|s| format!("{s} bytes")).unwrap_or_else(|| "".to_string()),
                        e.file_name().to_string_lossy()
                    ));
                }
                serde_json::json!({
                    "success": true,
                    "result": items.join("\n"),
                    "path": safe_path.to_string_lossy(),
                    "count": items.len(),
                    "recursive": false
                })
            }
            Err(e) => serde_json::json!({
                "success": false,
                "error": format!("Failed to list directory '{}': {}", safe_path.to_string_lossy(), e)
            }),
        }
    }
}
