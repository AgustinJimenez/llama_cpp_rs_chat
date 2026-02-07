use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Pinned CMake version — known-good LTS release.
const CMAKE_VERSION: &str = "3.31.6";

/// GitHub download URL template.
const CMAKE_URL_BASE: &str = "https://github.com/Kitware/CMake/releases/download";

fn main() {
    // Try to find cmake — system first, then cached download, then fresh download.
    ensure_cmake_available();

    // Tauri build step
    tauri_build::build()
}

// ---------------------------------------------------------------------------
// Top-level cmake resolution
// ---------------------------------------------------------------------------

fn ensure_cmake_available() {
    // 1. System cmake (PATH or well-known locations)
    if let Some(path) = find_system_cmake() {
        set_cmake_env(&path);
        return;
    }

    // 2. Previously downloaded cmake in target/cmake/
    let cache_dir = cmake_cache_dir();
    if let Some(path) = find_cached_cmake(&cache_dir) {
        set_cmake_env(&path);
        return;
    }

    // 3. Download portable cmake
    println!("cargo:warning=CMake not found on system — downloading portable CMake {CMAKE_VERSION}...");
    match download_and_extract_cmake(&cache_dir) {
        Ok(cmake_path) => {
            set_cmake_env(&cmake_path);
            println!("cargo:warning=CMake {CMAKE_VERSION} downloaded and ready at: {}", cmake_path.display());
        }
        Err(e) => {
            eprintln!("\n\
                ╔══════════════════════════════════════════════════════════════╗\n\
                ║  CMake auto-download failed: {e:<31} ║\n\
                ║                                                              ║\n\
                ║  Install CMake manually:                                     ║\n\
                ║    Windows:  winget install Kitware.CMake                     ║\n\
                ║    macOS:    brew install cmake                               ║\n\
                ║    Linux:    sudo apt install cmake                           ║\n\
                ╚══════════════════════════════════════════════════════════════╝\n");
            panic!("CMake is required but could not be found or downloaded.");
        }
    }
}

