//! Web fetching, HTML-to-markdown conversion, and document format extractors.

use serde_json::Value;
use std::io::Read;

use super::{WEB_FETCH_CACHE, MAX_FETCH_CHARS};

/// Fetch a web page using headless Chrome (JS-rendered), falling back to ureq.
/// Includes per-session URL cache and PDF text extraction.
pub(super) fn tool_web_fetch(args: &Value, use_htmd: bool) -> String {
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return "Error: 'url' argument is required".to_string(),
    };

    let max_chars = args
        .get("max_length")
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_FETCH_CHARS as u64) as usize;

    // --- Cache check: return cached result if URL was already fetched this session ---
    if let Ok(cache) = WEB_FETCH_CACHE.lock() {
        if let Some(cached) = cache.get(url) {
            eprintln!("[WEB_FETCH] Cache hit for {url}");
            return cached.clone();
        }
    }

    // --- PDF detection: fetch raw bytes and extract text ---
    let url_lower = url.to_ascii_lowercase();
    if url_lower.ends_with(".pdf") || url_lower.contains(".pdf?") || url_lower.contains(".pdf#") {
        let result = fetch_pdf_text(url, max_chars);
        cache_result(url, &result);
        return result;
    }

    // When use_htmd is enabled, use htmd (HTML→Markdown) instead of html2text (HTML→plaintext)
    // This produces LLM-optimized markdown with links, headers, and formatting preserved
    if use_htmd {
        // Try Chrome first for JS-rendered content, convert to markdown via htmd
        match crate::web::browser::chrome_web_fetch_html(url) {
            Ok(html) if !html.is_empty() => {
                let result = html_to_markdown_truncated(&html, max_chars);
                cache_result(url, &result);
                return result;
            }
            Ok(_) => eprintln!("[WEB_FETCH/MD] Chrome returned empty HTML, trying ureq"),
            Err(e) => eprintln!("[WEB_FETCH/MD] Chrome failed: {e}, trying ureq"),
        }

        // Fallback: ureq fetch — check content-type for PDF before converting
        match fetch_bytes_with_content_type(url) {
            Ok((bytes, content_type)) => {
                if content_type.contains("application/pdf") {
                    let result = extract_pdf_text(&bytes, max_chars);
                    cache_result(url, &result);
                    return result;
                }
                let html = String::from_utf8_lossy(&bytes).to_string();
                let result = html_to_markdown_truncated(&html, max_chars);
                cache_result(url, &result);
                return result;
            }
            Err(e) => eprintln!("[WEB_FETCH/MD] ureq also failed: {e}, falling back to plain text"),
        }
    }

    // Default path: Try headless Chrome first (gets JS-rendered content)
    let chrome_timed_out = match crate::web::browser::chrome_web_fetch(url, max_chars) {
        Ok(content) if !content.is_empty() => {
            cache_result(url, &content);
            return content;
        }
        Ok(_) => {
            eprintln!("[WEB_FETCH] Chrome returned empty content, falling back to ureq");
            false
        }
        Err(e) => {
            let e_lower = e.to_lowercase();
            let timed_out = e_lower.contains("timed out") || e_lower.contains("timeout");
            eprintln!("[WEB_FETCH] Chrome failed: {e}, falling back to ureq");
            timed_out
        }
    };

    // Fallback: ureq-based fetch
    let result = tool_web_fetch_ureq(url, max_chars);

    // If both Chrome and ureq failed, prepend a clear timeout notice
    let result = if chrome_timed_out && result.starts_with("Error") {
        format!("Error: Request timed out. The URL '{url}' did not respond within the timeout period. {result}")
    } else {
        result
    };

    cache_result(url, &result);
    result
}

/// Store a web_fetch result in the per-session cache.
fn cache_result(url: &str, content: &str) {
    if let Ok(mut cache) = WEB_FETCH_CACHE.lock() {
        cache.insert(url.to_string(), content.to_string());
    }
}

/// Fetch a URL as raw bytes and return (bytes, content-type).
fn fetch_bytes_with_content_type(url: &str) -> Result<(Vec<u8>, String), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(15))
        .timeout_connect(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (compatible; LlamaChat/1.0)")
        .build();

    let response = agent.get(url).call().map_err(|e| format!("{e}"))?;
    let content_type = response.content_type().to_string();

    let mut buf = Vec::with_capacity(500_000);
    response
        .into_reader()
        .take(2_000_000) // 2MB limit for PDFs
        .read_to_end(&mut buf)
        .map_err(|e| format!("{e}"))?;

    Ok((buf, content_type))
}

