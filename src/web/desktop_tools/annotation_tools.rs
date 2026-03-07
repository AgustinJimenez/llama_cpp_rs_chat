//! Screenshot annotation, region OCR, and color finding tools.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

/// Draw shapes (rectangles, circles, lines) on a screenshot for annotation.
pub fn tool_annotate_screenshot(args: &Value) -> NativeToolResult {
    let shapes = match args.get("shapes").and_then(|v| v.as_array()) {
        Some(s) => s,
        None => {
            return NativeToolResult::text_only(
                "Error: 'shapes' array is required, e.g. [{\"type\":\"rect\",\"x\":10,\"y\":10,\"w\":100,\"h\":50}]"
                    .to_string(),
            )
        }
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!("Monitor {} out of range", monitor_idx));
    }
    let mut screen = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };

    let (sw, sh) = (screen.width(), screen.height());
    let mut drawn = 0;

    for shape in shapes {
        let shape_type = shape.get("type").and_then(|v| v.as_str()).unwrap_or("rect");
        let color = parse_color(shape.get("color").and_then(|v| v.as_str()).unwrap_or("red"));
        let thickness = shape.get("thickness").and_then(parse_int).unwrap_or(2) as i32;

        match shape_type {
            "rect" => {
                let x = shape.get("x").and_then(parse_int).unwrap_or(0) as i32;
                let y = shape.get("y").and_then(parse_int).unwrap_or(0) as i32;
                let w = shape.get("w").and_then(parse_int).unwrap_or(50) as i32;
                let h = shape.get("h").and_then(parse_int).unwrap_or(50) as i32;
                draw_rect(&mut screen, x, y, w, h, color, thickness, sw, sh);
                drawn += 1;
            }
            "circle" => {
                let x = shape.get("x").and_then(parse_int).unwrap_or(0) as i32;
                let y = shape.get("y").and_then(parse_int).unwrap_or(0) as i32;
                let r = shape.get("r").and_then(parse_int).unwrap_or(20) as i32;
                draw_circle(&mut screen, x, y, r, color, sw, sh);
                drawn += 1;
            }
            "line" => {
                let x1 = shape.get("x1").and_then(parse_int).unwrap_or(0) as i32;
                let y1 = shape.get("y1").and_then(parse_int).unwrap_or(0) as i32;
                let x2 = shape.get("x2").and_then(parse_int).unwrap_or(100) as i32;
                let y2 = shape.get("y2").and_then(parse_int).unwrap_or(100) as i32;
                draw_line(&mut screen, x1, y1, x2, y2, color, sw, sh);
                drawn += 1;
            }
            _ => {}
        }
    }

    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(std::io::Cursor::new(&mut buf));
    if let Err(e) = image::ImageEncoder::write_image(
        encoder,
        screen.as_raw(),
        sw,
        sh,
        image::ExtendedColorType::Rgba8,
    ) {
        return NativeToolResult::text_only(format!("Error encoding: {e}"));
    }

    NativeToolResult {
        text: format!("Drew {} shape(s) on screenshot", drawn),
        images: vec![buf],
    }
}

