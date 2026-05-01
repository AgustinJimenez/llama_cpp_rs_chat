//! File I/O tools: read, write, edit, undo, insert.

use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;

use crate::doc_extractors::{
    extract_csv_structured, extract_docx_text, extract_eml_text, extract_epub_text,
    extract_odt_text, extract_pdf_with_pages, extract_pptx_text, extract_rtf_text,
    extract_xlsx_text, extract_zip_listing,
};

/// Maximum file size to return inline (100 KB).
const MAX_READ_SIZE: usize = 100 * 1024;

// ─── File modification time cache (for edit_file concurrent modification detection) ──
static FILE_MTIME_CACHE: OnceLock<StdMutex<HashMap<String, u64>>> = OnceLock::new();

fn file_mtime_cache() -> &'static StdMutex<HashMap<String, u64>> {
    FILE_MTIME_CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

// ─── Duplicate read detection cache: path → (mtime, line_count) ───────────────
static READ_FILE_CACHE: OnceLock<StdMutex<HashMap<String, (u64, usize)>>> = OnceLock::new();

fn read_file_cache() -> &'static StdMutex<HashMap<String, (u64, usize)>> {
    READ_FILE_CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Clear the read-file duplicate cache entry for a path (call after writes/edits).
fn invalidate_read_cache(path: &str) {
    if let Ok(mut cache) = read_file_cache().lock() {
        cache.remove(path);
    }
}

pub fn get_file_mtime(path: &str) -> Option<u64> {
    std::fs::metadata(path).ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// Detect binary content by inspecting the first bytes.
/// More reliable than extension-only checking — catches misnamed files.
pub fn is_binary_content(bytes: &[u8]) -> bool {
    let check_size = bytes.len().min(8192);
    let mut non_printable = 0;

    for &byte in &bytes[..check_size] {
        // Null byte is a strong binary indicator
        if byte == 0 {
            return true;
        }
        // Count non-printable, non-whitespace bytes
        // Printable ASCII: 32-126, plus tab(9), newline(10), carriage return(13)
        if byte < 32 && byte != 9 && byte != 10 && byte != 13 {
            non_printable += 1;
        }
    }

    // If >10% non-printable, likely binary
    check_size > 0 && (non_printable * 100 / check_size) > 10
}

// ─── LRU file content cache ──────────────────────────────────────────────────

const FILE_CACHE_MAX_ENTRIES: usize = 50;
const FILE_CACHE_MAX_BYTES: usize = 25 * 1024 * 1024; // 25MB total

struct FileCacheEntry {
    content: String,
    mtime: u64,
    access_order: u64,
}

static FILE_CONTENT_CACHE: OnceLock<StdMutex<(HashMap<String, FileCacheEntry>, u64, usize)>> = OnceLock::new();

fn file_content_cache() -> &'static StdMutex<(HashMap<String, FileCacheEntry>, u64, usize)> {
    FILE_CONTENT_CACHE.get_or_init(|| StdMutex::new((HashMap::new(), 0, 0)))
}

