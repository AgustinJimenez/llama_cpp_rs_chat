//! Screenshot, OCR, screen recording, and visual detection tool definitions.

use super::{p, Params, ToolDef};
use serde_json::{json, Value};

pub static SCREENSHOT_TOOLS: &[ToolDef] = &[
    // ─── take_screenshot ───
    ToolDef {
        name: "take_screenshot",
        description: "Capture a screenshot of the user's screen. Returns the file path and image dimensions. Use monitor=-1 to list available monitors without capturing.",
        params: Params::Simple(&[
            p("monitor", "integer", "Monitor index (0=primary, 1,2..=other monitors). Use -1 to list available monitors."),
        ]),
        required: &[],
    },
    // ─── screenshot_region ───
    ToolDef {
        name: "screenshot_region",
        description: "Capture a screenshot of a specific rectangular region of the screen. Returns the cropped image.",
        params: Params::Simple(&[
            p("x", "integer", "Left edge X coordinate"),
            p("y", "integer", "Top edge Y coordinate"),
            p("width", "integer", "Width of the region in pixels"),
            p("height", "integer", "Height of the region in pixels"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["x", "y", "width", "height"],
    },
    // ─── screenshot_diff ───
    ToolDef {
        name: "screenshot_diff",
        description: "Compare current screen to a baseline. First call with save_baseline=true to save, then call again to compare. Reports percentage of changed pixels and bounding box.",
        params: Params::Simple(&[
            p("save_baseline", "boolean", "If true, save current screen as baseline instead of comparing (default false)"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("highlight", "boolean", "Return image with red rectangle highlighting changed region"),
        ]),
        required: &[],
    },
    // ─── ocr_screen ───
    ToolDef {
        name: "ocr_screen",
        description: "Extract text from the screen using OCR. Returns recognized text with line structure. Works on any app including GPU-rendered ones where get_ui_tree returns empty. Use 'engine' to select OCR backend: 'auto' (default, tries best available), 'ocrs' (Rust-native, fast), 'tesseract' (most accurate, requires install), 'native' (Windows WinRT / macOS Vision). Prefer window/pid to auto-crop instead of scanning full monitor.",
        params: Params::Simple(&[
            p("engine", "string", "OCR engine: 'auto' (default), 'ocrs' (Rust-native), 'tesseract' (CLI), 'native'/'winrt' (platform built-in)"),
            p("window", "string", "Window title to auto-crop OCR to (case-insensitive)"),
            p("title", "string", "Alias for window title/process filter to auto-crop OCR to"),
            p("pid", "integer", "Specific process ID to auto-crop OCR to. Prefer this once you know the window identity."),
            p("x", "integer", "Left edge of region to OCR"),
            p("y", "integer", "Top edge of region to OCR"),
            p("width", "integer", "Width of region"),
            p("height", "integer", "Height of region"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("language", "string", "OCR language code (tesseract/macOS Vision only)"),
        ]),
        required: &[],
    },
    // ─── ocr_region ───
    ToolDef {
        name: "ocr_region",
        description: "Perform OCR on a specific rectangular region of the screen. Returns recognized text and the cropped region image.",
        params: Params::Simple(&[
            p("x", "integer", "Left edge X coordinate"),
            p("y", "integer", "Top edge Y coordinate"),
            p("width", "integer", "Region width in pixels"),
            p("height", "integer", "Region height in pixels"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["width", "height"],
    },
    // ─── ocr_find_text ───
    ToolDef {
        name: "ocr_find_text",
        description: "OCR the screen and find specific text, returning its bounding box coordinates. Prefer pid when you already know the target window identity; otherwise use window/title or a manual region to avoid scanning the full monitor.",
        params: Params::Simple(&[
            p("text", "string", "Text to search for (case-insensitive)"),
            p("window", "string", "Window title to auto-crop OCR search to (case-insensitive)"),
            p("title", "string", "Alias for window title/process filter to auto-crop OCR search to"),
            p("pid", "integer", "Specific process ID to auto-crop OCR search to. Prefer this once you know the window identity."),
            p("x", "integer", "Optional region X offset"),
            p("y", "integer", "Optional region Y offset"),
            p("width", "integer", "Optional region width"),
            p("height", "integer", "Optional region height"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("language", "string", "OCR language code (e.g. 'en-US', 'ja-JP'). macOS Vision only."),
        ]),
        required: &["text"],
    },
    // ─── detect_ui_elements ───
    ToolDef {
        name: "detect_ui_elements",
        description: "Detect all interactive UI elements on screen using YOLO vision model + OCR. Returns a numbered list of elements with labels and coordinates. Works on ANY app including GPU-rendered ones (Unreal, Unity, Blender). Use this instead of get_ui_tree when the app uses custom rendering. Each element includes center coordinates for use with click_screen.",
        params: Params::Simple(&[
            p("monitor", "integer", "Monitor index (default 0)"),
            p("confidence", "number", "Detection confidence threshold 0.0-1.0 (default 0.15)"),
            p("ocr", "boolean", "Run OCR on detected elements to read labels (default true)"),
        ]),
        required: &[],
    },
    // ─── window_screenshot ───
    ToolDef {
        name: "window_screenshot",
        description: "Capture a screenshot of a specific window by title. Smaller and more focused than a full screen screenshot.",
        params: Params::Simple(&[
            p("title", "string", "Window title or app name to capture (case-insensitive substring match)"),
        ]),
        required: &["title"],
    },
    // ─── find_image_on_screen ───
    ToolDef {
        name: "find_image_on_screen",
        description: "Find a template image on the screen using pixel matching (SSD). Returns the position and confidence if found. Useful for finding icons, buttons, or UI elements by their visual appearance.",
        params: Params::Simple(&[
            p("template", "string", "Path to the template image file (PNG, JPEG, etc.)"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("confidence", "number", "Minimum confidence threshold 0.0-1.0 (default 0.9)"),
            p("step", "integer", "Search step size in pixels — larger = faster but less precise (default 2)"),
        ]),
        required: &["template"],
    },
    // ─── find_color_on_screen ───
    ToolDef {
        name: "find_color_on_screen",
        description: "Find pixels on screen matching a specific color (hex #RRGGBB) within tolerance. Returns coordinates of matches.",
        params: Params::Simple(&[
            p("color", "string", "Target color in hex format #RRGGBB"),
            p("tolerance", "integer", "Color matching tolerance per channel 0-255 (default 30)"),
            p("max_results", "integer", "Maximum matches to return (default 10)"),
            p("step", "integer", "Pixel scan step size (default 4, use 1 for thorough)"),
            p("region_x", "integer", "Optional region left X"),
            p("region_y", "integer", "Optional region top Y"),
            p("region_w", "integer", "Optional region width"),
            p("region_h", "integer", "Optional region height"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["color"],
    },
    // ─── start_screen_recording ───
    ToolDef {
        name: "start_screen_recording",
        description: "Start recording the screen to a video file using ffmpeg. Call stop_screen_recording to finish.",
        params: Params::Simple(&[
            p("output_path", "string", "Output file path (e.g. 'recording.mp4')"),
            p("fps", "integer", "Frames per second (default 15)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["output_path"],
    },
    // ─── stop_screen_recording ───
    ToolDef {
        name: "stop_screen_recording",
        description: "Stop an active screen recording started by start_screen_recording.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── capture_gif ───
    ToolDef {
        name: "capture_gif",
        description: "Capture a short animated GIF of the screen (pure Rust, no ffmpeg needed).",
        params: Params::Simple(&[
            p("output_path", "string", "Output GIF file path"),
            p("duration_ms", "integer", "Recording duration in ms (default 3000)"),
            p("fps", "integer", "Frames per second (default 10)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["output_path"],
    },
    // ─── highlight_point ───
    ToolDef {
        name: "highlight_point",
        description: "Draw a crosshair marker on a screenshot at specified coordinates. Useful for debugging coordinate targeting.",
        params: Params::Simple(&[
            p("x", "integer", "X coordinate"),
            p("y", "integer", "Y coordinate"),
            p("color", "string", "Marker color: red, green, blue, yellow (default red)"),
            p("size", "integer", "Marker size in pixels (default 20)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["x", "y"],
    },
];

/// Complex screenshot tools that require runtime JSON construction.
pub fn complex_screenshot_tools() -> Vec<Value> {
    vec![
        // ─── annotate_screenshot — has complex array param ───
        json!({
            "name": "annotate_screenshot",
            "description": "Draw shapes (rectangles, circles, lines) on a screenshot for visual annotation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "shapes": {
                        "type": "array",
                        "description": "Array of shapes: {type: rect|circle|line, x, y, w, h, r, x1, y1, x2, y2, color, thickness}",
                        "items": { "type": "object" }
                    },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["shapes"]
            }
        }),
    ]
}
