//! YOLO-based UI element detection from screenshots.
//!
//! Uses OmniParser's YOLO model (ONNX) to detect interactive UI elements
//! in screenshots. Combined with OCR, this enables desktop automation
//! for non-vision models by providing structured element lists.

use image::{GenericImageView, imageops::FilterType};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

use super::helpers::{tool_error, parse_int};
use super::NativeToolResult;

const MODEL_PATH: &str = "assets/yolo-model/ui-detect.onnx";
const INPUT_SIZE: usize = 640;
const CHANNELS: usize = 3;
const BBOX_FIELDS: usize = 5; // cx, cy, w, h, conf
const DEFAULT_CONFIDENCE: f32 = 0.15;
const NMS_IOU_THRESHOLD: f32 = 0.5;
const ROW_GROUP_PX: f32 = 40.;

static YOLO_SESSION: OnceLock<Result<Mutex<ort::session::Session>, String>> = OnceLock::new();

fn get_session() -> Result<&'static Mutex<ort::session::Session>, String> {
    let result = YOLO_SESSION.get_or_init(|| {
        let model_path = match find_model_path() {
            Ok(p) => p,
            Err(e) => return Err(e),
        };
        (|| -> Result<Mutex<ort::session::Session>, String> {
            let mut builder = ort::session::Session::builder()
                .map_err(|e| format!("ONNX session builder error: {e}"))?;
            builder = builder.with_intra_threads(2)
                .map_err(|e| format!("ONNX thread config error: {e}"))?;
            let session = builder.commit_from_file(&model_path)
                .map_err(|e| format!("Failed to load YOLO model from {model_path}: {e}"))?;
            Ok(Mutex::new(session))
        })()
    });
    result.as_ref().map_err(|e| e.clone())
}

fn find_model_path() -> Result<String, String> {
    if std::path::Path::new(MODEL_PATH).exists() {
        return Ok(MODEL_PATH.to_string());
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let p = dir.join("ui-detect.onnx");
        if p.exists() {
            return Ok(p.to_string_lossy().to_string());
        }
    }
    Err(format!("YOLO model not found at {MODEL_PATH}. Download ui-detect.onnx from the releases page."))
}

/// Detect UI elements in a screenshot image (raw RGBA bytes).
pub(crate) fn detect_ui_elements(
    img: &image::RgbaImage,
    confidence_threshold: f32,
) -> Result<Vec<UiElement>, String> {
    let session_lock = get_session()?;
    let mut session = session_lock.lock().map_err(|e| format!("Session lock error: {e}"))?;
    let (orig_w, orig_h) = img.dimensions();

    // Preprocess: resize to 640x640, normalize to [0,1], CHW layout (flat vec)
    let resized = image::DynamicImage::ImageRgba8(img.clone())
        .resize_exact(INPUT_SIZE as u32, INPUT_SIZE as u32, FilterType::CatmullRom);
    let mut input_data = vec![0f32; CHANNELS * INPUT_SIZE * INPUT_SIZE];
    for pixel in resized.pixels() {
        let x = pixel.0 as usize;
        let y = pixel.1 as usize;
        let [r, g, b, _] = pixel.2.0;
        let offset = y * INPUT_SIZE + x;
        input_data[offset] = r as f32 / 255.;                           // R channel
        input_data[INPUT_SIZE * INPUT_SIZE + offset] = g as f32 / 255.;  // G channel
        input_data[2 * INPUT_SIZE * INPUT_SIZE + offset] = b as f32 / 255.; // B channel
    }

    // Create tensor and run inference
    let start = std::time::Instant::now();
    let shape = vec![1i64, CHANNELS as i64, INPUT_SIZE as i64, INPUT_SIZE as i64];
    let input_tensor = ort::value::Tensor::from_array((shape, input_data))
        .map_err(|e| format!("ONNX tensor error: {e}"))?;
    let outputs = session.run(
        ort::inputs!["images" => input_tensor]
    ).map_err(|e| format!("YOLO inference error: {e}"))?;
    let inference_ms = start.elapsed().as_millis();

    // Extract output: shape [1, 5, 8400] as flat slice
    let output_value = outputs.get("output0").ok_or("Missing output0 tensor")?;
    let (shape_info, data) = output_value
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("Failed to extract output tensor: {e}"))?;

    // Verify shape
    let dims: Vec<usize> = shape_info.iter().map(|&d| d as usize).collect();
    if dims.len() < 3 || dims[1] < BBOX_FIELDS {
        return Err(format!("Unexpected output shape: {dims:?}, expected [1, 5, 8400]"));
    }
    let num_detections = dims[2];

    let scale_x = orig_w as f32 / INPUT_SIZE as f32;
    let scale_y = orig_h as f32 / INPUT_SIZE as f32;

    // Output layout is [1, 5, N] where dim 1 = [cx, cy, w, h, conf]
    // To get detection i: data[field * N + i]
    let mut boxes: Vec<[f32; 5]> = Vec::new();
    for i in 0..num_detections {
        let conf = data[4 * num_detections + i];
        if conf < confidence_threshold {
            continue;
        }
        let cx = data[i] * scale_x;
        let cy = data[num_detections + i] * scale_y;
        let w = data[2 * num_detections + i] * scale_x;
        let h = data[3 * num_detections + i] * scale_y;
        let x1 = (cx - w / 2.).max(0.);
        let y1 = (cy - h / 2.).max(0.);
        let x2 = (cx + w / 2.).min(orig_w as f32);
        let y2 = (cy + h / 2.).min(orig_h as f32);
        boxes.push([x1, y1, x2, y2, conf]);
    }

    // NMS: sort by confidence, greedily remove overlapping
    boxes.sort_by(|a, b| b[4].total_cmp(&a[4]));
    let mut kept = Vec::new();
    while let Some(best) = boxes.first().copied() {
        kept.push(best);
        boxes.retain(|b| iou_rect(&best, b) < NMS_IOU_THRESHOLD);
    }

    // Sort by position (top-to-bottom, left-to-right)
    kept.sort_by(|a, b| {
        let row_a = (a[1] / ROW_GROUP_PX) as i32;
        let row_b = (b[1] / ROW_GROUP_PX) as i32;
        row_a.cmp(&row_b).then(a[0].total_cmp(&b[0]))
    });

    let elements: Vec<UiElement> = kept
        .iter()
        .enumerate()
        .map(|(i, b)| UiElement {
            index: i,
            x1: b[0] as i32,
            y1: b[1] as i32,
            x2: b[2] as i32,
            y2: b[3] as i32,
            center_x: ((b[0] + b[2]) / 2.) as i32,
            center_y: ((b[1] + b[3]) / 2.) as i32,
            confidence: b[4],
            label: String::new(),
        })
        .collect();

    eprintln!(
        "[YOLO] Detected {} UI elements in {}ms (threshold={:.2})",
        elements.len(), inference_ms, confidence_threshold
    );

    Ok(elements)
}