/// Fetch a PDF URL and extract text.
fn fetch_pdf_text(url: &str, max_chars: usize) -> String {
    eprintln!("[WEB_FETCH/PDF] Detected PDF URL: {url}");
    match fetch_bytes_with_content_type(url) {
        Ok((bytes, _)) => extract_pdf_text(&bytes, max_chars),
        Err(e) => format!("Error fetching PDF: {e}"),
    }
}

/// Extract text from PDF bytes using pdf-extract.
pub fn extract_pdf_text(bytes: &[u8], max_chars: usize) -> String {
    match pdf_extract::extract_text_from_mem(bytes) {
        Ok(text) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                return "(PDF contains no extractable text — may be scanned/image-based)".to_string();
            }
            eprintln!("[WEB_FETCH/PDF] Extracted {} chars from PDF", text.len());
            if text.len() > max_chars {
                let mut end = max_chars;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}\n\n[PDF truncated at {} chars]", &text[..end], max_chars)
            } else {
                text
            }
        }
        Err(e) => format!("Error extracting PDF text: {e}"),
    }
}

/// Extract plain text from DOCX bytes (ZIP containing word/document.xml with <w:t> tags).
pub fn extract_docx_text(bytes: &[u8], max_chars: usize) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => return format!("Error reading DOCX archive: {e}"),
    };

    let mut xml_content = String::new();
    match archive.by_name("word/document.xml") {
        Ok(mut entry) => {
            if let Err(e) = entry.read_to_string(&mut xml_content) {
                return format!("Error reading DOCX document.xml: {e}");
            }
        }
        Err(e) => return format!("Error: not a valid DOCX file (missing word/document.xml): {e}"),
    }

    let mut reader = quick_xml::Reader::from_str(&xml_content);
    let mut text = String::new();
    let mut in_t = false;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                let local = e.local_name();
                if local.as_ref() == b"p" && !text.is_empty() {
                    text.push('\n');
                } else if local.as_ref() == b"t" {
                    in_t = true;
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                if e.local_name().as_ref() == b"t" {
                    in_t = false;
                }
            }
            Ok(quick_xml::events::Event::Text(e)) if in_t => {
                if let Ok(s) = e.unescape() {
                    text.push_str(&s);
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return format!("Error parsing DOCX XML: {e}"),
            _ => {}
        }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(DOCX contains no extractable text)".to_string();
    }

    eprintln!("[READ_FILE/DOCX] Extracted {} chars from DOCX", text.len());
    if text.len() > max_chars {
        let mut end = max_chars;
        while end > 0 && !text.is_char_boundary(end) { end -= 1; }
        format!("{}\n\n[DOCX truncated at {} chars]", &text[..end], max_chars)
    } else {
        text
    }
}

/// Extract plain text from PPTX bytes (ZIP containing ppt/slide*.xml with <a:t> tags).
pub fn extract_pptx_text(bytes: &[u8], max_chars: usize) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => return format!("Error reading PPTX archive: {e}"),
    };

    // Collect slide file names and sort them (slide1.xml, slide2.xml, ...)
    let mut slide_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .collect();
    slide_names.sort();

    if slide_names.is_empty() {
        return "(PPTX contains no slides)".to_string();
    }

    let mut text = String::new();
    for (idx, slide_name) in slide_names.iter().enumerate() {
        let mut xml_content = String::new();
        match archive.by_name(slide_name) {
            Ok(mut entry) => {
                if entry.read_to_string(&mut xml_content).is_err() {
                    continue;
                }
            }
            Err(_) => continue,
        }

        if !text.is_empty() { text.push('\n'); }
        text.push_str(&format!("--- Slide {} ---\n", idx + 1));

        // Extract text from <a:t> tags (DrawingML text elements)
        let mut reader = quick_xml::Reader::from_str(&xml_content);
        let mut in_t = false;
        let mut slide_has_text = false;

        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Start(e)) if e.local_name().as_ref() == b"t" => in_t = true,
                Ok(quick_xml::events::Event::End(e)) if e.local_name().as_ref() == b"t" => in_t = false,
                Ok(quick_xml::events::Event::Start(e)) if e.local_name().as_ref() == b"p" => {
                    if slide_has_text { text.push('\n'); }
                }
                Ok(quick_xml::events::Event::Text(e)) if in_t => {
                    if let Ok(s) = e.unescape() {
                        text.push_str(&s);
                        slide_has_text = true;
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(PPTX contains no extractable text)".to_string();
    }

    eprintln!("[READ_FILE/PPTX] Extracted {} chars from {} slides", text.len(), slide_names.len());
    super::truncate_text_content(&text, max_chars)
}

