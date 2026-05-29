//! Browser, web search, and in-app browser tool definitions.

use super::{p, Params, ToolDef};

pub static BROWSER_TOOLS: &[ToolDef] = &[
    // ─── open_url ───
    ToolDef {
        name: "open_url",
        description: "Open a URL in the user's external system browser outside the app. Only use this when the user explicitly asks to open something in their default browser or leave the in-app browser. Do NOT use this for web browsing, web search, reading pages, or screenshots inside the app — use browser_navigate and the other browser_* tools instead.",
        params: Params::Simple(&[
            p("url", "string", "The URL to open (must start with http:// or https://)"),
        ]),
        required: &["url"],
    },
    // ─── Browser view control (visible to user in chat UI) ───
    ToolDef {
        name: "open_browser_view",
        description: "Open the in-app browser view with a URL, visible to the user. Use this when you want to show the user a webpage directly in the chat interface. The user can see the page and interact with it. Useful for: showing search results, showing an article, or asking the user to solve a CAPTCHA.",
        params: Params::Simple(&[
            p("url", "string", "Full URL to navigate to (e.g. 'https://example.com')"),
        ]),
        required: &["url"],
    },
    ToolDef {
        name: "close_browser_view",
        description: "Close the in-app browser view. Call this when the user is done viewing the page or the CAPTCHA is solved.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── Unified browser control tools (work for both web and Tauri) ───
    ToolDef {
        name: "browser_navigate",
        description: "Open or navigate the in-app browser to a URL. Creates a new session if none exists. Use this to start any browser-based task. The page becomes visible to the user in the browser view. PARALLEL BROWSING: use different tab_id values (e.g. 'tab-1', 'tab-2') to open multiple pages simultaneously — open all links in parallel, then read each with browser_get_text(tab_id='tab-N').",
        params: Params::Simple(&[
            p("url", "string", "URL to navigate to (with or without https://)"),
            p("tab_id", "string", "Named browser tab (default: 'main'). Use different IDs for parallel sessions, e.g. 'tab-1', 'tab-2'."),
        ]),
        required: &["url"],
    },
    ToolDef {
        name: "browser_click",
        description: "Click an element in the browser using a CSS selector (e.g. 'button.submit', '#login', 'a[href*=\"signin\"]'). Returns immediately; effects appear in the next screenshot.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector of the element to click"),
            p("tab_id", "string", "Named browser tab (default: 'main')"),
        ]),
        required: &["selector"],
    },
    ToolDef {
        name: "browser_go_back",
        description: "Navigate the browser back to the previous page (equivalent to pressing the Back button). Use this after reading an article to return to the page you came from, instead of re-navigating to a URL.",
        params: Params::Simple(&[
            p("tab_id", "string", "Named browser tab (default: 'main')"),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_type",
        description: "Type text into an input field in the browser by CSS selector. Set press_enter=true to submit the form after typing.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector of the input field"),
            p("text", "string", "Text to type"),
            p("press_enter", "boolean", "Press Enter after typing (default: false)"),
            p("tab_id", "string", "Named browser tab (default: 'main')"),
        ]),
        required: &["selector", "text"],
    },
    ToolDef {
        name: "browser_query",
        description: "Extract structured data from the page as a JSON array. Use this to get all story titles + links from HN, all rows from a table, or any repeated elements. Examples: selector='.titleline > a' attributes='text,href' → [{text:'...', href:'...'}]; selector='table tr' attributes='text' → all row texts. Much better than browser_get_text for structured data.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector to match elements (e.g. '.titleline > a', 'article h2', 'table tr', '.score')"),
            p("attributes", "string", "Comma-separated attributes to extract: 'text' (innerText), 'href', 'src', 'class', 'id', 'html' (outerHTML), or any HTML attribute. Default: 'text'"),
            p("limit", "integer", "Max number of elements to return (default: 20)"),
            p("tab_id", "string", "Named browser tab (default: 'main')"),
        ]),
        required: &["selector"],
    },
    ToolDef {
        name: "browser_fetch_text",
        description: "Fetch the readable text from any URL via HTTP — WITHOUT navigating the browser or leaving the current page. Use this to read article content while keeping the browser on the page you're working with (e.g. stay on HN while reading each linked article). Faster than browser_navigate + browser_get_text for static pages. Returns up to max_chars characters (default: 8000). If the response includes `\"partial\": true`, the page requires JavaScript — use browser_navigate + browser_get_text instead.",
        params: Params::Simple(&[
            p("url", "string", "URL to fetch (must start with http:// or https://)"),
            p("max_chars", "integer", "Maximum characters to return (default: 8000)"),
        ]),
        required: &["url"],
    },
    ToolDef {
        name: "browser_search",
        description: "Search the web using a text query. Returns a list of results with titles, URLs, and snippets. Do NOT pass a URL here — use browser_navigate to open a specific URL.",
        params: Params::Simple(&[
            p("query", "string", "The search query"),
            p("max_results", "integer", "Max results to return (default: 8)"),
            p("tab_id", "string", "Named browser tab (default: 'main')"),
        ]),
        required: &["query"],
    },
    ToolDef {
        name: "browser_eval",
        description: "Evaluate arbitrary JavaScript in the browser page context and return the result. Use for complex queries that browser_query can't handle (computed styles, DOM manipulation, event dispatch). Return value must be JSON-serializable.",
        params: Params::Simple(&[
            p("js", "string", "JavaScript expression or async function body. Last expression is returned."),
            p("tab_id", "string", "Named browser tab (default: 'main')"),
        ]),
        required: &["js"],
    },
    ToolDef {
        name: "browser_get_html",
        description: "Get the raw HTML of the current page. Use browser_get_text for readable content — only use this when you need to inspect HTML structure, find specific attributes, or extract data that browser_query can't handle. Returns up to max_chars characters (default: 8000).",
        params: Params::Simple(&[
            p("max_chars", "integer", "Maximum characters to return (default: 8000). Lower for faster processing, higher for more detail."),
            p("tab_id", "string", "Named browser tab (default: 'main')"),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_wait",
        description: "Wait for a CSS selector to appear in the page (after navigation, AJAX load, etc.). Returns true if found, false on timeout.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector to wait for"),
            p("timeout_ms", "integer", "Max wait time in milliseconds (default: 5000)"),
            p("tab_id", "string", "Named browser tab (default: 'main')"),
        ]),
        required: &["selector"],
    },
    ToolDef {
        name: "browser_close",
        description: "Close a browser tab and free its resources. Omit tab_id (or use 'main') to close the entire browser.",
        params: Params::Simple(&[
            p("tab_id", "string", "Named tab to close (default: 'main' — closes the entire browser)"),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_get_text",
        description: "Get the visible text of the current page (HTML stripped). Returns up to max_chars characters (default: 8000). If the page is longer, the result ends with '[N chars remaining — call browser_get_text(offset=M) to continue]'. Use offset to paginate through long articles.",
        params: Params::Simple(&[
            p("offset", "integer", "Character offset to start reading from (default 0). Use the offset shown in the '[N chars remaining]' footer to read the next page."),
            p("max_chars", "integer", "Maximum characters to return (default: 8000). Lower for faster processing, higher for more detail."),
            p("tab_id", "string", "Named browser tab (default: 'main')"),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_get_links",
        description: "Get all links on the current page as [{text, href}] pairs (up to 200 links).",
        params: Params::Simple(&[]),
        required: &[],
    },
    ToolDef {
        name: "browser_snapshot",
        description: "Get all interactive elements on the page as [{tag, text, href, sel}]. 'sel' is a ready-to-use CSS selector for browser_click or browser_type. Use this BEFORE guessing selectors — it shows you exactly what's clickable. Use filter='.container' to scope to a specific part of the page (e.g. filter='.itemlist' on HN to get only story links). Default: 80 elements, 8000 chars.",
        params: Params::Simple(&[
            p("limit", "integer", "Max elements to return (default: 80). Lower = faster, less noise."),
            p("filter", "string", "CSS selector to scope the search (e.g. '.itemlist', 'nav', '#content'). Omit to search the whole page."),
            p("max_chars", "integer", "Max characters in the result (default: 8000)."),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_scroll",
        description: "Scroll the page visually. Use 'selector' to bring a specific element into view, or 'amount' for pixel offset. NOTE: page text content is not viewport-dependent — use browser_get_text(offset=N) to read beyond the first 30K chars instead of scrolling.",
        params: Params::Simple(&[
            p("amount", "integer", "Pixels to scroll (positive=down). Optional if selector given."),
            p("selector", "string", "CSS selector of element to scroll into view. Optional if amount given."),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_press_key",
        description: "Press a keyboard key in the active page. Examples: 'Enter', 'Tab', 'Escape', 'ArrowDown', 'PageDown', 'Control+a', 'Control+l'. Useful for forms and keyboard shortcuts.",
        params: Params::Simple(&[
            p("key", "string", "Key name (e.g. 'Enter', 'Tab', 'ArrowDown', 'Control+a')"),
        ]),
        required: &["key"],
    },
];