/// Get cached file content if mtime matches, otherwise read from disk and cache.
fn read_file_cached(path: &str) -> Result<String, String> {
    let current_mtime = get_file_mtime(path).unwrap_or(0);

    // Check cache
    if let Ok(mut cache) = file_content_cache().lock() {
        let (ref mut map, ref mut counter, _) = *cache;
        if let Some(entry) = map.get_mut(path) {
            if entry.mtime == current_mtime {
                *counter += 1;
                entry.access_order = *counter;
                return Ok(entry.content.clone());
            }
        }
    }

    // Cache miss — read from disk as bytes for binary detection
    let bytes = std::fs::read(path).map_err(|e| format!("Error reading file: {e}"))?;

    // Content-based binary check
    if has_binary_extension(path) || is_binary_content(&bytes) {
        let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
        if !EXTRACTABLE_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
            return Err(format!("Error: '{}' appears to be a binary file ({} bytes). Cannot read as text.", path, bytes.len()));
        }
    }

    let content = String::from_utf8_lossy(&bytes).to_string();

    // Store in cache (evict oldest if needed)
    if let Ok(mut cache) = file_content_cache().lock() {
        let (ref mut map, ref mut counter, ref mut total_bytes) = *cache;
        *counter += 1;

        let content_len = content.len();

        // Evict if over limits
        while (map.len() >= FILE_CACHE_MAX_ENTRIES || *total_bytes + content_len > FILE_CACHE_MAX_BYTES) && !map.is_empty() {
            // Find oldest entry
            if let Some(oldest_key) = map.iter()
                .min_by_key(|(_, v)| v.access_order)
                .map(|(k, _)| k.clone())
            {
                if let Some(removed) = map.remove(&oldest_key) {
                    *total_bytes = total_bytes.saturating_sub(removed.content.len());
                }
            } else {
                break;
            }
        }

        *total_bytes += content_len;
        map.insert(path.to_string(), FileCacheEntry {
            content: content.clone(),
            mtime: current_mtime,
            access_order: *counter,
        });
    }

    Ok(content)
}

/// Invalidate the LRU file content cache for a path (call after writes/edits).
fn invalidate_file_cache(path: &str) {
    if let Ok(mut cache) = file_content_cache().lock() {
        let (ref mut map, _, ref mut total_bytes) = *cache;
        if let Some(removed) = map.remove(path) {
            *total_bytes = total_bytes.saturating_sub(removed.content.len());
        }
    }
}

/// Check if a file has a binary extension that cannot be meaningfully read as text.
fn has_binary_extension(path: &str) -> bool {
    let binary_extensions = [
        "exe", "dll", "so", "dylib", "o", "obj", "a", "lib",
        "zip", "tar", "gz", "bz2", "xz", "7z", "rar",
        "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "svg",
        "mp3", "mp4", "wav", "avi", "mkv", "mov", "flac",
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
        "gguf", "bin", "dat", "db", "sqlite",
        "pyc", "class", "wasm",
    ];
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        binary_extensions.contains(&ext.to_lowercase().as_str())
    } else {
        false
    }
}

/// Extensions that are binary but have dedicated text-extraction support.
const EXTRACTABLE_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "xlsx", "xls", "xlsm", "pptx", "epub", "odt", "rtf", "csv", "eml", "msg",
    "zip", "7z",
];