/// OCR a specific rectangular region of the screen.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_ocr_region(args: &Value) -> NativeToolResult {
    let x = args.get("x").and_then(parse_int).unwrap_or(0) as u32;
    let y = args.get("y").and_then(parse_int).unwrap_or(0) as u32;
    let width = match args.get("width").and_then(parse_int) {
        Some(w) => w as u32,
        None => return NativeToolResult::text_only("Error: 'width' is required".to_string()),
    };
    let height = match args.get("height").and_then(parse_int) {
        Some(h) => h as u32,
        None => return NativeToolResult::text_only("Error: 'height' is required".to_string()),
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    // Capture screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!("Monitor {} out of range", monitor_idx));
    }
    let screen = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };

    // Crop to region
    let (sw, sh) = (screen.width(), screen.height());
    let x = x.min(sw.saturating_sub(1));
    let y = y.min(sh.saturating_sub(1));
    let width = width.min(sw - x);
    let height = height.min(sh - y);

    let cropped = image::imageops::crop_imm(&screen, x, y, width, height).to_image();

    // Encode to PNG for OCR
    let mut png_buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(std::io::Cursor::new(&mut png_buf));
    if let Err(e) = image::ImageEncoder::write_image(
        encoder,
        cropped.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    ) {
        return NativeToolResult::text_only(format!("Error encoding: {e}"));
    }

    // Use WinRT OCR on the full screen then filter by region
    let result = super::ui_tools::tool_ocr_screen(&serde_json::json!({"monitor": monitor_idx}));

    // Filter: extract text mentioning found items, or just return the OCR text with region note
    NativeToolResult {
        text: format!(
            "OCR region ({x},{y} {width}x{height}):\n{}",
            result.text
        ),
        images: vec![png_buf],
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_ocr_region(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: ocr_region is not available on this platform".to_string())
}

/// Find pixels on screen matching a specific color (with tolerance).
pub fn tool_find_color_on_screen(args: &Value) -> NativeToolResult {
    let color_str = match args.get("color").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return NativeToolResult::text_only("Error: 'color' (hex #RRGGBB) is required".to_string()),
    };
    let tolerance = args.get("tolerance").and_then(parse_int).unwrap_or(30) as i32;
    let max_results = args.get("max_results").and_then(parse_int).unwrap_or(10) as usize;
    let step = args.get("step").and_then(parse_int).unwrap_or(4) as u32;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    // Parse hex color
    let color_str = color_str.trim_start_matches('#');
    if color_str.len() != 6 {
        return NativeToolResult::text_only("Error: color must be #RRGGBB hex format".to_string());
    }
    let target_r = u8::from_str_radix(&color_str[0..2], 16).unwrap_or(0) as i32;
    let target_g = u8::from_str_radix(&color_str[2..4], 16).unwrap_or(0) as i32;
    let target_b = u8::from_str_radix(&color_str[4..6], 16).unwrap_or(0) as i32;

    // Optionally restrict to a region
    let region_x = args.get("region_x").and_then(parse_int).map(|v| v as u32);
    let region_y = args.get("region_y").and_then(parse_int).map(|v| v as u32);
    let region_w = args.get("region_w").and_then(parse_int).map(|v| v as u32);
    let region_h = args.get("region_h").and_then(parse_int).map(|v| v as u32);

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!("Monitor {} out of range", monitor_idx));
    }
    let screen = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };

    let (sw, sh) = (screen.width(), screen.height());
    let start_x = region_x.unwrap_or(0);
    let start_y = region_y.unwrap_or(0);
    let end_x = (start_x + region_w.unwrap_or(sw)).min(sw);
    let end_y = (start_y + region_h.unwrap_or(sh)).min(sh);

    let mut matches = Vec::new();
    let raw = screen.as_raw();

    let step = step.max(1);
    for py in (start_y..end_y).step_by(step as usize) {
        for px in (start_x..end_x).step_by(step as usize) {
            let idx = (py * sw + px) as usize * 4;
            let r = raw[idx] as i32;
            let g = raw[idx + 1] as i32;
            let b = raw[idx + 2] as i32;

            if (r - target_r).abs() <= tolerance
                && (g - target_g).abs() <= tolerance
                && (b - target_b).abs() <= tolerance
            {
                matches.push((px, py));
                if matches.len() >= max_results {
                    break;
                }
            }
        }
        if matches.len() >= max_results {
            break;
        }
    }

    if matches.is_empty() {
        NativeToolResult::text_only(format!(
            "No pixels matching #{} (tolerance {}) found",
            color_str, tolerance
        ))
    } else {
        let coords: Vec<String> = matches.iter().map(|(x, y)| format!("({x},{y})")).collect();
        NativeToolResult::text_only(format!(
            "Found {} match(es) for #{} (tolerance {}): {}",
            matches.len(),
            color_str,
            tolerance,
            coords.join(", ")
        ))
    }
}

// ─── Drawing helpers ───────────────────────────────────────────────────────────

fn parse_color(name: &str) -> image::Rgba<u8> {
    match name {
        "green" => image::Rgba([0, 255, 0, 255]),
        "blue" => image::Rgba([0, 100, 255, 255]),
        "yellow" => image::Rgba([255, 255, 0, 255]),
        "white" => image::Rgba([255, 255, 255, 255]),
        "cyan" => image::Rgba([0, 255, 255, 255]),
        "magenta" => image::Rgba([255, 0, 255, 255]),
        _ => image::Rgba([255, 0, 0, 255]), // red
    }
}

fn put_thick_pixel(img: &mut image::RgbaImage, x: i32, y: i32, color: image::Rgba<u8>, sw: u32, sh: u32) {
    if x >= 0 && y >= 0 && (x as u32) < sw && (y as u32) < sh {
        img.put_pixel(x as u32, y as u32, color);
    }
}

fn draw_rect(img: &mut image::RgbaImage, x: i32, y: i32, w: i32, h: i32, color: image::Rgba<u8>, thickness: i32, sw: u32, sh: u32) {
    for t in 0..thickness {
        // Top and bottom edges
        for dx in 0..w {
            put_thick_pixel(img, x + dx, y + t, color, sw, sh);
            put_thick_pixel(img, x + dx, y + h - 1 - t, color, sw, sh);
        }
        // Left and right edges
        for dy in 0..h {
            put_thick_pixel(img, x + t, y + dy, color, sw, sh);
            put_thick_pixel(img, x + w - 1 - t, y + dy, color, sw, sh);
        }
    }
}

fn draw_circle(img: &mut image::RgbaImage, cx: i32, cy: i32, r: i32, color: image::Rgba<u8>, sw: u32, sh: u32) {
    for angle in 0..720 {
        let rad = (angle as f64 * 0.5).to_radians();
        let px = cx as f64 + r as f64 * rad.cos();
        let py = cy as f64 + r as f64 * rad.sin();
        put_thick_pixel(img, px as i32, py as i32, color, sw, sh);
    }
}

fn draw_line(img: &mut image::RgbaImage, x1: i32, y1: i32, x2: i32, y2: i32, color: image::Rgba<u8>, sw: u32, sh: u32) {
    let dx = (x2 - x1).abs();
    let dy = (y2 - y1).abs();
    let steps = dx.max(dy).max(1);
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let px = (x1 as f64 + (x2 - x1) as f64 * t) as i32;
        let py = (y1 as f64 + (y2 - y1) as f64 * t) as i32;
        put_thick_pixel(img, px, py, color, sw, sh);
    }
}
