//! UI Automation (UIA) interaction tool definitions.

use super::{p, Params, ToolDef};

pub static UI_AUTOMATION_TOOLS: &[ToolDef] = &[
    // ─── click_ui_element ───
    ToolDef {
        name: "click_ui_element",
        description: "Find a UI element by name and/or control type using UI Automation, then click its center. Works without screenshots — finds buttons, links, text fields by their accessible name.",
        params: Params::Simple(&[
            p("name", "string", "Element name to search for (case-insensitive substring match)"),
            p("control_type", "string", "Control type filter: Button, Edit, CheckBox, ComboBox, MenuItem, Hyperlink, etc."),
            p("title", "string", "Window title (default: active window)"),
            p("index", "integer", "Click the Nth match (0-based, default 0). Use with find_ui_elements to see all matches first."),
            p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
        ]),
        required: &[],
    },
    // ─── find_ui_elements ───
    ToolDef {
        name: "find_ui_elements",
        description: "Search for ALL UI elements matching name/control_type in a window. Returns positions, sizes, and element descriptions. Useful for discovering available UI controls.",
        params: Params::Simple(&[
            p("name", "string", "Element name filter (case-insensitive substring)"),
            p("control_type", "string", "Control type filter (button, edit, checkbox, etc.)"),
            p("title", "string", "Window title (default: active window)"),
            p("max_results", "integer", "Max elements to return (default 10, max 50)"),
        ]),
        required: &[],
    },
    // ─── invoke_ui_action ───
    ToolDef {
        name: "invoke_ui_action",
        description: "Invoke a UI Automation action on an element. Supports: invoke (click buttons), toggle (checkboxes), expand/collapse (tree nodes, dropdowns), select (list items), set_value (text fields). More reliable than coordinate clicking for standard Windows controls.",
        params: Params::Simple(&[
            p("name", "string", "Element name to match (case-insensitive substring)"),
            p("control_type", "string", "Control type filter (button, checkbox, edit, combobox, etc.)"),
            p("action", "string", "Action: invoke, toggle, expand, collapse, select, set_value"),
            p("value", "string", "Value for set_value action"),
            p("title", "string", "Window title (default: active window)"),
        ]),
        required: &["action"],
    },
    // ─── read_ui_element_value ───
    ToolDef {
        name: "read_ui_element_value",
        description: "Read the current text value of a UI element (text field, label, status bar, etc.) using UI Automation ValuePattern.",
        params: Params::Simple(&[
            p("name", "string", "Element name to match (case-insensitive substring)"),
            p("control_type", "string", "Control type filter"),
            p("title", "string", "Window title (default: active window)"),
        ]),
        required: &[],
    },
    // ─── wait_for_ui_element ───
    ToolDef {
        name: "wait_for_ui_element",
        description: "Wait until a UI element matching name/control_type appears in a window. Useful for waiting for dialogs, loading indicators, or UI state changes.",
        params: Params::Simple(&[
            p("name", "string", "Element name to wait for"),
            p("control_type", "string", "Control type to wait for"),
            p("title", "string", "Window title (default: active window)"),
            p("timeout_ms", "integer", "Max wait in ms (default 10000, max 30000)"),
            p("poll_ms", "integer", "Polling interval in ms (default 500, min 100)"),
        ]),
        required: &[],
    },
    // ─── type_into_element ───
    ToolDef {
        name: "type_into_element",
        description: "Find a UI element by name/type, click it to focus, then type text. Combines click_ui_element + type_text in one step.",
        params: Params::Simple(&[
            p("text", "string", "Text to type into the element"),
            p("name", "string", "Element name to find (case-insensitive substring)"),
            p("control_type", "string", "Control type filter (Edit, ComboBox, etc.)"),
            p("title", "string", "Window title (default: active window)"),
        ]),
        required: &["text"],
    },
    // ─── get_window_text ───
    ToolDef {
        name: "get_window_text",
        description: "Extract all text content from a window via UI Automation tree walk. Returns text from labels, edit fields, and documents. Useful for reading window content without OCR.",
        params: Params::Simple(&[
            p("title", "string", "Window title (default: active window)"),
            p("max_chars", "integer", "Max characters to return (default 50000)"),
        ]),
        required: &[],
    },
    // ─── file_dialog_navigate ───
    ToolDef {
        name: "file_dialog_navigate",
        description: "Navigate a file Open/Save dialog: sets the filename field and clicks the button. Useful for automating file selection in native dialogs.",
        params: Params::Simple(&[
            p("filename", "string", "File path or name to enter"),
            p("button", "string", "Button to click: Open, Save, etc. (default: Open)"),
            p("title", "string", "Dialog window title (auto-detected if omitted)"),
        ]),
        required: &["filename"],
    },
    // ─── drag_and_drop_element ───
    ToolDef {
        name: "drag_and_drop_element",
        description: "Find two UI elements by name/type and drag from one to the other. Combines find_ui_element + mouse_drag.",
        params: Params::Simple(&[
            p("from_name", "string", "Source element name"),
            p("from_type", "string", "Source control type"),
            p("to_name", "string", "Target element name"),
            p("to_type", "string", "Target control type"),
            p("title", "string", "Window title (default: active window)"),
            p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
        ]),
        required: &[],
    },
    // ─── scroll_element ───
    ToolDef {
        name: "scroll_element",
        description: "Find a UI element by name/type and scroll it. Uses mouse wheel at the element's center. Useful for scrolling specific panels or lists.",
        params: Params::Simple(&[
            p("name", "string", "Element name to find"),
            p("control_type", "string", "Control type filter"),
            p("direction", "string", "Scroll direction: up or down (default: down)"),
            p("amount", "integer", "Number of scroll clicks (default 3)"),
            p("title", "string", "Window title (default: active window)"),
        ]),
        required: &[],
    },
    // ─── hover_element ───
    ToolDef {
        name: "hover_element",
        description: "Hover over a UI element by name/type to trigger tooltip or hover effects. Returns tooltip text if found.",
        params: Params::Simple(&[
            p("name", "string", "UI element name (partial match)"),
            p("control_type", "string", "UI control type (Button, Edit, etc.)"),
            p("title", "string", "Window title filter (default: active window)"),
            p("hover_ms", "integer", "How long to hover before capturing (default 800)"),
        ]),
        required: &[],
    },
    // ─── handle_dialog ───
    ToolDef {
        name: "handle_dialog",
        description: "Detect and interact with modal dialogs. Lists dialog text and buttons, optionally clicks a button.",
        params: Params::Simple(&[
            p("button", "string", "Button name to click (e.g. 'OK', 'Cancel', 'Yes', 'Save')"),
        ]),
        required: &[],
    },
    // ─── wait_for_element_state ───
    ToolDef {
        name: "wait_for_element_state",
        description: "Wait until a UI element reaches a specific state (exists, gone, visible, hidden).",
        params: Params::Simple(&[
            p("name", "string", "UI element name (partial match)"),
            p("control_type", "string", "UI control type filter"),
            p("state", "string", "Target state: exists, gone, visible, hidden"),
            p("title", "string", "Window title filter"),
            p("timeout_ms", "integer", "Maximum wait time (default 5000)"),
        ]),
        required: &["state"],
    },
    // ─── find_and_click_text ───
    ToolDef {
        name: "find_and_click_text",
        description: "OCR the screen, find specific text, and click its center — all in one step. Combines ocr_find_text + click_screen. Use 'index' to click the Nth match.",
        params: Params::Simple(&[
            p("text", "string", "Text to find and click (case-insensitive)"),
            p("index", "integer", "Click the Nth match (0-based, default 0)"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
        ]),
        required: &["text"],
    },
    // ─── click_and_verify ───
    ToolDef {
        name: "click_and_verify",
        description: "Find text on screen via OCR, click it, then verify that different expected text appeared. Combines find_and_click_text + OCR verification.",
        params: Params::Simple(&[
            p("click_text", "string", "Text to find and click"),
            p("expect_text", "string", "Text expected to appear after clicking"),
            p("timeout_ms", "integer", "Maximum wait for verification (default 5000)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["click_text", "expect_text"],
    },
    // ─── get_context_menu ───
    ToolDef {
        name: "get_context_menu",
        description: "Right-click at coordinates to open a context menu, read menu items via UI Automation, and optionally click one. Returns a numbered list of menu items.",
        params: Params::Simple(&[
            p("x", "integer", "X coordinate to right-click"),
            p("y", "integer", "Y coordinate to right-click"),
            p("click_item", "string", "Menu item name to click (optional — just reads if omitted)"),
            p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
        ]),
        required: &["x", "y"],
    },
    // ─── wait_for_text_on_screen ───
    ToolDef {
        name: "wait_for_text_on_screen",
        description: "Poll OCR until specified text appears on screen. Useful for waiting for loading to complete, dialogs to appear, or status text changes.",
        params: Params::Simple(&[
            p("text", "string", "Text to wait for (case-insensitive)"),
            p("timeout_ms", "integer", "Max wait in ms (default 10000, max 30000)"),
            p("poll_ms", "integer", "Polling interval in ms (default 1000, min 500)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["text"],
    },
    // ─── wait_for_screen_change ───
    ToolDef {
        name: "wait_for_screen_change",
        description: "Wait until a screen region changes visually. Useful for waiting for loading indicators, animations, or content updates.",
        params: Params::Simple(&[
            p("x", "integer", "Region X (default 0)"),
            p("y", "integer", "Region Y (default 0)"),
            p("width", "integer", "Region width (default 200)"),
            p("height", "integer", "Region height (default 200)"),
            p("timeout_ms", "integer", "Max wait in ms (default 10000, max 30000)"),
            p("threshold", "number", "% of pixels that must change (default 5)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &[],
    },
    // ─── clear_field ───
    ToolDef {
        name: "clear_field",
        description: "Clear the currently focused input field (Ctrl+A → Delete). Optionally type new text after clearing.",
        params: Params::Simple(&[
            p("then_type", "string", "Text to type after clearing the field"),
            p("delay_ms", "integer", "Wait after action (default 200)"),
        ]),
        required: &[],
    },
];