pub fn tool_read_file(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };

    let path_lower = path.to_ascii_lowercase();

    // Content-based binary detection: read bytes first for reliable detection
    // (runs BEFORE duplicate read check — reject binary files immediately)
    {
        let bytes_check = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return format!("Error reading '{path}': {e}"),
        };
        if has_binary_extension(path) || is_binary_content(&bytes_check) {
            let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
            if !EXTRACTABLE_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
                return format!(
                    "Error: '{}' appears to be a binary file ({} bytes). Cannot read as text.",
                    path, bytes_check.len()
                );
            }
        }
    }

    // PDF files: extract text with optional page range
    if path_lower.ends_with(".pdf") {
        let pages_param = args.get("pages").and_then(|v| v.as_str()).unwrap_or("");
        return extract_pdf_with_pages(path, pages_param, MAX_READ_SIZE);
    }

    // DOCX files: extract text from ZIP/XML structure
    if path_lower.ends_with(".docx") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_docx_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // PPTX files: extract text from ZIP/XML structure
    if path_lower.ends_with(".pptx") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_pptx_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // XLSX/XLS files: extract spreadsheet data as tab-separated text
    if path_lower.ends_with(".xlsx") || path_lower.ends_with(".xls") || path_lower.ends_with(".xlsm") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_xlsx_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // EPUB ebooks: extract text from XHTML content files
    if path_lower.ends_with(".epub") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_epub_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // ODT (LibreOffice Writer): extract text from content.xml
    if path_lower.ends_with(".odt") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_odt_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // RTF: extract plain text
    if path_lower.ends_with(".rtf") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_rtf_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // ZIP archives: list contents
    if path_lower.ends_with(".zip") || path_lower.ends_with(".7z") || path_lower.ends_with(".tar.gz") || path_lower.ends_with(".tgz") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_zip_listing(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // CSV files: structured parsing with headers
    if path_lower.ends_with(".csv") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_csv_structured(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // Email files: extract headers, body, attachment listing
    if path_lower.ends_with(".eml") || path_lower.ends_with(".msg") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_eml_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // Parse offset/limit parameters
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let limit = args.get("limit").and_then(|v| v.as_u64()).map(|v| v as usize);

    // Duplicate read detection: if full re-read (no offset/limit) and file unchanged, return stub
    if offset == 0 && limit.is_none() {
        let current_mtime = get_file_mtime(path);
        if let Ok(cache) = read_file_cache().lock() {
            if let Some(&(cached_mtime, cached_lines)) = cache.get(path) {
                if current_mtime == Some(cached_mtime) {
                    return format!(
                        "File unchanged since last read ({} lines, ~{} tokens). \
                         The content from the earlier read is still current — use offset/limit \
                         to read specific sections, or search_files to find specific content.",
                        cached_lines, cached_lines * 10 / 4
                    );
                }
            }
        }
    }

    // Use LRU cache for file content (handles binary detection internally)
    let content = match read_file_cached(path) {
        Ok(c) => c,
        Err(e) => {
            // If cache returned a binary error, propagate it
            if e.starts_with("Error:") || e.starts_with("Binary file:") {
                return e;
            }
            // If reading failed, try encoding detection fallback
            match std::fs::read(path) {
                Ok(bytes) => return read_with_encoding_detection(&bytes, MAX_READ_SIZE),
                Err(e2) => return format!("Error reading '{path}': {e2}"),
            }
        }
    };

    // Cache file mtime for concurrent modification detection in edit_file
    if let Some(mtime) = get_file_mtime(path) {
        if let Ok(mut cache) = file_mtime_cache().lock() {
            cache.insert(path.to_string(), mtime);
        }
    }

    // Apply offset/limit and format with line numbers + token estimate
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Default cap: 2000 lines when no limit specified and file exceeds it
    const MAX_LINES_DEFAULT: usize = 2000;
    let was_truncated_by_cap = limit.is_none() && total_lines > MAX_LINES_DEFAULT;

    let start = if offset > 0 { (offset - 1).min(total_lines) } else { 0 };
    let effective_limit = limit.unwrap_or_else(|| {
        if total_lines > MAX_LINES_DEFAULT { MAX_LINES_DEFAULT } else { total_lines }
    });
    let end = (start + effective_limit).min(total_lines);

    let selected_lines = &lines[start..end];
    let selected_text: String = selected_lines.join("\n");

    // Estimate tokens (~4 chars per token)
    let estimated_tokens = (selected_text.len() + 3) / 4;

    // Build header
    let range_info = if offset > 0 || limit.is_some() {
        format!(" (lines {}-{})", start + 1, end)
    } else {
        String::new()
    };
    let header = format!(
        "[File: {} | {} lines{} | ~{} tokens]",
        path, total_lines, range_info, estimated_tokens
    );

    // Format with line numbers (cat -n style)
    let numbered: String = selected_lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:>6}\t{}", start + i + 1, line))
        .collect::<Vec<_>>()
        .join("\n");

    let mut result = format!("{}\n{}", header, numbered);

    // Append truncation notice if capped at MAX_LINES_DEFAULT
    if was_truncated_by_cap {
        result.push_str(&format!(
            "\n[File truncated at {} lines. Total: {} lines (~{} tokens). \
             Use offset and limit parameters to read specific portions, \
             or search_files to find specific content.]",
            MAX_LINES_DEFAULT, total_lines, total_lines * 10 / 4
        ));
    }

    // Cache this read for duplicate detection (mtime + line count)
    if let Some(mtime) = get_file_mtime(path) {
        if let Ok(mut cache) = read_file_cache().lock() {
            cache.insert(path.to_string(), (mtime, total_lines));
        }
    }

    // Apply truncation if still too large
    if result.len() > MAX_READ_SIZE {
        truncate_text_content(&result, MAX_READ_SIZE)
    } else {
        result
    }
}

