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

#[path = "file_tools/editing.rs"]
mod editing;
pub use editing::{tool_edit_file, tool_insert_text, tool_multi_edit, tool_undo_edit};

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

    // Duplicate read detection: track whether this is a re-read of unchanged content
    // (used only to annotate the header — we always return the full content so context
    // compression cannot leave the model without the file it asked for)
    let is_duplicate_read = if offset == 0 && limit.is_none() {
        let current_mtime = get_file_mtime(path);
        if let Ok(cache) = read_file_cache().lock() {
            if let Some(&(cached_mtime, _)) = cache.get(path) {
                current_mtime == Some(cached_mtime)
            } else { false }
        } else { false }
    } else { false };

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
    let unchanged_note = if is_duplicate_read { " | unchanged since last read" } else { "" };
    let header = format!(
        "[File: {} | {} lines{} | ~{} tokens{}]",
        path, total_lines, range_info, estimated_tokens, unchanged_note
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
            "\n[File truncated at line {}. Total: {} lines (~{} tokens). \
             To read the next section: read_file(path=\"{}\", offset={}, limit={}). \
             Or use search_files to find specific content.]",
            end, total_lines, total_lines * 10 / 4,
            path, end + 1, MAX_LINES_DEFAULT
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
