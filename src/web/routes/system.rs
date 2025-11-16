// System monitoring route handlers

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;

#[cfg(target_os = "windows")]
use std::process::Command;

pub async fn handle_system_usage() -> Result<Response<Body>, Infallible> {
    // Get system usage using Windows-native commands
    let (cpu_usage, ram_usage, gpu_usage) = get_windows_system_usage();

    let response = serde_json::json!({
        "cpu": cpu_usage,
        "gpu": gpu_usage,
        "ram": ram_usage,
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(Body::from(serde_json::to_string(&response).unwrap()))
        .unwrap())
}

#[cfg(target_os = "windows")]
fn get_windows_system_usage() -> (f32, f32, f32) {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    lazy_static::lazy_static! {
        static ref LAST_USAGE: Mutex<(Instant, f32, f32, f32)> = Mutex::new((Instant::now(), 0.0, 0.0, 0.0));
    }

    // Cache for 500ms to allow smooth real-time updates
    let mut last = LAST_USAGE.lock().unwrap();
    if last.0.elapsed() < Duration::from_millis(500) {
        return (last.1, last.2, last.3);
    }

    // Get CPU usage via PowerShell
    let cpu_output = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Counter '\\Processor(_Total)\\% Processor Time' | Select-Object -ExpandProperty CounterSamples | Select-Object -ExpandProperty CookedValue"
        ])
        .output();

    let cpu_usage = if let Ok(output) = cpu_output {
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0)
    } else {
        0.0
    };

    // Get RAM usage via PowerShell (using WMI)
    let ram_output = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "gwmi Win32_OperatingSystem | % { [math]::Round((($_.TotalVisibleMemorySize - $_.FreePhysicalMemory) / $_.TotalVisibleMemorySize) * 100, 2) }"
        ])
        .output();

    let ram_usage = if let Ok(output) = ram_output {
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0)
    } else {
        0.0
    };

    // Get GPU usage via nvidia-smi (if available)
    let gpu_output = Command::new("nvidia-smi")
        .args(&[
            "--query-gpu=utilization.gpu",
            "--format=csv,noheader,nounits"
        ])
        .output();

    let gpu_usage = if let Ok(output) = gpu_output {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .and_then(|line| line.trim().parse::<f32>().ok())
            .unwrap_or(0.0)
    } else {
        0.0
    };

    // Update cache
    *last = (Instant::now(), cpu_usage, ram_usage, gpu_usage);

    (cpu_usage, ram_usage, gpu_usage)
}

#[cfg(not(target_os = "windows"))]
fn get_windows_system_usage() -> (f32, f32, f32) {
    // Return placeholder values on non-Windows platforms
    (0.0, 0.0, 0.0)
}
