use std::process::Command;
use std::env;

fn main() {
    // Always try to set up CMAKE path if available, even before checking features
    setup_cmake_environment();
    
    // Check if we're building with real LLaMA (which requires CMake)
    let real_llama = env::var("CARGO_FEATURE_DOCKER").is_ok() && !env::var("CARGO_FEATURE_MOCK").is_ok();
    
    if real_llama {
        // Check if CMake is available
        if !is_cmake_available() {
            print_cmake_installation_help();
            eprintln!("
FOR E2E TESTING ONLY:
To run tests with mock implementation:
1. Run: cargo test --features mock --no-default-features
");
            panic!("CMake is required but not found. Please install CMake and ensure it's in your PATH.");
        } else {
            println!("cargo:warning=CMake found, proceeding with LLaMA compilation...");
        }
    }
    
    tauri_build::build()
}

fn setup_cmake_environment() {
    // Try to find and set CMAKE environment variable early
    let cmake_paths = [
        "cmake",
        "C:\\Program Files\\CMake\\bin\\cmake.exe",
    ];

    for path in &cmake_paths {
        if let Ok(output) = Command::new(path).arg("--version").output() {
            if output.status.success() {
                println!("cargo:rustc-env=CMAKE={}", path);
                println!("cargo:warning=Setting CMAKE environment variable to: {}", path);
                env::set_var("CMAKE", path);

                // Also add the bin directory to PATH if it's not "cmake" (system-wide)
                if path != &"cmake" {
                    if let Some(parent) = std::path::Path::new(path).parent() {
                        if let Some(parent_str) = parent.to_str() {
                            let current_path = env::var("PATH").unwrap_or_default();
                            if !current_path.contains(parent_str) {
                                let new_path = format!("{};{}", current_path, parent_str);
                                env::set_var("PATH", new_path);
                                println!("cargo:warning=Added {} to PATH", parent_str);
                            }
                        }
                    }
                }
                return;
            }
        }
    }
}

fn is_cmake_available() -> bool {
    // Try common CMake locations on Windows
    let cmake_paths = [
        "cmake",
        "C:\\Program Files\\CMake\\bin\\cmake.exe",
        "C:\\Program Files (x86)\\CMake\\bin\\cmake.exe",
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe",
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe",
        "E:\\repo\\llama_cpp_rs_test\\cmake\\windows\\bin\\cmake.exe",
    ];

    for path in &cmake_paths {
        if let Ok(output) = Command::new(path).arg("--version").output() {
            if output.status.success() {
                println!("cargo:warning=Found CMake at: {}", path);
                // Set CMAKE environment variable for the build
                println!("cargo:rustc-env=CMAKE={}", path);
                return true;
            }
        }
    }
    false
}

fn print_cmake_installation_help() {
    eprintln!("
╔══════════════════════════════════════════════════════════════════════════════╗
║                                CMAKE REQUIRED                                ║
╠══════════════════════════════════════════════════════════════════════════════╣
║ This project requires CMake to build the LLaMA C++ library.                 ║
║                                                                              ║
║ WINDOWS INSTALLATION OPTIONS:                                               ║
║                                                                              ║
║ Option 1 - Official Installer (Recommended):                               ║
║   1. Visit: https://cmake.org/download/                                     ║
║   2. Download 'Windows x64 Installer'                                       ║
║   3. Run installer and CHECK 'Add CMake to system PATH'                     ║
║   4. Restart your terminal/IDE                                              ║
║                                                                              ║
║ Option 2 - Package Managers:                                                ║
║   • Chocolatey: choco install cmake                                         ║
║   • Scoop: scoop install cmake                                              ║
║   • winget: winget install Kitware.CMake                                    ║
║                                                                              ║
║ Option 3 - Quick Fix (Already have CMake installed?):                       ║
║   Run the setup script in this project:                                     ║
║   • PowerShell (as Admin): .\\setup_cmake_path.ps1                          ║
║   • Or Batch (as Admin): setup_cmake_path.bat                               ║
║                                                                              ║
║ Note: Mock implementation is only for E2E testing:                          ║
║   cargo test --features mock --no-default-features                          ║
║                                                                              ║
║ After installation, verify with: cmake --version                            ║
╚══════════════════════════════════════════════════════════════════════════════╝
");

    // On Windows, try to show a message box as well
    #[cfg(target_os = "windows")]
    {
        show_windows_message_box();
    }
}

#[cfg(target_os = "windows")]
fn show_windows_message_box() {
    // Try to show a Windows message box for better visibility
    let _ = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(r#"
            Add-Type -AssemblyName System.Windows.Forms
            $result = [System.Windows.Forms.MessageBox]::Show(
                "CMake is required to build this project but was not found in your PATH.`n`n" +
                "Solutions:`n" +
                "1. Run 'setup_cmake_path.bat' as Administrator (if CMake is installed)`n" +
                "2. Install CMake from https://cmake.org/download/`n" +
                "3. Make sure to check 'Add CMake to system PATH' during installation`n`n" +
                "See the terminal for more detailed instructions.",
                "CMake Not Found - LLaMA Chat Build",
                [System.Windows.Forms.MessageBoxButtons]::OK,
                [System.Windows.Forms.MessageBoxIcon]::Warning
            )
        "#)
        .output();
}