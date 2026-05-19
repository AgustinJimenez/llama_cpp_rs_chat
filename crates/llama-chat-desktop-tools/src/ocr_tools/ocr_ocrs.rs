//! ocrs (Rust-native) and PaddleOCR-VL (VLM subprocess) OCR backends.

use std::sync::Mutex;
use super::ocr_common::OcrMatch;

// ─── ocrs: pure Rust OCR engine (cross-platform, no external deps) ───────────

lazy_static::lazy_static! {
    static ref OCRS_ENGINE: Mutex<Option<ocrs::OcrEngine>> = Mutex::new(None);
}

/// Initialize the ocrs engine lazily (loads models on first use).
fn get_or_init_ocrs() -> Result<(), String> {
    let mut guard = OCRS_ENGINE.lock().map_err(|_| "OCR engine mutex poisoned")?;
    if guard.is_some() {
        return Ok(());
    }
    let models_dir = std::path::PathBuf::from("assets/ocr-models");
    let det_path = models_dir.join("text-detection.rten");
    let rec_path = models_dir.join("text-recognition.rten");
    if !det_path.exists() || !rec_path.exists() {
        return Err(format!("OCR models not found at {}", models_dir.display()));
    }
    let det_model = rten::Model::load_file(det_path)
        .map_err(|e| format!("Failed to load detection model: {e}"))?;
    let rec_model = rten::Model::load_file(rec_path)
        .map_err(|e| format!("Failed to load recognition model: {e}"))?;
    let engine = ocrs::OcrEngine::new(ocrs::OcrEngineParams {
        detection_model: Some(det_model),
        recognition_model: Some(rec_model),
        decode_method: ocrs::DecodeMethod::BeamSearch { width: 5 },
        ..Default::default()
    }).map_err(|e| format!("Failed to create OCR engine: {e}"))?;
    *guard = Some(engine);
    eprintln!("[OCR] ocrs engine initialized");
    Ok(())
}

/// Run OCR on an image using the ocrs (Rust-native) engine.
pub(crate) fn ocr_image_ocrs(img: &image::RgbaImage) -> Result<String, String> {
    get_or_init_ocrs()?;
    let guard = OCRS_ENGINE.lock().map_err(|_| "OCR engine mutex poisoned")?;
    let engine = guard.as_ref().ok_or("OCR engine not initialized")?;
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
    let (w, h) = rgb.dimensions();
    let source = ocrs::ImageSource::from_bytes(rgb.as_raw(), (w, h))
        .map_err(|e| format!("ImageSource error: {e}"))?;
    let input = engine.prepare_input(source)
        .map_err(|e| format!("prepare_input error: {e}"))?;
    engine.get_text(&input).map_err(|e| format!("get_text error: {e}"))
}

/// Run OCR with bounding boxes using the ocrs engine.
#[allow(dead_code)]
pub(crate) fn ocr_find_text_ocrs(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    get_or_init_ocrs()?;
    let guard = OCRS_ENGINE.lock().map_err(|_| "OCR engine mutex poisoned")?;
    let engine = guard.as_ref().ok_or("OCR engine not initialized")?;
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
    let (w, h) = rgb.dimensions();
    let source = ocrs::ImageSource::from_bytes(rgb.as_raw(), (w, h))
        .map_err(|e| format!("ImageSource error: {e}"))?;
    let input = engine.prepare_input(source)
        .map_err(|e| format!("prepare_input error: {e}"))?;
    let word_rects = engine.detect_words(&input)
        .map_err(|e| format!("detect_words error: {e}"))?;
    let line_rects = engine.find_text_lines(&input, &word_rects);
    let line_texts = engine.recognize_text(&input, &line_rects)
        .map_err(|e| format!("recognize_text error: {e}"))?;

    let search_lower = search.to_lowercase();
    let mut matches = Vec::new();
    for line in line_texts.iter().flatten() {
        let line_str = line.to_string();
        if line_str.to_lowercase().contains(&search_lower) {
            matches.push(OcrMatch {
                text: line_str,
                x: offset_x, y: offset_y,
                width: 0.0, height: 0.0,
                center_x: offset_x, center_y: offset_y,
                confidence: 1.0,
            });
        }
    }
    Ok(matches)
}

// ─── PaddleOCR-VL: Vision Language Model OCR (0.9B, runs on CPU) ─────────────

/// Find PaddleOCR-VL model files in assets/ocr-vlm/ or target cache.
#[allow(dead_code)]
fn find_vlm_ocr_model() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let candidates = [
        "assets/ocr-vlm",
        "target/ocr-vlm-cache",
    ];
    for dir in &candidates {
        let model_path = std::path::Path::new(dir).join("PaddleOCR-VL-1.5-Q8_0.gguf");
        let mmproj_path = std::path::Path::new(dir).join("mmproj-F16.gguf");
        if model_path.exists() && mmproj_path.exists() {
            return Some((model_path, mmproj_path));
        }
    }
    None
}

/// Run OCR on an image using PaddleOCR-VL (vision language model).
/// Loads the model on first use (~1.4s), then runs inference on CPU.
#[allow(dead_code)]
pub(crate) fn ocr_image_vlm(img: &image::RgbaImage) -> Result<String, String> {
    let (model_path, mmproj_path) = find_vlm_ocr_model()
        .ok_or("PaddleOCR-VL model not found in assets/ocr-vlm/")?;

    // Save image as PNG for the vision pipeline
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join("llama_chat_vlm_ocr_tmp.png");
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    dyn_img.save(&tmp_path).map_err(|e| format!("Failed to save temp image: {e}"))?;

    // Use llama-cli style subprocess approach since we can't load two models
    // in our single-worker architecture. Run a quick inference in a child process.
    let our_exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let output = std::process::Command::new(&our_exe)
        .arg("--vlm-ocr")
        .arg("--model").arg(&model_path)
        .arg("--mmproj").arg(&mmproj_path)
        .arg("--image").arg(&tmp_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run VLM OCR subprocess: {e}"))?;

    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("VLM OCR failed: {}", stderr.trim()));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        Err("VLM OCR returned empty result".into())
    } else {
        Ok(text)
    }
}