/// Extract spreadsheet data as tab-separated text from XLSX/XLS/XLSM bytes.
pub fn extract_xlsx_text(bytes: &[u8], max_chars: usize) -> String {
    use calamine::{Reader, Data};

    let cursor = std::io::Cursor::new(bytes);
    let mut workbook = match calamine::open_workbook_auto_from_rs(cursor) {
        Ok(wb) => wb,
        Err(e) => return format!("Error reading spreadsheet: {e}"),
    };

    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return "(Spreadsheet contains no sheets)".to_string();
    }

    let mut text = String::new();
    for sheet_name in &sheet_names {
        if let Ok(range) = workbook.worksheet_range(sheet_name) {
            if !text.is_empty() { text.push('\n'); }
            text.push_str(&format!("=== {} ===\n", sheet_name));

            for row in range.rows() {
                let row_values: Vec<String> = row.iter().map(|cell| match cell {
                    Data::String(s) => s.clone(),
                    Data::Int(i) => i.to_string(),
                    Data::Float(f) => {
                        // Show integers without decimal point
                        if *f == f.trunc() && f.abs() < 1e15 { format!("{}", *f as i64) } else { f.to_string() }
                    }
                    Data::Bool(b) => b.to_string(),
                    Data::DateTime(dt) => format!("{dt}"),
                    Data::DateTimeIso(s) => s.clone(),
                    Data::DurationIso(s) => s.clone(),
                    Data::Error(e) => format!("ERR:{e:?}"),
                    Data::Empty => String::new(),
                }).collect();
                text.push_str(&row_values.join("\t"));
                text.push('\n');
            }
        }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(Spreadsheet contains no data)".to_string();
    }

    eprintln!("[READ_FILE/XLSX] Extracted {} chars from {} sheets", text.len(), sheet_names.len());
    super::truncate_text_content(&text, max_chars)
}

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

    eprintln!("[READ_FILE/EPUB] Extracted {} chars from {} content files", text.len(), content_files.len());
    super::truncate_text_content(&text, max_chars)
}

