//! Miscellaneous format extractors: EPUB, RTF, ZIP, CSV, EML, and HTML→Markdown.
use std::io::Read;
use super::{convert_and_truncate, extract_body_content};

/// Extract plain text from EPUB bytes (ZIP containing XHTML content files).
pub fn extract_epub_text(bytes: &[u8], max_chars: usize) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => return format!("Error reading EPUB archive: {e}"),
    };

    // Collect XHTML/HTML content files (skip images, CSS, etc.)
    let mut content_files: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let name = entry.name().to_string();
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm") {
                content_files.push(name);
            }
        }
    }
    content_files.sort();

    let mut text = String::new();
    for name in &content_files {
        if let Ok(mut entry) = archive.by_name(name) {
            let mut html = String::new();
            if entry.read_to_string(&mut html).is_ok() {
                // Strip HTML tags to get plain text
                let plain = html2text::from_read(html.as_bytes(), 120);
                let trimmed = plain.trim();
                if !trimmed.is_empty() {
                    if !text.is_empty() { text.push_str("\n\n"); }
                    text.push_str(trimmed);
                }
            }
        }
        if text.len() > max_chars { break; }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(EPUB contains no readable text)".to_string();
    }

    let text_len = text.len();
    let file_count = content_files.len();
    eprintln!("[READ_FILE/EPUB] Extracted {text_len} chars from {file_count} content files");
    crate::truncate_text_content(&text, max_chars)
}

/// Extract plain text from RTF bytes.
pub fn extract_rtf_text(bytes: &[u8], max_chars: usize) -> String {
    let content = String::from_utf8_lossy(bytes);
    let tokens = match rtf_parser::lexer::Lexer::scan(&content) {
        Ok(t) => t,
        Err(e) => return format!("Error lexing RTF: {e}"),
    };
    let mut parser = rtf_parser::parser::Parser::new(tokens);
    match parser.parse() {
        Ok(doc) => {
            let text = doc.get_text();
            let text = text.trim().to_string();
            if text.is_empty() {
                return "(RTF document contains no text)".to_string();
            }
            let text_len = text.len();
            eprintln!("[READ_FILE/RTF] Extracted {text_len} chars");
            crate::truncate_text_content(&text, max_chars)
        }
        Err(e) => format!("Error parsing RTF: {e}"),
    }
}

/// List contents of a ZIP archive (file names and sizes).
pub fn extract_zip_listing(bytes: &[u8], max_chars: usize) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => return format!("Error reading ZIP archive: {e}"),
    };

    let entry_count = archive.len();
    let mut text = format!("ZIP archive: {entry_count} entries\n\n");
    let mut total_size: u64 = 0;

    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let size = entry.size();
            total_size += size;
            let kind = if entry.is_dir() { "DIR " } else { "FILE" };
            // Format size human-readable
            let size_str = if size >= 1_048_576 {
                let mb = size as f64 / 1_048_576.0;
                format!("{mb:.1}MB")
            } else if size >= 1024 {
                let kb = size as f64 / 1024.0;
                format!("{kb:.1}KB")
            } else {
                format!("{size}B")
            };
            let entry_name = entry.name();
            text.push_str(&format!("{kind} {size_str:>8}  {entry_name}\n"));
        }
        if text.len() > max_chars { break; }
    }

    let total_str = if total_size >= 1_048_576 {
        let mb = total_size as f64 / 1_048_576.0;
        format!("{mb:.1}MB")
    } else if total_size >= 1024 {
        let kb = total_size as f64 / 1024.0;
        format!("{kb:.1}KB")
    } else {
        format!("{total_size}B")
    };
    text.push_str(&format!("\nTotal uncompressed: {total_str}"));

    let zip_len = archive.len();
    eprintln!("[READ_FILE/ZIP] Listed {zip_len} entries, total {total_str}");
    crate::truncate_text_content(&text, max_chars)
}

/// Parse CSV file into a structured text representation with headers.
pub fn extract_csv_structured(bytes: &[u8], max_chars: usize) -> String {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(bytes);

    let mut text = String::new();

    // Headers
    if let Ok(headers) = reader.headers() {
        if !headers.is_empty() {
            text.push_str(&headers.iter().collect::<Vec<_>>().join("\t"));
            text.push('\n');
            // Separator line
            text.push_str(&headers.iter().map(|h| "-".repeat(h.len().max(3))).collect::<Vec<_>>().join("\t"));
            text.push('\n');
        }
    }

    // Rows
    let mut row_count = 0;
    for result in reader.records() {
        match result {
            Ok(record) => {
                text.push_str(&record.iter().collect::<Vec<_>>().join("\t"));
                text.push('\n');
                row_count += 1;
            }
            Err(e) => {
                text.push_str(&format!("[CSV parse error: {e}]\n"));
                break;
            }
        }
        if text.len() > max_chars { break; }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(CSV file is empty)".to_string();
    }

    eprintln!("[READ_FILE/CSV] Parsed {row_count} rows");
    crate::truncate_text_content(&text, max_chars)
}

