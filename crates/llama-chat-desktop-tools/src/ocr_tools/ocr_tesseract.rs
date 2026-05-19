//! Tesseract CLI OCR backend: cross-platform text and bounding-box extraction.

use super::ocr_common::OcrMatch;

pub(crate) fn tesseract_install_hint() -> &'static str {
    if cfg!(windows) {
        "Install Tesseract for better OCR accuracy and retry with engine='tesseract'."
    } else if cfg!(target_os = "macos") {
        "Install Tesseract with 'brew install tesseract' for better OCR accuracy and retry with engine='tesseract'."
    } else {
        "Install Tesseract with your package manager (for example 'sudo apt install tesseract-ocr') and retry with engine='tesseract'."
    }
}

struct TesseractInstall {
    binary: String,
    tessdata_dir: Option<String>,
}

/// Find the tesseract binary — checks explicit env vars, PATH, then bundled/system locations.
fn find_tesseract_install() -> TesseractInstall {
    if let Ok(path) = std::env::var("LLAMA_CHAT_TESSERACT_PATH") {
        if !path.is_empty() && std::path::Path::new(&path).exists() {
            let tessdata_dir = std::env::var("LLAMA_CHAT_TESSDATA_DIR")
                .ok()
                .filter(|p| !p.is_empty() && std::path::Path::new(p).exists());
            return TesseractInstall {
                binary: path,
                tessdata_dir,
            };
        }
    }

    // Check if on PATH
    if let Ok(output) = std::process::Command::new("tesseract").arg("--version").output() {
        if output.status.success() {
            return TesseractInstall {
                binary: "tesseract".to_string(),
                tessdata_dir: None,
            };
        }
    }
    // Check target/tesseract-cache/ (auto-downloaded by ensure-tesseract)
    {
        let cache_bin = format!("target/tesseract-cache/{}", if cfg!(windows) { "tesseract.exe" } else { "tesseract" });
        if std::path::Path::new(&cache_bin).exists() {
            let tessdata_dir = std::path::Path::new("target/tesseract-cache/tessdata")
                .exists()
                .then(|| "target/tesseract-cache/tessdata".to_string());
            return TesseractInstall {
                binary: cache_bin,
                tessdata_dir,
            };
        }
    }

    // Common Windows install locations
    #[cfg(windows)]
    {
        let mut candidates = vec![
            r"C:\Program Files\Tesseract-OCR\tesseract.exe".to_string(),
            r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe".to_string(),
        ];

        if let Ok(resource_dir) = std::env::var("LLAMA_CHAT_RESOURCE_DIR") {
            candidates.extend([
                std::path::Path::new(&resource_dir)
                    .join("tesseract")
                    .join("tesseract.exe")
                    .to_string_lossy()
                    .into_owned(),
                std::path::Path::new(&resource_dir)
                    .join("tesseract")
                    .join("bin")
                    .join("tesseract.exe")
                    .to_string_lossy()
                    .into_owned(),
                std::path::Path::new(&resource_dir)
                    .join("assets")
                    .join("tesseract")
                    .join("tesseract.exe")
                    .to_string_lossy()
                    .into_owned(),
            ]);
        }

        for path in &candidates {
            if std::path::Path::new(path).exists() {
                let tessdata_dir = std::path::Path::new(path)
                    .parent()
                    .map(|dir| dir.join("tessdata"))
                    .filter(|dir| dir.exists())
                    .map(|dir| dir.to_string_lossy().into_owned());
                return TesseractInstall {
                    binary: path.to_string(),
                    tessdata_dir,
                };
            }
        }
        // Also check assets/tesseract/ (bundled)
        if std::path::Path::new("assets/tesseract/tesseract.exe").exists() {
            let tessdata_dir = std::path::Path::new("assets/tesseract")
                .join("tessdata");
            return TesseractInstall {
                binary: "assets/tesseract/tesseract.exe".to_string(),
                tessdata_dir: tessdata_dir.exists().then(|| tessdata_dir.to_string_lossy().into_owned()),
            };
        }
    }
    TesseractInstall {
        binary: "tesseract".to_string(),
        tessdata_dir: None,
    }
}

/// OCR via tesseract CLI (cross-platform: Windows, macOS, Linux).
/// On Windows, falls back to WinRT if tesseract is not installed.
pub(crate) fn ocr_image_tesseract(img: &image::RgbaImage, language: Option<&str>) -> Result<String, String> {
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join("llama_chat_ocr_tmp.png");
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    dyn_img.save(&tmp_path).map_err(|e| format!("Failed to save temp image: {e}"))?;
    let install = find_tesseract_install();
    let mut cmd = std::process::Command::new(&install.binary);
    cmd.arg(tmp_path.to_str().unwrap_or(""))
        .arg("stdout")
        .arg("--dpi").arg("300");
    if let Some(tessdata_dir) = &install.tessdata_dir {
        cmd.env("TESSDATA_PREFIX", tessdata_dir);
    }
    if let Some(lang) = language {
        cmd.arg("-l").arg(lang);
    }
    let output = cmd.output()
        .map_err(|e| format!("tesseract not found or failed: {e}. {}", tesseract_install_hint()))?;
    let _ = std::fs::remove_file(&tmp_path);
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!("tesseract error: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

/// OCR with bounding boxes via tesseract TSV output (cross-platform).
pub(crate) fn ocr_find_text_tesseract(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join("llama_chat_ocr_find_tmp.png");
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    dyn_img.save(&tmp_path).map_err(|e| format!("Failed to save temp image: {e}"))?;
    let install = find_tesseract_install();
    let mut cmd = std::process::Command::new(&install.binary);
    cmd.arg(tmp_path.to_str().unwrap_or(""))
        .arg("stdout")
        .arg("--psm").arg("3")
        .arg("tsv");
    if let Some(tessdata_dir) = &install.tessdata_dir {
        cmd.env("TESSDATA_PREFIX", tessdata_dir);
    }
    let output = cmd.output()
        .map_err(|e| format!("tesseract failed: {e}. {}", tesseract_install_hint()))?;
    let _ = std::fs::remove_file(&tmp_path);
    if !output.status.success() {
        return Err(format!("tesseract error: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let tsv = String::from_utf8_lossy(&output.stdout);
    let search_lower = search.to_lowercase();
    let mut matches = Vec::new();
    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 12 {
            let word = cols[11].trim();
            if word.to_lowercase().contains(&search_lower) {
                let x: f64 = cols[6].parse().unwrap_or(0.0);
                let y: f64 = cols[7].parse().unwrap_or(0.0);
                let w: f64 = cols[8].parse().unwrap_or(0.0);
                let h: f64 = cols[9].parse().unwrap_or(0.0);
                let confidence: f64 = cols[10].parse::<f64>().unwrap_or(0.0) / 100.0;
                matches.push(OcrMatch {
                    text: word.to_string(),
                    x: x + offset_x,
                    y: y + offset_y,
                    width: w,
                    height: h,
                    center_x: x + w / 2.0 + offset_x,
                    center_y: y + h / 2.0 + offset_y,
                    confidence,
                });
            }
        }
    }
    Ok(matches)
}