/// Extract plain text from ODT bytes (OpenDocument Text: ZIP with content.xml containing <text:p> tags).
pub fn extract_odt_text(bytes: &[u8], max_chars: usize) -> String {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => return format!("Error reading ODT archive: {e}"),
    };

    let mut xml_content = String::new();
    match archive.by_name("content.xml") {
        Ok(mut entry) => {
            if let Err(e) = entry.read_to_string(&mut xml_content) {
                return format!("Error reading content.xml: {e}");
            }
        }
        Err(e) => return format!("Error finding content.xml in ODT: {e}"),
    }

    let mut reader = Reader::from_str(&xml_content);
    let mut text = String::new();
    let mut in_text_element = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                let name = std::str::from_utf8(local.as_ref()).unwrap_or("");
                // <text:p>, <text:h>, <text:span> contain text
                if name == "p" || name == "h" || name == "span" {
                    in_text_element = true;
                    if name == "p" || name == "h" {
                        if !text.is_empty() && !text.ends_with('\n') {
                            text.push('\n');
                        }
                    }
                }
                // <text:tab/> → tab, <text:line-break/> → newline
                if name == "tab" { text.push('\t'); }
                if name == "line-break" { text.push('\n'); }
            }
            Ok(Event::Text(ref e)) if in_text_element => {
                if let Ok(t) = e.unescape() {
                    text.push_str(&t);
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                let name = std::str::from_utf8(local.as_ref()).unwrap_or("");
                if name == "p" || name == "h" || name == "span" {
                    in_text_element = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                eprintln!("[READ_FILE/ODT] XML parse error: {e}");
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(ODT document contains no text)".to_string();
    }

    eprintln!("[READ_FILE/ODT] Extracted {} chars", text.len());
    super::truncate_text_content(&text, max_chars)
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
            eprintln!("[READ_FILE/RTF] Extracted {} chars", text.len());
            super::truncate_text_content(&text, max_chars)
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

    let mut text = format!("ZIP archive: {} entries\n\n", archive.len());
    let mut total_size: u64 = 0;

    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let size = entry.size();
            total_size += size;
            let kind = if entry.is_dir() { "DIR " } else { "FILE" };
            // Format size human-readable
            let size_str = if size >= 1_048_576 {
                format!("{:.1}MB", size as f64 / 1_048_576.0)
            } else if size >= 1024 {
                format!("{:.1}KB", size as f64 / 1024.0)
            } else {
                format!("{}B", size)
            };
            text.push_str(&format!("{} {:>8}  {}\n", kind, size_str, entry.name()));
        }
        if text.len() > max_chars { break; }
    }

    let total_str = if total_size >= 1_048_576 {
        format!("{:.1}MB", total_size as f64 / 1_048_576.0)
    } else if total_size >= 1024 {
        format!("{:.1}KB", total_size as f64 / 1024.0)
    } else {
        format!("{}B", total_size)
    };
    text.push_str(&format!("\nTotal uncompressed: {total_str}"));

    eprintln!("[READ_FILE/ZIP] Listed {} entries, total {}", archive.len(), total_str);
    super::truncate_text_content(&text, max_chars)
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

    eprintln!("[READ_FILE/CSV] Parsed {} rows", row_count);
    super::truncate_text_content(&text, max_chars)
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
                    text.push_str(&format!("{}: {}\n", key, val));
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
            atts.push(format!("{} ({})", name, part.ctype.mimetype));
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

    eprintln!("[READ_FILE/EML] Extracted {} chars, {} attachments", text.len(), attachments.len());
    super::truncate_text_content(&text, max_chars)
}

/// Convert HTML to LLM-optimized markdown using dom_smoothie + htmd, then truncate.
///
/// Pipeline: full HTML → dom_smoothie (extract article) → htmd (HTML→markdown)
/// Fallback: if readability fails → extract <body> → htmd → html2text
fn html_to_markdown_truncated(html: &str, max_chars: usize) -> String {
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
            // Fallback: extract <body> only (avoids <head> tag leakage)
            let body = extract_body_content(html);
            if body.is_empty() { html } else {
                // Need to return owned string — use a leak-free approach
                return convert_and_truncate(&body, max_chars);
            }
        }
    };

    convert_and_truncate(source, max_chars)
}

/// Convert HTML source to markdown and truncate.
fn convert_and_truncate(source: &str, max_chars: usize) -> String {
    let converter = htmd::HtmlToMarkdown::new();
    let markdown = converter.convert(source).unwrap_or_else(|e| {
        eprintln!("[WEB_FETCH/MD] htmd conversion failed: {e}, using raw html2text");
        html2text::from_read(source.as_bytes(), 120)
    });

    if markdown.is_empty() {
        return "(empty page)".to_string();
    }

    if markdown.len() > max_chars {
        let mut end = max_chars;
        while end > 0 && !markdown.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}\n\n[Truncated at {} chars]", &markdown[..end], max_chars)
    } else {
        markdown
    }
}

/// Extract content between <body> and </body> tags (case-insensitive).
fn extract_body_content(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<body").and_then(|i| lower[i..].find('>').map(|j| i + j + 1));
    let end = lower.rfind("</body>");
    match (start, end) {
        (Some(s), Some(e)) if s < e => html[s..e].to_string(),
        _ => String::new(), // fallback: convert whole HTML
    }
}

/// Fetch raw HTML via ureq (no html2text conversion).
/// Fallback web fetch via ureq HTTP client (used when Chrome is unavailable).
fn tool_web_fetch_ureq(url: &str, max_chars: usize) -> String {
    let result = crate::web::routes::tools::fetch_url_as_text(url, max_chars);

    if let Some(true) = result.get("success").and_then(|v| v.as_bool()) {
        result
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("(empty page)")
            .to_string()
    } else {
        let error = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        format!("Error fetching URL: {error}")
    }
}
