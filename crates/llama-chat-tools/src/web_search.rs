//! Web search via DuckDuckGo API with HTML scraping fallback.
//! Server-side HTTP requests — no browser needed, bypasses bot detection.

use serde_json::Value;

const MAX_SEARCH_RESULT_CHARS: usize = 8_000;

/// Search the web using DuckDuckGo, falling back to HTML scraping.
pub fn search(query: &str, max_results: usize) -> String {
    // Try DuckDuckGo Instant Answer API first
    match search_ddg_api(query, max_results) {
        Some(result) if !result.is_empty() => return result,
        _ => {
            eprintln!("[WEB_SEARCH] DDG API returned no results, trying HTML scraping");
        }
    }
    // Fallback: scrape DuckDuckGo HTML
    search_ddg_html(query, max_results)
}

/// DuckDuckGo Instant Answer API (structured JSON results).
fn search_ddg_api(query: &str, max_results: usize) -> Option<String> {
    let encoded = urlencoding::encode(query);
    let url = format!("https://api.duckduckgo.com/?q={encoded}&format=json&no_html=1&skip_disambig=1");

    let resp = ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0 (compatible; LlamaChat/1.0)")
        .call()
        .ok()?;

    let body = resp.into_string().ok()?;
    let data: Value = serde_json::from_str(&body).ok()?;

    let mut output = String::new();
    let mut count = 0;

    // Abstract (direct answer)
    if let Some(abstract_text) = data.get("AbstractText").and_then(|v| v.as_str()) {
        if !abstract_text.is_empty() {
            let source = data.get("AbstractSource").and_then(|v| v.as_str()).unwrap_or("");
            let url = data.get("AbstractURL").and_then(|v| v.as_str()).unwrap_or("");
            output.push_str(&format!("1. {source}\n   URL: {url}\n   {abstract_text}\n\n"));
            count += 1;
        }
    }

    // Related topics
    if let Some(topics) = data.get("RelatedTopics").and_then(|v| v.as_array()) {
        for topic in topics {
            if count >= max_results { break; }

            // Nested topic group
            if let Some(sub_topics) = topic.get("Topics").and_then(|v| v.as_array()) {
                for sub in sub_topics {
                    if count >= max_results { break; }
                    if let Some(line) = format_ddg_topic(sub, count + 1) {
                        output.push_str(&line);
                        count += 1;
                    }
                }
                continue;
            }

            if let Some(line) = format_ddg_topic(topic, count + 1) {
                output.push_str(&line);
                count += 1;
            }
        }
    }

    if count == 0 { return None; }

    if output.len() > MAX_SEARCH_RESULT_CHARS {
        output.truncate(MAX_SEARCH_RESULT_CHARS);
        output.push_str("\n...[truncated]");
    }

    Some(format!("Search results for '{query}':\n\n{output}"))
}

fn format_ddg_topic(topic: &Value, idx: usize) -> Option<String> {
    let text = topic.get("Text").and_then(|v| v.as_str()).unwrap_or("");
    let url = topic.get("FirstURL").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() && url.is_empty() { return None; }
    Some(format!("{idx}. {text}\n   URL: {url}\n\n"))
}

/// Fallback: scrape DuckDuckGo HTML results page.
fn search_ddg_html(query: &str, max_results: usize) -> String {
    let encoded = urlencoding::encode(query);
    let url = format!("https://html.duckduckgo.com/html/?q={encoded}");

    let resp = match ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .call()
    {
        Ok(r) => r,
        Err(e) => return format!("Search failed: {e}"),
    };

    let body = match resp.into_string() {
        Ok(b) => b,
        Err(e) => return format!("Search failed: {e}"),
    };

    // Parse HTML for result links
    let mut output = String::new();
    let mut count = 0;

    // DuckDuckGo HTML results have class="result__a" for title links
    // and class="result__snippet" for descriptions
    for line in body.lines() {
        if count >= max_results { break; }

        // Extract title + URL from result links
        if line.contains("result__a") {
            if let Some(href) = extract_attr(line, "href") {
                let title = extract_text_content(line);
                // DDG proxies URLs through their redirect
                let clean_url = if href.contains("uddg=") {
                    href.split("uddg=").nth(1)
                        .and_then(|u| urlencoding::decode(u).ok())
                        .map(|u| u.into_owned())
                        .unwrap_or(href)
                } else {
                    href
                };
                count += 1;
                output.push_str(&format!("{count}. {title}\n   URL: {clean_url}\n"));
            }
        }

        // Extract snippet
        if line.contains("result__snippet") {
            let snippet = extract_text_content(line);
            if !snippet.is_empty() {
                output.push_str(&format!("   {snippet}\n"));
            }
            output.push('\n');
        }
    }

    if count == 0 {
        return format!("No results found for '{query}'.");
    }

    if output.len() > MAX_SEARCH_RESULT_CHARS {
        output.truncate(MAX_SEARCH_RESULT_CHARS);
        output.push_str("\n...[truncated]");
    }

    format!("Search results for '{query}':\n\n{output}")
}

/// Extract an HTML attribute value from a tag string.
fn extract_attr(html: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = html.find(&pattern)? + pattern.len();
    let end = html[start..].find('"')? + start;
    Some(html[start..end].to_string())
}

/// Strip HTML tags and decode entities from a string.
fn extract_text_content(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}
