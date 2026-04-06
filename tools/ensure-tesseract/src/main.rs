//! Ensures Tesseract OCR is available for the project.
//!
//! Checks system PATH and common install locations first.
//! If not found, downloads and caches a portable copy.
//!
//! Usage:
//!   cargo run --manifest-path tools/ensure-tesseract/Cargo.toml --release

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

// Platform-specific download URLs
#[cfg(target_os = "windows")]
const TESSERACT_URL: &str = "https://github.com/UB-Mannheim/tesseract/releases/download/v5.4.0.20240606/tesseract-ocr-w64-setup-5.4.0.20240606.exe";

#[cfg(target_os = "macos")]
const TESSERACT_URL: &str = ""; // Will use brew bottle direct download

#[cfg(target_os = "linux")]
const TESSERACT_URL: &str = "https://github.com/AlexanderP/tesseract-appimage/releases/download/v5.5.2/tesseract-5.5.2_lept-1.87-x86_64.AppImage";

const TESSDATA_FAST_URL: &str = "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/main/eng.traineddata";

fn main() -> ExitCode {
    match ensure_tesseract() {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("WARNING: Tesseract setup failed: {e}");
            eprintln!("OCR will fall back to ocrs (Rust-native, lower accuracy).");
            // Don't fail the build — Tesseract is optional
            ExitCode::SUCCESS
        }
    }
}

fn ensure_tesseract() -> Result<PathBuf, String> {
    let cache_dir = auto_cache_dir();

    // 1. Check if already cached
    let bin = tesseract_binary_name();
    let cached_bin = cache_dir.join(&bin);
    if cached_bin.exists() {
        eprintln!("Tesseract found in cache: {}", cached_bin.display());
        return Ok(cache_dir);
    }

    // 2. Check system PATH
    if let Ok(output) = Command::new("tesseract").arg("--version").output() {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout);
            eprintln!("Tesseract found on PATH: {}", version.lines().next().unwrap_or("unknown"));
            return Ok(PathBuf::from("system"));
        }
    }

    // 3. Check common install locations
    if let Some(path) = find_system_tesseract() {
        eprintln!("Tesseract found at: {}", path.display());
        return Ok(path);
    }

    // 4. Download
    eprintln!("Tesseract not found — downloading portable copy...");
    fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create cache dir: {e}"))?;

    download_tesseract(&cache_dir)?;
    download_eng_traineddata(&cache_dir)?;

    eprintln!("Tesseract ready at {}", cache_dir.display());
    Ok(cache_dir)
}

fn auto_cache_dir() -> PathBuf {
    // Use target/tesseract-cache relative to the project root
    let mut dir = env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("."))
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    // Walk up to find the project root (contains Cargo.toml)
    for _ in 0..5 {
        if dir.join("Cargo.toml").exists() {
            return dir.join("target").join("tesseract-cache");
        }
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        }
    }
    PathBuf::from("target/tesseract-cache")
}

fn tesseract_binary_name() -> &'static str {
    if cfg!(windows) { "tesseract.exe" } else { "tesseract" }
}

fn find_system_tesseract() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let candidates = [
            r"C:\Program Files\Tesseract-OCR",
            r"C:\Program Files (x86)\Tesseract-OCR",
        ];
        for dir in &candidates {
            let path = PathBuf::from(dir);
            if path.join("tesseract.exe").exists() {
                return Some(path);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let candidates = [
            "/usr/local/bin/tesseract",
            "/opt/homebrew/bin/tesseract",
        ];
        for path in &candidates {
            if Path::new(path).exists() {
                return Some(PathBuf::from(path).parent().unwrap().to_path_buf());
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn download_tesseract(cache_dir: &Path) -> Result<(), String> {
    let installer_path = cache_dir.join("tesseract-installer.exe");

    // Download installer
    eprintln!("Downloading Tesseract installer (~48MB)...");
    download_file(TESSERACT_URL, &installer_path)?;

    // Silent install to cache dir
    eprintln!("Extracting Tesseract (silent install to cache)...");
    let status = Command::new("cmd")
        .args(["/C", &format!(
            "\"{}\" /S /D={}",
            installer_path.display(),
            cache_dir.display()
        )])
        .status()
        .map_err(|e| format!("Failed to run installer: {e}"))?;

    if !status.success() {
        // Try alternative: run directly (might need different quoting)
        let status2 = Command::new(&installer_path)
            .args(["/S", &format!("/D={}", cache_dir.display())])
            .status()
            .map_err(|e| format!("Failed to run installer (attempt 2): {e}"))?;

        if !status2.success() {
            return Err("Tesseract installer failed. You may need to install it manually.".into());
        }
    }

    // Clean up installer
    let _ = fs::remove_file(&installer_path);
    Ok(())
}

#[cfg(target_os = "linux")]
fn download_tesseract(cache_dir: &Path) -> Result<(), String> {
    let appimage_path = cache_dir.join("tesseract");

    // Download AppImage
    eprintln!("Downloading Tesseract AppImage (~23MB)...");
    download_file(TESSERACT_URL, &appimage_path)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&appimage_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to set permissions: {e}"))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn download_tesseract(cache_dir: &Path) -> Result<(), String> {
    // Try brew install first (most reliable on macOS)
    eprintln!("Attempting to install Tesseract via Homebrew...");
    let status = Command::new("brew")
        .args(["install", "tesseract"])
        .status();

    match status {
        Ok(s) if s.success() => {
            eprintln!("Tesseract installed via Homebrew");
            return Ok(());
        }
        _ => {
            eprintln!("Homebrew not available, downloading binary...");
            // Fallback: download from conda-forge or other source
            return Err("macOS Tesseract auto-download not yet implemented. Install with: brew install tesseract".into());
        }
    }
}

fn download_eng_traineddata(cache_dir: &Path) -> Result<(), String> {
    let tessdata_dir = cache_dir.join("tessdata");
    let traineddata = tessdata_dir.join("eng.traineddata");

    if traineddata.exists() {
        return Ok(());
    }

    fs::create_dir_all(&tessdata_dir)
        .map_err(|e| format!("Failed to create tessdata dir: {e}"))?;

    eprintln!("Downloading eng.traineddata (~4MB)...");
    download_file(TESSDATA_FAST_URL, &traineddata)?;
    Ok(())
}

fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;

    let total = response.header("content-length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut reader = response.into_reader();
    let mut file = fs::File::create(dest)
        .map_err(|e| format!("Failed to create file: {e}"))?;

    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf)
            .map_err(|e| format!("Read error: {e}"))?;
        if n == 0 { break; }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Write error: {e}"))?;
        downloaded += n as u64;
        if total > 0 {
            eprint!("\r  {:.1}MB / {:.1}MB ({:.0}%)",
                downloaded as f64 / 1_048_576.0,
                total as f64 / 1_048_576.0,
                downloaded as f64 / total as f64 * 100.0);
        }
    }
    eprintln!();
    Ok(())
}
