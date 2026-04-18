//! Web search tools — Brave, Google Chrome, DuckDuckGo API, and ureq fallback.

use serde_json::Value;

/// Maximum characters to return from web search results.
const MAX_SEARCH_RESULT_CHARS: usize = 8_000;

/// Search the web using DuckDuckGo Instant Answer API, falling back to HTML scraping.
pub(super) fn tool_web_search(args: &Value, provider: Option<&str>, api_key: Option<&str>, backend: &crate::web::browser::BrowserBackend) -> String {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return "Error: 'query' argument is required".to_string(),
    };

    let max_results = args
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    match provider {
        Some("Brave") => {
            eprintln!("[WEB_SEARCH] Using Brave Search API");
            match tool_web_search_brave(query, max_results, api_key) {
                Ok(output) => output,
                Err(e) => {
                    if e.contains("missing") {
                        return format!("Error: {e}");
                    }
                    eprintln!("[WEB_SEARCH] Brave API failed: {e}, falling back to DDG");
                    tool_web_search_ureq(query, max_results)
                }
            }
        }
        Some("Google") => {
            eprintln!("[WEB_SEARCH] Using Google via browser backend");
            tool_web_search_google_chrome(query, max_results, backend)
        }
        // Camofox provider is handled in tool_web_search_with_vision() (supports CAPTCHA screenshots)
        Some("Camofox") => {
            eprintln!("[WEB_SEARCH] Camofox should be handled by tool_web_search_with_vision");
            tool_web_search_ureq(query, max_results)
        }
        _ => {
            // DuckDuckGo (default)
            // Try DuckDuckGo Instant Answer API first (reliable, no CAPTCHAs)
            match tool_web_search_ddg_api(query, max_results) {
                Some(result) if !result.is_empty() => return result,
                _ => {
                    eprintln!("[WEB_SEARCH] DDG API returned no results, trying HTML scraping");
                }
            }

            // Fallback: ureq-based DuckDuckGo HTML scraping
            tool_web_search_ureq(query, max_results)
        }
    }
}