/// Extract text from .eml email files (headers + body + attachment listing).
pub fn extract_eml_text(bytes: &[u8], max_chars: usize) -> String {
    let parsed = match mailparse::parse_mail(bytes) {
        Ok(m) => m,
        Err(e) => return format!("Error parsing email: {e}"),
    };

    let mut text = String::new();

    // Extract key headers
    for key in &["From", "To", "Cc", "Subject", "Date"] {
        for header in &parsed.headers {
            if header.get_key().eq_ignore_ascii_case(key) {
                let val = header.get_value();
                if !val.is_empty() {
                    text.push_str(&format!("{key}: {val}\n"));
                }
                break;
            }
        }
    }
    text.push_str("\n---\n\n");

    // Extract body text
    fn collect_body(part: &mailparse::ParsedMail, out: &mut String) {
        let ctype = part.ctype.mimetype.to_ascii_lowercase();
        if ctype == "text/plain" || ctype == "text/html" {
            if let Ok(body) = part.get_body() {
                let body_text = if ctype == "text/html" {
                    html2text::from_read(body.as_bytes(), 120)
                } else {
                    body
                };
                let trimmed = body_text.trim();
                if !trimmed.is_empty() {
                    out.push_str(trimmed);
                    out.push_str("\n\n");
                }
            }
        }
        for sub in &part.subparts {
            collect_body(sub, out);
        }
    }
    collect_body(&parsed, &mut text);

    // List attachments
    fn collect_attachments(part: &mailparse::ParsedMail, atts: &mut Vec<String>) {
        let disp = part.get_content_disposition();
        if disp.disposition == mailparse::DispositionType::Attachment {
            let name = disp.params.get("filename")
                .cloned()
                .unwrap_or_else(|| "(unnamed)".to_string());
            let mimetype = &part.ctype.mimetype;
            atts.push(format!("{name} ({mimetype})"));
        }
        for sub in &part.subparts {
            collect_attachments(sub, atts);
        }
    }
    let mut attachments = Vec::new();
    collect_attachments(&parsed, &mut attachments);
    if !attachments.is_empty() {
        text.push_str("Attachments:\n");
        for att in &attachments {
            text.push_str(&format!("  - {att}\n"));
        }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(Email contains no readable content)".to_string();
    }

    let text_len = text.len();
    let att_count = attachments.len();
    eprintln!("[READ_FILE/EML] Extracted {text_len} chars, {att_count} attachments");
    crate::truncate_text_content(&text, max_chars)
}

/// Convert HTML to LLM-optimized markdown using dom_smoothie + htmd, then truncate.
///
/// Pipeline: full HTML → dom_smoothie (extract article) → htmd (HTML→markdown)
/// Fallback: if readability fails → extract <body> → htmd → html2text
#[allow(dead_code)]
pub(super) fn html_to_markdown_truncated(html: &str, max_chars: usize) -> String {
    // Step 1: Try dom_smoothie readability extraction (strips nav, ads, footer)
    let article_html = match dom_smoothie::Readability::new(html, None, None) {
        Ok(mut reader) => match reader.parse() {
            Ok(article) => {
                let content = article.content;
                if content.trim().is_empty() {
                    eprintln!("[WEB_FETCH/MD] readability returned empty content, falling back");
                    None
                } else {
                    Some(content)
                }
            }
            Err(e) => {
                eprintln!("[WEB_FETCH/MD] readability parse failed: {e}, falling back");
                None
            }
        },
        Err(e) => {
            eprintln!("[WEB_FETCH/MD] readability init failed: {e}, falling back");
            None
        }
    };

    // Step 2: Convert to markdown via htmd
    let source = match &article_html {
        Some(article) => article,
        None => {
            let body = extract_body_content(html);
            if body.is_empty() {
                html
            } else {
                return convert_and_truncate(&body, max_chars);
            }
        }
    };

    convert_and_truncate(source, max_chars)
}