/// Point the `cmake` crate (and downstream llama-cpp-sys-2) at the given binary.
fn set_cmake_env(cmake_bin: &Path) {
    let cmake_str = cmake_bin.to_string_lossy();
    println!("cargo:warning=Using CMake at: {cmake_str}");
    env::set_var("CMAKE", &*cmake_str);

    // Add parent directory to PATH so child processes also find it
    if let Some(parent) = cmake_bin.parent() {
        if let Some(parent_str) = parent.to_str() {
            let current_path = env::var("PATH").unwrap_or_default();
            if !current_path.contains(parent_str) {
                let sep = if cfg!(windows) { ";" } else { ":" };
                env::set_var("PATH", format!("{parent_str}{sep}{current_path}"));
                println!("cargo:warning=Added {parent_str} to PATH");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System cmake detection
// ---------------------------------------------------------------------------

fn find_system_cmake() -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(windows) {
        &[
            "cmake",
            "C:\\Program Files\\CMake\\bin\\cmake.exe",
            "C:\\Program Files (x86)\\CMake\\bin\\cmake.exe",
        ]
    } else {
        &["cmake", "/usr/local/bin/cmake", "/opt/homebrew/bin/cmake"]
    };

    for &path in candidates {
        if let Ok(output) = Command::new(path).arg("--version").output() {
            if output.status.success() {
                println!("cargo:warning=Found system CMake at: {path}");
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Cached cmake detection
// ---------------------------------------------------------------------------

fn cmake_cache_dir() -> PathBuf {
    // Use OUT_DIR to find target/ — OUT_DIR is like target/debug/build/<pkg>/out
    // Walk up to find target/
    if let Ok(out_dir) = env::var("OUT_DIR") {
        let out = PathBuf::from(&out_dir);
        // Walk up looking for a directory named "target"
        let mut dir = out.as_path();
        while let Some(parent) = dir.parent() {
            if dir.file_name().map(|n| n == "target").unwrap_or(false) {
                return dir.join("cmake");
            }
            dir = parent;
        }
    }
    // Fallback: relative to crate root
    PathBuf::from("target/cmake")
}

fn find_cached_cmake(cache_dir: &Path) -> Option<PathBuf> {
    let cmake_bin = cached_cmake_binary(cache_dir);
    if cmake_bin.exists() {
        // Verify it actually runs
        if let Ok(output) = Command::new(&cmake_bin).arg("--version").output() {
            if output.status.success() {
                println!("cargo:warning=Found cached CMake at: {}", cmake_bin.display());
                return Some(cmake_bin);
            }
        }
    }
    None
}

fn cached_cmake_binary(cache_dir: &Path) -> PathBuf {
    let (platform_tag, _) = platform_info();
    let dir_name = format!("cmake-{CMAKE_VERSION}-{platform_tag}");

    if cfg!(target_os = "macos") {
        // macOS archive contains CMake.app/Contents/bin/cmake
        cache_dir
            .join(&dir_name)
            .join("CMake.app")
            .join("Contents")
            .join("bin")
            .join("cmake")
    } else {
        let bin_name = if cfg!(windows) { "cmake.exe" } else { "cmake" };
        cache_dir.join(&dir_name).join("bin").join(bin_name)
    }
}

// ---------------------------------------------------------------------------
// Download + extract
// ---------------------------------------------------------------------------

fn download_and_extract_cmake(cache_dir: &Path) -> Result<PathBuf, String> {
    let (platform_tag, ext) = platform_info();
    let archive_name = format!("cmake-{CMAKE_VERSION}-{platform_tag}.{ext}");
    let url = format!("{CMAKE_URL_BASE}/v{CMAKE_VERSION}/{archive_name}");

    println!("cargo:warning=Downloading {url} ...");

    // Download to a temp file in the cache dir
    fs::create_dir_all(cache_dir).map_err(|e| format!("Failed to create {}: {e}", cache_dir.display()))?;
    let archive_path = cache_dir.join(&archive_name);

    download_file(&url, &archive_path)?;

    println!("cargo:warning=Extracting {archive_name} ...");

    // Extract
    if ext == "zip" {
        extract_zip(&archive_path, cache_dir)?;
    } else {
        extract_tar_gz(&archive_path, cache_dir)?;
    }

    // Clean up archive
    let _ = fs::remove_file(&archive_path);

    // Return path to cmake binary
    let cmake_bin = cached_cmake_binary(cache_dir);
    if cmake_bin.exists() {
        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&cmake_bin, fs::Permissions::from_mode(0o755));
        }
        Ok(cmake_bin)
    } else {
        Err(format!("Extracted archive but cmake binary not found at {}", cmake_bin.display()))
    }
}

fn platform_info() -> (&'static str, &'static str) {
    match (env::consts::OS, env::consts::ARCH) {
        ("windows", "x86_64") => ("windows-x86_64", "zip"),
        ("windows", "aarch64") => ("windows-arm64", "zip"),
        ("linux", "x86_64") => ("linux-x86_64", "tar.gz"),
        ("linux", "aarch64") => ("linux-aarch64", "tar.gz"),
        ("macos", _) => ("macos-universal", "tar.gz"),
        (os, arch) => {
            panic!("Unsupported platform for CMake auto-download: {os}/{arch}. Install CMake manually.");
        }
    }
}

fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if resp.status() != 200 {
        return Err(format!("HTTP {} for {url}", resp.status()));
    }

    let mut reader = resp.into_reader();
    let mut file = fs::File::create(dest)
        .map_err(|e| format!("Failed to create {}: {e}", dest.display()))?;

    io::copy(&mut reader, &mut file)
        .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;

    Ok(())
}

#[cfg(windows)]
fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive)
        .map_err(|e| format!("Failed to open {}: {e}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read zip: {e}"))?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)
            .map_err(|e| format!("Failed to read zip entry {i}: {e}"))?;

        let out_path = dest.join(entry.mangled_name());

        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|e| format!("mkdir failed: {e}"))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir failed: {e}"))?;
            }
            let mut outfile = fs::File::create(&out_path)
                .map_err(|e| format!("Failed to create {}: {e}", out_path.display()))?;
            io::copy(&mut entry, &mut outfile)
                .map_err(|e| format!("Failed to extract {}: {e}", out_path.display()))?;
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn extract_zip(_archive: &Path, _dest: &Path) -> Result<(), String> {
    Err("zip extraction not expected on this platform".to_string())
}

#[cfg(not(windows))]
fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive)
        .map_err(|e| format!("Failed to open {}: {e}", archive.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);

    tar.unpack(dest)
        .map_err(|e| format!("Failed to extract tar.gz: {e}"))?;

    Ok(())
}

#[cfg(windows)]
fn extract_tar_gz(_archive: &Path, _dest: &Path) -> Result<(), String> {
    Err("tar.gz extraction not expected on this platform".to_string())
}