/// Truncate text content to max_chars with a notice.
pub fn truncate_text_content(content: &str, max_chars: usize) -> String {
    let total_bytes = content.len();
    if total_bytes > max_chars {
        let mut end = max_chars;
        while end > 0 && !content.is_char_boundary(end) { end -= 1; }
        format!(
            "{}\n\n[Truncated: showing first {} of {} bytes]",
            &content[..end], end, total_bytes
        )
    } else {
        content.to_string()
    }
}

/// Read non-UTF8 bytes using encoding_rs auto-detection (Latin-1, Shift-JIS, etc.).
pub fn read_with_encoding_detection(bytes: &[u8], max_chars: usize) -> String {
    // Try common encodings: UTF-8 BOM, then let encoding_rs detect
    let (decoded, encoding_used, had_errors) = encoding_rs::Encoding::for_bom(bytes)
        .map(|(enc, _)| enc.decode(bytes))
        .unwrap_or_else(|| {
            // No BOM — try Windows-1252 (Latin-1 superset, most common non-UTF8 on Windows)
            encoding_rs::WINDOWS_1252.decode(bytes)
        });

    let label = if had_errors { " (with some decoding errors)" } else { "" };
    let header = format!("[Decoded from {} encoding{}]\n", encoding_used.name(), label);
    let content = format!("{}{}", header, decoded);
    truncate_text_content(&content, max_chars)
}

/// Write content to a file, creating parent directories as needed.
pub fn tool_write_file(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument is required".to_string(),
    };

    // Create parent directories if they don't exist
    if let Some(parent) = Path::new(path).parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("Error creating directories for '{path}': {e}");
            }
        }
    }

    match std::fs::write(path, content) {
        Ok(()) => {
            // Update mtime cache after successful write
            if let Some(mtime) = get_file_mtime(path) {
                if let Ok(mut cache) = file_mtime_cache().lock() {
                    cache.insert(path.to_string(), mtime);
                }
            }
            // Invalidate read cache and LRU content cache so next read_file returns fresh content
            invalidate_read_cache(path);
            invalidate_file_cache(path);
            format!("Written {} bytes to {}", content.len(), path)
        }
        Err(e) => format!("Error writing '{path}': {e}"),
    }
}

/// Normalize curly/smart quotes to straight quotes.
/// Models sometimes generate curly quotes which cause edit_file "not found" errors.
fn normalize_quotes(s: &str) -> String {
    s.replace('\u{2018}', "'")  // left single curly
     .replace('\u{2019}', "'")  // right single curly
     .replace('\u{201C}', "\"") // left double curly
     .replace('\u{201D}', "\"") // right double curly
}

