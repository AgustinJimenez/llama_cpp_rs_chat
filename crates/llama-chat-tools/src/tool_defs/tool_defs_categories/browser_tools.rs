//! Browser, web search, and Camofox tool definitions.

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
    // ─── Camofox CAPTCHA interaction tools ───
    ToolDef {
        name: "camofox_click",
        description: "Click an element on the active Camofox browser tab (used after a CAPTCHA is detected during web_search). Provide the element ref shown in the page snapshot (e.g. 'e1', 'e3'). Returns a screenshot of the updated page.",
        params: Params::Simple(&[
            p("ref", "string", "Element reference to click (e.g. 'e1', 'e3')"),
        ]),
        required: &["ref"],
    },
    ToolDef {
        name: "camofox_screenshot",
        description: "Take a screenshot of the active Camofox browser tab. Use this to see the current state of a CAPTCHA page after interacting with it.",
        params: Params::Simple(&[]),
        required: &[],
    },
    ToolDef {
        name: "camofox_type",
        description: "Type text into an input field on the active Camofox browser tab. Used during CAPTCHA solving if text input is needed.",
        params: Params::Simple(&[
            p("ref", "string", "Element reference of the input field (e.g. 'e2')"),
            p("text", "string", "Text to type"),
            p("press_enter", "boolean", "Whether to press Enter after typing (default: false)"),
        ]),
        required: &["ref", "text"],
    },
    // ─── Browser view control (visible to user in chat UI) ───
    ToolDef {
        name: "open_browser_view",
        description: "Open the in-app browser view with a URL, visible to the user. Use this when you want to show the user a webpage directly in the chat interface. The user can see the page and interact with it. Useful for: showing search results, showing an article, or asking the user to solve a CAPTCHA. Creates a new Camofox tab and displays it live.",
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
        description: "Open or navigate the in-app browser to a URL. Creates a new session if none exists. Use this to start any browser-based task. The page becomes visible to the user in the browser view.",
        params: Params::Simple(&[
            p("url", "string", "URL to navigate to (with or without https://)"),
        ]),
        required: &["url"],
    },
    ToolDef {
        name: "browser_click",
        description: "Click an element in the browser using a CSS selector (e.g. 'button.submit', '#login', 'a[href*=\"signin\"]'). Returns immediately; effects appear in the next screenshot.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector of the element to click"),
        ]),
        required: &["selector"],
    },
    ToolDef {
        name: "browser_type",
        description: "Type text into an input field in the browser by CSS selector. Set press_enter=true to submit the form after typing.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector of the input field"),
            p("text", "string", "Text to type"),
            p("press_enter", "boolean", "Press Enter after typing (default: false)"),
        ]),
        required: &["selector", "text"],
    },
    ToolDef {
        name: "browser_query",
        description: "Extract structured data from the page using CSS selectors. Returns an array of matched elements with the requested attributes. Much simpler than browser_eval for data extraction.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector to match elements (e.g. '.titleline > a', 'article h2', 'table tr')"),
            p("attributes", "string", "Comma-separated attributes to extract: 'text' (innerText), 'href', 'src', 'class', 'id', 'html' (outerHTML), or any HTML attribute. Default: 'text'"),
            p("limit", "integer", "Max number of elements to return (default: 20)"),
        ]),
        required: &["selector"],
    },
    ToolDef {
        name: "browser_search",
        description: "Search the web. Returns a list of results with titles, URLs, and snippets. Use this for web searches instead of navigating to search engines manually.",
        params: Params::Simple(&[
            p("query", "string", "The search query"),
            p("max_results", "integer", "Max results to return (default: 8)"),
        ]),
        required: &["query"],
    },
    ToolDef {
        name: "browser_eval",
        description: "Evaluate arbitrary JavaScript in the browser page context and return the result. Use for complex queries that browser_query can't handle (computed styles, DOM manipulation, event dispatch). Return value must be JSON-serializable.",
        params: Params::Simple(&[
            p("js", "string", "JavaScript expression or async function body. Last expression is returned."),
        ]),
        required: &["js"],
    },
    ToolDef {
        name: "browser_get_html",
        description: "Get the raw HTML of the current page (up to 50,000 chars). Use browser_get_text for readable content — only use this when you need to inspect HTML structure, find specific attributes, or extract data that browser_query can't handle.",
        params: Params::Simple(&[]),
        required: &[],
    },
    ToolDef {
        name: "browser_wait",
        description: "Wait for a CSS selector to appear in the page (after navigation, AJAX load, etc.). Returns true if found, false on timeout.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector to wait for"),
            p("timeout_ms", "integer", "Max wait time in milliseconds (default: 5000)"),
        ]),
        required: &["selector"],
    },
    ToolDef {
        name: "browser_close",
        description: "Close the active browser session and free its resources.",
        params: Params::Simple(&[]),
        required: &[],
    },
    ToolDef {
        name: "browser_get_text",
        description: "Get the visible text of the current page (HTML stripped). Returns up to 30,000 chars. If the page is longer, the result ends with '[N chars remaining — call browser_get_text(offset=M) to continue]'. Use offset to paginate through long articles.",
        params: Params::Simple(&[
            p("offset", "integer", "Character offset to start reading from (default 0). Use the offset shown in the '[N chars remaining]' footer to read the next page."),
            p("summary", "string", "Custom prompt for AI summarization of the page content (e.g. 'extract the pricing table' or 'summarize the main article'). Saves context tokens on large pages. Pass false to get raw text without summarization."),
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
        description: "Get the list of interactable elements on the page: buttons, links, inputs, selects. Each entry has {tag, text, href, sel} where 'sel' is a CSS selector you can pass to browser_click or browser_type. Use this to find cookie banners, form fields, navigation links, or any clickable element.",
        params: Params::Simple(&[]),
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