/// A detected UI element with bounding box and optional OCR label.
#[derive(Debug, Clone)]
pub(crate) struct UiElement {
    pub index: usize,
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub center_x: i32,
    pub center_y: i32,
    pub confidence: f32,
    pub label: String,
}

impl UiElement {
    pub fn to_display_string(&self) -> String {
        if self.label.is_empty() {
            format!(
                "[{}] Element at ({},{})–({},{}) center=({},{}) conf={:.0}%",
                self.index, self.x1, self.y1, self.x2, self.y2,
                self.center_x, self.center_y, self.confidence * 100.
            )
        } else {
            format!(
                "[{}] \"{}\" at ({},{})–({},{}) center=({},{}) conf={:.0}%",
                self.index, self.label, self.x1, self.y1, self.x2, self.y2,
                self.center_x, self.center_y, self.confidence * 100.
            )
        }
    }
}

fn iou_rect(a: &[f32; 5], b: &[f32; 5]) -> f32 {
    let inter_x1 = a[0].max(b[0]);
    let inter_y1 = a[1].max(b[1]);
    let inter_x2 = a[2].min(b[2]);
    let inter_y2 = a[3].min(b[3]);
    let inter_area = (inter_x2 - inter_x1).max(0.) * (inter_y2 - inter_y1).max(0.);
    let area_a = (a[2] - a[0]) * (a[3] - a[1]);
    let area_b = (b[2] - b[0]) * (b[3] - b[1]);
    let union = area_a + area_b - inter_area;
    if union <= 0. { 0. } else { inter_area / union }
}

/// Tool: detect_ui_elements — scan screenshot for interactive UI elements.
pub fn tool_detect_ui_elements(args: &Value) -> NativeToolResult {
    let monitor_idx = parse_int(args.get("monitor").unwrap_or(&Value::Null)).unwrap_or(0) as usize;
    let confidence = args.get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(DEFAULT_CONFIDENCE as f64) as f32;
    let with_ocr = args.get("ocr")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // Capture screenshot
    let monitors = match super::helpers::cached_monitors() {
        Ok(m) => m,
        Err(e) => return tool_error("detect_ui_elements", e),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return tool_error("detect_ui_elements", format!("Monitor {} not found", monitor_idx)),
    };
    let capture = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => return tool_error("detect_ui_elements", format!("Screenshot failed: {e}")),
    };
    let img = image::RgbaImage::from_raw(capture.width(), capture.height(), capture.into_raw())
        .unwrap_or_default();

    // Run YOLO detection
    let mut elements = match detect_ui_elements(&img, confidence) {
        Ok(e) => e,
        Err(e) => return tool_error("detect_ui_elements", e),
    };

    // Optionally run OCR on each detected region
    if with_ocr && !elements.is_empty() {
        for elem in &mut elements {
            let x = elem.x1.max(0) as u32;
            let y = elem.y1.max(0) as u32;
            let w = ((elem.x2 - elem.x1).max(1) as u32).min(img.width().saturating_sub(x));
            let h = ((elem.y2 - elem.y1).max(1) as u32).min(img.height().saturating_sub(y));
            if w < 5 || h < 5 { continue; }
            let cropped = image::imageops::crop_imm(&img, x, y, w, h).to_image();
            if let Ok(text) = super::ocr_tools::ocr_image_ocrs(&cropped) {
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    elem.label = trimmed;
                }
            }
        }
    }

    // Format output
    let mut output = format!("Detected {} UI elements:\n", elements.len());
    for elem in &elements {
        output.push_str(&elem.to_display_string());
        output.push('\n');
    }
    output.push_str("\nTip: Use click_screen with center coordinates to interact with an element.");

    NativeToolResult::text_only(output)
}