/// Search using Brave Search API.
fn tool_web_search_brave(
    query: &str,
    max_results: usize,
    api_key: Option<&str>,
) -> Result<String, String> {
    let api_key = api_key
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            std::env::var("BRAVE_SEARCH_API_KEY")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
        .ok_or_else(|| "Error: Brave API key is missing".to_string())?;

    let count = max_results.clamp(1, 20);
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        urlencoding::encode(query),
        count
    );

    let response = ureq::get(&url)
        .set("Accept", "application/json")
        .set("X-Subscription-Token", &api_key)
        .call()
        .map_err(|e| format!("Error: Brave API request failed: {e}"))?;

    let body = response
        .into_string()
        .map_err(|e| format!("Error: Failed to read Brave API response: {e}"))?;

    let payload: Value = serde_json::from_str(&body)
        .map_err(|e| format!("Error: Failed to parse Brave API response: {e}"))?;

    let results = payload
        .get("web")
        .and_then(|v| v.get("results"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Extract image results (returned in the same response)
    let image_results = payload
        .get("images")
        .and_then(|v| v.get("results"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if results.is_empty() && image_results.is_empty() {
        return Ok(format!("Search results for '{query}' (via Brave):\n\n(no results)"));
    }

    let mut output = String::new();
    for (i, item) in results.iter().take(max_results).enumerate() {
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
        let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("").trim();
        let desc = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if title.is_empty() && url.is_empty() {
            continue;
        }

        output.push_str(&format!("{}. {}\n", i + 1, if title.is_empty() { url } else { title }));
        if !url.is_empty() {
            output.push_str(&format!("   URL: {url}\n"));
        }
        if !desc.is_empty() {
            output.push_str(&format!("   {desc}\n"));
        }
        output.push('\n');
    }

    // Append image results as markdown (rendered by frontend MD view)
    if !image_results.is_empty() {
        let max_images = 5.min(image_results.len());
        output.push_str(&format!("\nImages ({max_images} results):\n"));
        for item in image_results.iter().take(max_images) {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("Image");
            let thumb = item
                .get("thumbnail")
                .and_then(|v| v.get("src"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let source_url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if !thumb.is_empty() {
                output.push_str(&format!("![{title}]({thumb})\n"));
                if !source_url.is_empty() {
                    output.push_str(&format!("  Source: {source_url}\n"));
                }
            }
        }
        output.push('\n');
    }

    if output.len() > MAX_SEARCH_RESULT_CHARS {
        output.truncate(MAX_SEARCH_RESULT_CHARS);
        output.push_str("\n...[truncated]");
    }

    Ok(format!("Search results for '{query}' (via Brave):\n\n{output}"))
}

/// Search using Google via headless Chrome.
/// Navigates to Google search, extracts result titles, snippets, and URLs from the DOM.
fn tool_web_search_google_chrome(query: &str, max_results: usize, backend: &crate::web::browser::BrowserBackend) -> String {
    let url = format!(
        "https://www.google.com/search?q={}&num={}&hl=en",
        urlencoding::encode(query),
        max_results.min(10)
    );

    // Use Chrome to fetch the search results page
    match crate::web::browser::web_fetch(backend, &url, 50_000) {
        Ok(content) if !content.is_empty() => {
            // Detect CAPTCHA / bot block before parsing
            let content_lower = content.to_lowercase();
            if content_lower.contains("unusual traffic")
                || content_lower.contains("captcha")
                || content_lower.contains("please verify you are a human")
            {
                eprintln!("[WEB_SEARCH] Google CAPTCHA detected, falling back to DDG");
                return tool_web_search_ureq(query, max_results);
            }

            // Parse the text output for search results
            // Chrome returns html2text-formatted content from Google's SERP
            let mut output = String::new();
            let mut count = 0;

            // html2text converts Google results into a readable format.
            // We extract lines that look like result titles/URLs/snippets.
            let lines: Vec<&str> = content.lines().collect();
            let mut i = 0;
            while i < lines.len() && count < max_results {
                let line = lines[i].trim();

                // Detect result links: html2text renders [title][url] or title\nurl patterns
                // Google SERP in text form has patterns like:
                // "[Title](https://...)" or just "https://..." lines
                if let Some(link_start) = line.find("](http") {
                    // Markdown-style link: [Title](URL)
                    let title = &line[1..link_start];
                    let url_end = line[link_start + 2..].find(')').unwrap_or(line.len() - link_start - 2);
                    let result_url = &line[link_start + 2..link_start + 2 + url_end];

                    // Skip Google internal links, image links, cached links
                    if !result_url.contains("google.com")
                        && !result_url.contains("webcache")
                        && !title.is_empty()
                        && title.len() > 3
                    {
                        count += 1;
                        output.push_str(&format!("{count}. {title}\n"));
                        output.push_str(&format!("   URL: {result_url}\n"));

                        // Look for a snippet on the next few lines
                        for j in 1..=3 {
                            if i + j < lines.len() {
                                let next = lines[i + j].trim();
                                if !next.is_empty()
                                    && !next.starts_with('[')
                                    && !next.starts_with("http")
                                    && next.len() > 20
                                {
                                    output.push_str(&format!("   {next}\n"));
                                    break;
                                }
                            }
                        }
                        output.push('\n');
                    }
                }
                i += 1;
            }

            if output.is_empty() {
                // Fallback: return raw text content (truncated) if parsing failed
                let truncated = if content.len() > MAX_SEARCH_RESULT_CHARS {
                    let mut end = MAX_SEARCH_RESULT_CHARS;
                    while end > 0 && !content.is_char_boundary(end) { end -= 1; }
                    format!(
                        "{}...\n[Truncated: {} of {} chars]",
                        &content[..end],
                        end,
                        content.len()
                    )
                } else {
                    content
                };
                return format!(
                    "Search results for '{query}' (via Google):\n\n{truncated}"
                );
            }

            if output.len() > MAX_SEARCH_RESULT_CHARS {
                output.truncate(MAX_SEARCH_RESULT_CHARS);
                output.push_str("\n...[truncated]");
            }

            format!(
                "Search results for '{query}' (via Google):\n\n{output}\
                 Note: Use web_fetch to read specific URLs for more detail."
            )
        }
        Ok(_) => {
            eprintln!("[WEB_SEARCH] Google Chrome returned empty content, falling back to DDG");
            tool_web_search_ureq(query, max_results)
        }
        Err(e) => {
            let e_lower = e.to_lowercase();
            let timed_out = e_lower.contains("timed out") || e_lower.contains("timeout");
            eprintln!("[WEB_SEARCH] Google Chrome failed: {e}, falling back to DDG");
            let result = tool_web_search_ureq(query, max_results);
            if timed_out && (result.contains("No results") || result.starts_with("Error")) {
                format!("Error: Web search timed out for query '{query}'. The search engine did not respond in time. Try again or use a different query.")
            } else {
                result
            }
        }
    }
}

/// Search using DuckDuckGo Instant Answer API (knowledge-based results).
/// Returns structured results from Wikipedia, related topics, and direct answers.
pub(super) fn tool_web_search_ddg_api(query: &str, max_results: usize) -> Option<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(10))
        .timeout_connect(std::time::Duration::from_secs(5))
        .user_agent("Mozilla/5.0 (compatible; LlamaChat/1.0)")
        .build();

    let url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        urlencoding::encode(query)
    );

    let response = agent.get(&url).call().ok()?;
    let body = response.into_string().ok()?;
    let data: Value = serde_json::from_str(&body).ok()?;

    let mut output = String::new();
    let mut count = 0;

    // Main abstract (usually Wikipedia)
    let abstract_text = data["AbstractText"].as_str().unwrap_or("");
    let abstract_url = data["AbstractURL"].as_str().unwrap_or("");
    let abstract_source = data["AbstractSource"].as_str().unwrap_or("");
    let heading = data["Heading"].as_str().unwrap_or("");
    let abstract_image = data["Image"].as_str().unwrap_or("");

    if !abstract_text.is_empty() {
        count += 1;
        output.push_str(&format!("{count}. {heading}\n"));
        output.push_str(&format!("   Source: {abstract_source}\n"));
        if !abstract_url.is_empty() {
            output.push_str(&format!("   URL: {abstract_url}\n"));
        }
        output.push_str(&format!("   {abstract_text}\n"));
        // Include main image if available
        if !abstract_image.is_empty() {
            let img_url = if abstract_image.starts_with('/') {
                format!("https://duckduckgo.com{abstract_image}")
            } else {
                abstract_image.to_string()
            };
            output.push_str(&format!("   ![{heading}]({img_url})\n"));
        }
        output.push('\n');
    }

    // Direct answer (e.g., calculator, conversions)
    let answer = data["Answer"].as_str().unwrap_or("");
    if !answer.is_empty() {
        count += 1;
        output.push_str(&format!("{count}. Direct Answer\n"));
        output.push_str(&format!("   {answer}\n\n"));
    }

    // Related topics (additional knowledge links)
    if let Some(topics) = data["RelatedTopics"].as_array() {
        for topic in topics {
            if count >= max_results {
                break;
            }
            // Direct topic entry
            if let (Some(text), Some(url)) = (topic["Text"].as_str(), topic["FirstURL"].as_str())
            {
                count += 1;
                output.push_str(&format!("{count}. {text}\n"));
                output.push_str(&format!("   URL: {url}\n"));
                // Include topic icon if available
                if let Some(icon_url) = topic["Icon"]["URL"].as_str().filter(|u| !u.is_empty()) {
                    let full_url = if icon_url.starts_with('/') {
                        format!("https://duckduckgo.com{icon_url}")
                    } else {
                        icon_url.to_string()
                    };
                    // Skip tiny .ico files
                    if !full_url.ends_with(".ico") {
                        output.push_str(&format!("   ![icon]({full_url})\n"));
                    }
                }
                output.push('\n');
            }
            // Grouped topics (subcategories)
            if let Some(sub_topics) = topic["Topics"].as_array() {
                for sub in sub_topics {
                    if count >= max_results {
                        break;
                    }
                    if let (Some(text), Some(url)) =
                        (sub["Text"].as_str(), sub["FirstURL"].as_str())
                    {
                        count += 1;
                        output.push_str(&format!("{count}. {text}\n"));
                        output.push_str(&format!("   URL: {url}\n\n"));
                    }
                }
            }
        }
    }

    if output.is_empty() {
        return None;
    }

    if output.len() > MAX_SEARCH_RESULT_CHARS {
        output.truncate(MAX_SEARCH_RESULT_CHARS);
        output.push_str("\n...[truncated]");
    }

    Some(format!(
        "Search results for '{query}' (via DuckDuckGo):\n\n{output}\
         Note: These are knowledge-based results. Use web_fetch to read specific URLs for more detail."
    ))
}

/// Fallback web search via ureq HTTP scraping (used when Chrome is unavailable).
fn tool_web_search_ureq(query: &str, max_results: usize) -> String {
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(15))
        .timeout_connect(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build();

    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    let response = match agent.get(&url).call() {
        Ok(r) => r,
        Err(e) => return format!("Error: Failed to search DuckDuckGo: {e}"),
    };

    let body = match response.into_string() {
        Ok(b) => b,
        Err(e) => return format!("Error: Failed to read search response: {e}"),
    };

    // Try structured regex parsing first
    let results = parse_ddg_results(&body, max_results);
    if !results.is_empty() {
        return format!("Search results for '{query}':\n\n{results}");
    }

    // Fallback: use html2text for raw conversion
    let text = html2text::from_read(body.as_bytes(), 120);
    let truncated = if text.len() > MAX_SEARCH_RESULT_CHARS {
        let mut end = MAX_SEARCH_RESULT_CHARS;
        while end > 0 && !text.is_char_boundary(end) { end -= 1; }
        format!(
            "{}...\n[Truncated: first {} of {} chars]",
            &text[..end],
            end,
            text.len()
        )
    } else {
        text
    };

    format!("Search results for '{query}':\n\n{truncated}")
}

/// Parse DuckDuckGo HTML search results into structured text.
pub(super) fn parse_ddg_results(html: &str, max_results: usize) -> String {
    use regex::Regex;

    lazy_static::lazy_static! {
        static ref RESULT_LINK: Regex = Regex::new(
            r#"(?s)class="result__a"[^>]*href="([^"]*)"[^>]*>([^<]*)</a>"#
        ).unwrap();

        static ref RESULT_SNIPPET: Regex = Regex::new(
            r#"(?s)class="result__snippet"[^>]*>(.*?)</(?:a|td|div|span)"#
        ).unwrap();
    }

    let links: Vec<(String, String)> = RESULT_LINK
        .captures_iter(html)
        .take(max_results)
        .map(|cap| {
            let href = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let title = cap
                .get(2)
                .map(|m| m.as_str().trim())
                .unwrap_or("")
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#x27;", "'");
            (href.to_string(), title)
        })
        .collect();

    if links.is_empty() {
        return String::new();
    }

    let snippets: Vec<String> = RESULT_SNIPPET
        .captures_iter(html)
        .take(max_results)
        .map(|cap| {
            let raw = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            // Strip inner HTML tags from snippet
            let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();
            tag_re
                .replace_all(raw, "")
                .trim()
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#x27;", "'")
        })
        .collect();

    let mut output = String::new();
    for (i, (href, title)) in links.iter().enumerate() {
        let snippet = snippets.get(i).map(|s| s.as_str()).unwrap_or("");

        output.push_str(&format!("{}. {title}\n", i + 1));
        output.push_str(&format!("   URL: {href}\n"));
        if !snippet.is_empty() {
            output.push_str(&format!("   {snippet}\n"));
        }
        output.push('\n');
    }

    if output.len() > MAX_SEARCH_RESULT_CHARS {
        output.truncate(MAX_SEARCH_RESULT_CHARS);
        output.push_str("\n...[truncated]");
    }

    output
}
