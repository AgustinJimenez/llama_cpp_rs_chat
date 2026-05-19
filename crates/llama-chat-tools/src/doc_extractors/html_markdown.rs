pub(super) fn convert_and_truncate(source: &str, max_chars: usize) -> String {
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

pub(super) fn extract_body_content(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let start = lower
        .find("<body")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1));
    let end = lower.rfind("</body>");
    match (start, end) {
        (Some(s), Some(e)) if s < e => html[s..e].to_string(),
        _ => String::new(),
    }
}
