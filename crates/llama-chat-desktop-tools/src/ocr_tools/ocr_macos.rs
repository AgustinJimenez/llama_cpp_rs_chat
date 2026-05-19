//! macOS Vision framework OCR backend (VNRecognizeTextRequest via embedded Swift).

use super::ocr_common::OcrMatch;

/// The embedded Swift script for plain text OCR via the Vision framework.
/// Expects the image file path as the first command-line argument.
/// Optional language hint as argument 2.
/// Outputs each recognized text observation on stdout, one per line.
#[cfg(target_os = "macos")]
const SWIFT_OCR_TEXT_SCRIPT: &str = r#"
import Foundation
import Vision

let args = CommandLine.arguments
guard args.count > 1 else {
    fputs("Usage: swift - <image_path> [language]\n", stderr)
    exit(1)
}
let imagePath = args[1]
guard let image = NSImage(contentsOfFile: imagePath),
      let tiffData = image.tiffRepresentation,
      let bitmap = NSBitmapImageRep(data: tiffData),
      let cgImage = bitmap.cgImage else {
    fputs("Error: could not load image at \(imagePath)\n", stderr)
    exit(1)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
if args.count > 2 {
    request.recognitionLanguages = [args[2]]
}

let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
do {
    try handler.perform([request])
} catch {
    fputs("Vision error: \(error.localizedDescription)\n", stderr)
    exit(1)
}

guard let observations = request.results else { exit(0) }
for observation in observations {
    if let candidate = observation.topCandidates(1).first {
        print(candidate.string)
    }
}
"#;

/// The embedded Swift script for OCR with bounding boxes via the Vision framework.
/// Expects the image file path as the first command-line argument.
/// Outputs one line per observation: TEXT\tX\tY\tW\tH\tCONFIDENCE (pixel coords, origin top-left).
/// The image width and height are passed as arguments 2 and 3.
/// Optional language hint as argument 4.
#[cfg(target_os = "macos")]
const SWIFT_OCR_FIND_SCRIPT: &str = r#"
import Foundation
import Vision

let args = CommandLine.arguments
guard args.count > 3 else {
    fputs("Usage: swift - <image_path> <width> <height> [language]\n", stderr)
    exit(1)
}
let imagePath = args[1]
let imgWidth = Double(args[2]) ?? 0
let imgHeight = Double(args[3]) ?? 0

guard let image = NSImage(contentsOfFile: imagePath),
      let tiffData = image.tiffRepresentation,
      let bitmap = NSBitmapImageRep(data: tiffData),
      let cgImage = bitmap.cgImage else {
    fputs("Error: could not load image at \(imagePath)\n", stderr)
    exit(1)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
if args.count > 4 {
    request.recognitionLanguages = [args[4]]
}

let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
do {
    try handler.perform([request])
} catch {
    fputs("Vision error: \(error.localizedDescription)\n", stderr)
    exit(1)
}

guard let observations = request.results else { exit(0) }
for observation in observations {
    if let candidate = observation.topCandidates(1).first {
        // boundingBox is normalized (0..1), origin bottom-left
        let box = observation.boundingBox
        let x = box.origin.x * imgWidth
        let y = (1.0 - box.origin.y - box.size.height) * imgHeight  // flip Y
        let w = box.size.width * imgWidth
        let h = box.size.height * imgHeight
        let conf = observation.confidence
        print("\(candidate.string)\t\(Int(x))\t\(Int(y))\t\(Int(w))\t\(Int(h))\t\(String(format: "%.4f", conf))")
    }
}
"#;

/// OCR via macOS Vision framework (VNRecognizeTextRequest).
/// Writes the image to a temp PNG, runs an embedded Swift script, parses stdout.
#[cfg(target_os = "macos")]
pub(crate) fn ocr_image_vision(img: &image::RgbaImage, language: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join("llama_chat_ocr_vision_tmp.png");
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    dyn_img.save(&tmp_path).map_err(|e| format!("Failed to save temp image: {e}"))?;

    let mut cmd = Command::new("swift");
    cmd.arg("-")
        .arg(tmp_path.to_str().unwrap_or(""));
    if let Some(lang) = language {
        cmd.arg(lang);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("swift not found: {e}")
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(SWIFT_OCR_TEXT_SCRIPT.as_bytes());
    }

    let output = child.wait_with_output().map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("swift execution failed: {e}")
    })?;
    let _ = std::fs::remove_file(&tmp_path);

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "Vision OCR failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// OCR with bounding boxes via macOS Vision framework.
/// Returns OcrMatch results with pixel coordinates (origin top-left).
#[cfg(target_os = "macos")]
pub(crate) fn ocr_find_text_vision(
    img: &image::RgbaImage,
    search: &str,
    offset_x: f64,
    offset_y: f64,
    language: Option<&str>,
) -> Result<Vec<OcrMatch>, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let (img_w, img_h) = (img.width(), img.height());
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join("llama_chat_ocr_vision_find_tmp.png");
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    dyn_img.save(&tmp_path).map_err(|e| format!("Failed to save temp image: {e}"))?;

    let mut cmd = Command::new("swift");
    cmd.arg("-")
        .arg(tmp_path.to_str().unwrap_or(""))
        .arg(img_w.to_string())
        .arg(img_h.to_string());
    if let Some(lang) = language {
        cmd.arg(lang);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("swift not found: {e}")
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(SWIFT_OCR_FIND_SCRIPT.as_bytes());
    }

    let output = child.wait_with_output().map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("swift execution failed: {e}")
    })?;
    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        return Err(format!(
            "Vision OCR failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let search_lower = search.to_lowercase();
    let mut matches = Vec::new();

    for line in stdout.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 {
            continue;
        }
        let text = cols[0];
        if !text.to_lowercase().contains(&search_lower) {
            continue;
        }
        let x: f64 = cols[1].parse().unwrap_or(0.0);
        let y: f64 = cols[2].parse().unwrap_or(0.0);
        let w: f64 = cols[3].parse().unwrap_or(0.0);
        let h: f64 = cols[4].parse().unwrap_or(0.0);
        let confidence: f64 = cols.get(5).and_then(|s| s.parse().ok()).unwrap_or(1.0);
        matches.push(OcrMatch {
            text: text.to_string(),
            x: x + offset_x,
            y: y + offset_y,
            width: w,
            height: h,
            center_x: x + w / 2.0 + offset_x,
            center_y: y + h / 2.0 + offset_y,
            confidence,
        });
    }

    Ok(matches)
}
