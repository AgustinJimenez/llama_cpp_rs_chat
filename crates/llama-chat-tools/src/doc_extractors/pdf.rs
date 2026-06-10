//! PDF text extraction (pdf_oxide + pdf-extract fallback).

/// Extract PDF text from bytes, trying pdf_oxide first then falling back to pdf-extract.
pub fn extract_pdf_text(bytes: &[u8], max_chars: usize) -> String {
    // Try pdf_oxide first (fast, handles more encodings) via temp file
    let oxide_result: Result<String, String> = extract_pdf_with_oxide(bytes, max_chars);
    if let Ok(text) = oxide_result {
        if !text.is_empty() {
            return text;
        }
    }
    // Fallback to pdf-extract for edge cases
    match pdf_extract::extract_text_from_mem(bytes) {
        Ok(text) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                return "(PDF contains no extractable text — may be scanned/image-based)".to_string();
            }
            let text_len = text.len();
            eprintln!("[PDF] Fallback pdf-extract: {text_len} chars");
            if text.len() > max_chars {
                let mut end = max_chars;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}\n\n[PDF truncated at {max_chars} chars]", &text[..end])
            } else {
                text
            }
        }
        Err(e) => format!("Error extracting PDF text: {e}"),
    }
}

/// Extract PDF text with page range support. Returns text + page info header.
/// `pages_param` examples: "", "1-5", "3", "10-20"
pub fn extract_pdf_with_pages(path: &str, pages_param: &str, max_chars: usize) -> String {
    let mut doc = match pdf_oxide::PdfDocument::open(path) {
        Ok(d) => d,
        Err(e) => return format!("Error opening PDF: {e}"),
    };
    let total_pages = doc.page_count().unwrap_or(0);
    if total_pages == 0 {
        return "(PDF has 0 pages or could not determine page count)".to_string();
    }

    // Parse page range
    let (start, end) = if pages_param.is_empty() {
        (0, total_pages)
    } else if let Some((a, b)) = pages_param.split_once('-') {
        let s = a.trim().parse::<usize>().unwrap_or(1).saturating_sub(1);
        let e = b.trim().parse::<usize>().unwrap_or(total_pages).min(total_pages);
        (s, e)
    } else if let Ok(p) = pages_param.trim().parse::<usize>() {
        (p.saturating_sub(1), p.min(total_pages))
    } else {
        (0, total_pages)
    };

    let mut text = String::new();
    let mut pages_read = 0;
    for i in start..end {
        match doc.extract_text(i) {
            Ok(t) => {
                if !t.trim().is_empty() {
                    let page_num = i + 1;
                    text.push_str(&format!("\n--- Page {page_num} ---\n"));
                    text.push_str(&t);
                    pages_read += 1;
                }
            }
            Err(_) => continue,
        }
        if text.len() > max_chars { break; }
    }

    if text.trim().is_empty() {
        return format!("(PDF contains no extractable text — may be scanned/image-based. Total pages: {total_pages})");
    }

    let page_start = start + 1;
    let header = format!("[PDF: {total_pages} total pages, showing pages {page_start}-{end} ({pages_read} pages with text)]\n");

    let text_trimmed = text.trim();
    let result = format!("{header}{text_trimmed}");
    if result.len() > max_chars {
        let mut end_pos = max_chars;
        while end_pos > 0 && !result.is_char_boundary(end_pos) { end_pos -= 1; }
        format!("{}\n\n[Truncated at {max_chars} chars — use pages parameter to read specific sections]", &result[..end_pos])
    } else {
        result
    }
}

/// Extract PDF text using pdf_oxide (writes to temp file since it only supports file paths).
pub(super) fn extract_pdf_with_oxide(bytes: &[u8], max_chars: usize) -> Result<String, String> {
    // Write bytes to temp file
    let pid = std::process::id();
    let tmp = std::env::temp_dir().join(format!("llama_pdf_{pid}.pdf"));
    std::fs::write(&tmp, bytes).map_err(|e| format!("temp write: {e}"))?;
    let result: Result<String, String> = (|| -> Result<String, String> {
        let mut doc = pdf_oxide::PdfDocument::open(&tmp).map_err(|e| format!("open: {e}"))?;
        let page_count = doc.page_count().unwrap_or(0);
        let mut text = String::new();
        for i in 0..page_count {
            match doc.extract_text(i) {
                Ok(t) => text.push_str(&t),
                Err(_) => continue,
            }
            if text.len() > max_chars { break; }
        }
        Ok(text)
    })();
    let _ = std::fs::remove_file(&tmp);
    let text = result?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Ok(String::new()); // Fall through to pdf-extract
    }
    let trimmed_len = trimmed.len();
    eprintln!("[PDF] pdf_oxide: {trimmed_len} chars extracted");
    if trimmed.len() > max_chars {
        let mut end = max_chars;
        while end > 0 && !trimmed.is_char_boundary(end) { end -= 1; }
        Ok(format!("{}\n\n[PDF truncated at {max_chars} chars]", &trimmed[..end]))
    } else {
        Ok(trimmed)
    }
}
