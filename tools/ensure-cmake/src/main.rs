//! Ensures CMake is available, then runs the given command.
//!
//! llama-cpp-sys-2's build script needs cmake and runs BEFORE our build.rs
//! (dependencies build first). This tool ensures cmake is on PATH before
//! invoking cargo, solving the chicken-and-egg problem.
//!
//! Usage:
//!   cargo run --manifest-path tools/ensure-cmake/Cargo.toml -- cargo build --features cuda
//!
//! Resolution order:
//!   1. cmake already on PATH
//!   2. Previously downloaded copy in target/cmake/
//!   3. Download portable cmake from GitHub releases

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Pinned CMake version — same as build.rs.
const CMAKE_VERSION: &str = "3.31.6";
const CMAKE_URL_BASE: &str = "https://github.com/Kitware/CMake/releases/download";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        // No command — just ensure cmake and print path
        match ensure_cmake() {
            Ok(Some(bin_dir)) => {
                println!("{}", bin_dir.display());
                ExitCode::SUCCESS
            }
            Ok(None) => {
                eprintln!("cmake already on PATH");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("ERROR: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        // Ensure cmake, then exec the command with cmake on PATH
        match ensure_cmake() {
            Ok(cmake_bin_dir) => run_command(&args, cmake_bin_dir.as_deref()),
            Err(e) => {
                eprintln!("ERROR: Failed to ensure cmake: {e}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run_command(args: &[String], cmake_bin_dir: Option<&Path>) -> ExitCode {
    let (cmd, cmd_args) = args.split_first().unwrap();

    let mut command = Command::new(cmd);
    command.args(cmd_args);

    // Inject cmake into PATH if we downloaded it
    if let Some(bin_dir) = cmake_bin_dir {
        let current_path = env::var("PATH").unwrap_or_default();
        let sep = if cfg!(windows) { ";" } else { ":" };
        let new_path = format!("{}{sep}{current_path}", bin_dir.display());
        command.env("PATH", &new_path);
        command.env("CMAKE", bin_dir.join(cmake_binary_name()));
    }

    match command.status() {
        Ok(status) => {
            if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(status.code().unwrap_or(1) as u8)
            }
        }
        Err(e) => {
            eprintln!("ERROR: Failed to run '{}': {e}", cmd);
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// cmake resolution
// ---------------------------------------------------------------------------

/// Returns Some(bin_dir) if cmake needs to be added to PATH, None if already on PATH.
fn ensure_cmake() -> Result<Option<PathBuf>, String> {
    // 1. System cmake on PATH
    if let Some(cmake_path) = find_system_cmake() {
        // "cmake" alone means it's already on PATH — no injection needed
        if cmake_path.to_str() == Some("cmake") {
            eprintln!("✅ CMake found on system PATH");
            return Ok(None);
        }
        // Found at an absolute path (e.g. "C:\Program Files\CMake\bin\cmake.exe")
        // Need to inject its parent dir into PATH for child processes
        if let Some(bin_dir) = cmake_path.parent() {
            eprintln!("✅ CMake found at {}, injecting into PATH", cmake_path.display());
            return Ok(Some(bin_dir.to_path_buf()));
        }
    }

    // 2. Cached download
    let cache_dir = cmake_cache_dir();
    if let Some(bin_dir) = find_cached_cmake(&cache_dir) {
        eprintln!("✅ CMake found in cache: {}", bin_dir.display());
        return Ok(Some(bin_dir));
    }

    // 3. Download
    eprintln!("⚠️  CMake not found — downloading portable CMake {CMAKE_VERSION}...");
    let bin_dir = download_and_extract_cmake(&cache_dir)?;
    eprintln!("✅ CMake {CMAKE_VERSION} ready at {}", bin_dir.display());
    Ok(Some(bin_dir))
}

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
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

fn cmake_cache_dir() -> PathBuf {
    // Put cmake in the project's target/cmake/ directory
    // Walk up from the binary location to find the project root
    if let Ok(exe) = env::current_exe() {
        let mut dir = exe.as_path();
        while let Some(parent) = dir.parent() {
            if parent.join("Cargo.toml").exists() && parent.join("tools").exists() {
                return parent.join("target").join("cmake");
            }
            dir = parent;
        }
    }
    // Fallback: use current working directory
    PathBuf::from("target/cmake")
}

fn find_cached_cmake(cache_dir: &Path) -> Option<PathBuf> {
    let bin_dir = cached_cmake_bin_dir(cache_dir);
    let cmake_bin = bin_dir.join(cmake_binary_name());
    if cmake_bin.exists() {
        if let Ok(output) = Command::new(&cmake_bin).arg("--version").output() {
            if output.status.success() {
                return Some(bin_dir);
            }
        }
    }
    None
}

fn cached_cmake_bin_dir(cache_dir: &Path) -> PathBuf {
    let (tag, _) = platform_info();
    let dir_name = format!("cmake-{CMAKE_VERSION}-{tag}");

    if cfg!(target_os = "macos") {
        cache_dir
            .join(&dir_name)
            .join("CMake.app")
            .join("Contents")
            .join("bin")
    } else {
        cache_dir.join(&dir_name).join("bin")
    }
}

fn cmake_binary_name() -> &'static str {
    if cfg!(windows) { "cmake.exe" } else { "cmake" }
}

// ---------------------------------------------------------------------------
// Download + extract
// ---------------------------------------------------------------------------

fn download_and_extract_cmake(cache_dir: &Path) -> Result<PathBuf, String> {
    let (tag, ext) = platform_info();
    let archive_name = format!("cmake-{CMAKE_VERSION}-{tag}.{ext}");
    let url = format!("{CMAKE_URL_BASE}/v{CMAKE_VERSION}/{archive_name}");

    fs::create_dir_all(cache_dir)
        .map_err(|e| format!("Failed to create {}: {e}", cache_dir.display()))?;
    let archive_path = cache_dir.join(&archive_name);

    eprintln!("📥 Downloading {url}");
    download_file(&url, &archive_path)?;

    eprintln!("📦 Extracting {archive_name}");
    if ext == "zip" {
        extract_zip(&archive_path, cache_dir)?;
    } else {
        extract_tar_gz(&archive_path, cache_dir)?;
    }

    let _ = fs::remove_file(&archive_path);

    let bin_dir = cached_cmake_bin_dir(cache_dir);
    let cmake_bin = bin_dir.join(cmake_binary_name());
    if cmake_bin.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&cmake_bin, fs::Permissions::from_mode(0o755));
        }
        Ok(bin_dir)
    } else {
        Err(format!(
            "Extracted archive but cmake not found at {}",
            cmake_bin.display()
        ))
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
            panic!("Unsupported platform: {os}/{arch}. Install CMake manually.");
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
    let mut file =
        fs::File::create(dest).map_err(|e| format!("Failed to create {}: {e}", dest.display()))?;

    io::copy(&mut reader, &mut file)
        .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;

    Ok(())
}

#[cfg(windows)]
fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file =
        fs::File::open(archive).map_err(|e| format!("Failed to open {}: {e}", archive.display()))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed to read zip: {e}"))?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry {i}: {e}"))?;

        let out_path = dest.join(entry.mangled_name());
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| format!("mkdir failed: {e}"))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
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
    let file =
        fs::File::open(archive).map_err(|e| format!("Failed to open {}: {e}", archive.display()))?;
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