/// Generate a compact unified diff between old and new content.
fn simple_diff(old: &str, new: &str, path: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut diff = format!("--- a/{}\n+++ b/{}\n", path, path);
    let mut has_changes = false;

    // Find changed regions
    let max_lines = old_lines.len().max(new_lines.len());
    let mut i = 0;
    while i < max_lines {
        let old_line = old_lines.get(i).copied().unwrap_or("");
        let new_line = new_lines.get(i).copied().unwrap_or("");

        if old_line != new_line {
            has_changes = true;
            // Show context: 2 lines before
            let ctx_start = i.saturating_sub(2);
            if ctx_start < i {
                for j in ctx_start..i {
                    if let Some(l) = old_lines.get(j) {
                        diff.push_str(&format!(" {}\n", l));
                    }
                }
            }

            // Find the extent of the change
            let mut change_end = i;
            while change_end < max_lines {
                let ol = old_lines.get(change_end).copied().unwrap_or("");
                let nl = new_lines.get(change_end).copied().unwrap_or("");
                if ol == nl && change_end > i { break; }
                change_end += 1;
            }

            // Output removed lines
            for j in i..change_end.min(old_lines.len()) {
                diff.push_str(&format!("-{}\n", old_lines[j]));
            }
            // Output added lines
            for j in i..change_end.min(new_lines.len()) {
                diff.push_str(&format!("+{}\n", new_lines[j]));
            }

            // Show 2 lines after
            for j in change_end..change_end.saturating_add(2).min(new_lines.len()) {
                diff.push_str(&format!(" {}\n", new_lines[j]));
            }

            i = change_end;
        } else {
            i += 1;
        }
    }

    if !has_changes {
        return "No visible changes in diff".to_string();
    }

    // Truncate if too long
    if diff.len() > 2000 {
        let mut end = 1800;
        while end < diff.len() && !diff.is_char_boundary(end) { end += 1; }
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

    // Check if file was modified since last read (concurrent modification detection)
    if let Some(cached_mtime) = file_mtime_cache().lock().ok().and_then(|c| c.get(path).copied()) {
        if let Some(current_mtime) = get_file_mtime(path) {
            if current_mtime > cached_mtime {
                eprintln!("[EDIT_FILE] File {} modified since last read (cached={}s, current={}s)", path, cached_mtime, current_mtime);
            }
        }
    }

    // Read the file
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return format!("Error reading '{path}': {e}"),
    };

    // Count occurrences
    let match_count = content.matches(old_string).count();
    if match_count == 0 {
        // Fallback: try with curly quotes normalized to straight quotes
        let norm_content = normalize_quotes(&content);
        let norm_old = normalize_quotes(old_string);
        let norm_count = norm_content.matches(&norm_old).count();
        if norm_count == 1 {
            let norm_new = normalize_quotes(new_string);
            let new_content = norm_content.replacen(&norm_old, &norm_new, 1);
            let match_pos = norm_content.find(&norm_old).unwrap();
            let line_num = norm_content[..match_pos].lines().count().max(1);
            // Save backup for undo_edit
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

    // Find the line number of the match for the success message
    let match_pos = content.find(old_string).unwrap();
    let line_num = content[..match_pos].lines().count().max(1);

    // Perform the replacement
    let new_content = content.replacen(old_string, new_string, 1);

    // Save backup for undo_edit
    let backup_path = format!("{path}.llama_bak");
    let _ = std::fs::write(&backup_path, &content);

    match std::fs::write(path, &new_content) {
        Ok(()) => {
            // Update mtime cache after successful edit
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

/// Undo the last edit_file operation by restoring the backup.
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

/// Insert text at a specific line number in a file.
pub fn tool_insert_text(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return "Error: 'text' argument is required".to_string(),
    };
    // Line number may be JSON number or string
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

    // Clamp insertion point: line 1 = before first line, line N+1 = after last line
    let insert_idx = (line - 1).min(total_lines);

    // Split the inserted text into lines
    let new_lines: Vec<&str> = text.lines().collect();
    let inserted_count = new_lines.len();

    for (i, new_line) in new_lines.into_iter().enumerate() {
        lines.insert(insert_idx + i, new_line);
    }

    // Preserve trailing newline if original had one
    let mut new_content = lines.join("\n");
    if content.ends_with('\n') {
        new_content.push('\n');
    }

    match std::fs::write(path, &new_content) {
        Ok(()) => format!("Inserted {inserted_count} line(s) at line {line} in {path}"),
        Err(e) => format!("Error writing '{path}': {e}"),
    }
}
