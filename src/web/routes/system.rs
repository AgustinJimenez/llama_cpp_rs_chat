// System monitoring route handlers

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;

use crate::web::response_helpers::json_raw;
use crate::{sys_debug, sys_warn};

#[cfg(target_os = "windows")]
use std::process::Command;
#[cfg(target_os = "windows")]
use std::sync::Mutex;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};
#[cfg(target_os = "windows")]
use tokio::task::spawn_blocking;
#[cfg(target_os = "windows")]
use tokio::time::timeout;

pub async fn handle_system_usage() -> Result<Response<Body>, Infallible> {
    // Get system usage using Windows-native commands
    #[cfg(target_os = "windows")]
    let (cpu_usage, ram_usage, gpu_usage) = {
        let started = Instant::now();
        let result = timeout(
            Duration::from_millis(1500),
            spawn_blocking(get_windows_system_usage),
        )
        .await;

        match result {
            Ok(Ok(values)) => values,
            Ok(Err(join_err)) => {
                sys_warn!("[SYSTEM USAGE] spawn_blocking failed: {}", join_err);
                get_cached_windows_system_usage()
            }
            Err(_) => {
                sys_warn!(
                    "[SYSTEM USAGE] Timed out after {:?}, returning cached values",
                    started.elapsed()
                );
                get_cached_windows_system_usage()
            }
        }
    };

    #[cfg(not(target_os = "windows"))]
    let (cpu_usage, ram_usage, gpu_usage) = get_windows_system_usage();

    // Get hardware totals (cached alongside usage)
    #[cfg(target_os = "windows")]
    let (total_ram_gb, total_vram_gb) = {
        let last = HARDWARE_TOTALS.lock().unwrap();
        (last.0, last.1)
    };
    #[cfg(not(target_os = "windows"))]
    let (total_ram_gb, total_vram_gb) = (0.0_f32, 0.0_f32);

    let response = serde_json::json!({
        "cpu": cpu_usage,
        "gpu": gpu_usage,
        "ram": ram_usage,
        "total_ram_gb": total_ram_gb,
        "total_vram_gb": total_vram_gb,
    });

    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&response).unwrap(),
    ))
}

#[cfg(target_os = "windows")]
lazy_static::lazy_static! {
    static ref LAST_USAGE: Mutex<(Instant, f32, f32, f32)> =
        Mutex::new((Instant::now(), 0.0, 0.0, 0.0));
    /// Cached hardware totals: (total_ram_gb, total_vram_gb)
    static ref HARDWARE_TOTALS: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));
}

#[cfg(target_os = "windows")]
pub fn get_cached_windows_system_usage() -> (f32, f32, f32) {
    let last = LAST_USAGE.lock().unwrap();
    (last.1, last.2, last.3)
}

#[cfg(target_os = "windows")]
pub fn get_windows_system_usage() -> (f32, f32, f32) {
    // Cache for 500ms to allow smooth real-time updates
    let mut last = LAST_USAGE.lock().unwrap();
    if last.0.elapsed() < Duration::from_millis(500) {
        return (last.1, last.2, last.3);
    }

    // Get CPU usage via PowerShell
    let cpu_output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Counter '\\Processor(_Total)\\% Processor Time' | Select-Object -ExpandProperty CounterSamples | Select-Object -ExpandProperty CookedValue"
        ])
        .output();

    let cpu_usage = if let Ok(output) = cpu_output {
        if !output.status.success() {
            sys_debug!(
                "[SYSTEM USAGE] CPU command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0)
    } else {
        0.0
    };

    // Get RAM usage via PowerShell (using WMI)
    let ram_output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "gwmi Win32_OperatingSystem | % { [math]::Round((($_.TotalVisibleMemorySize - $_.FreePhysicalMemory) / $_.TotalVisibleMemorySize) * 100, 2) }"
        ])
        .output();

    let ram_usage = if let Ok(output) = ram_output {
        if !output.status.success() {
            sys_debug!(
                "[SYSTEM USAGE] RAM command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0)
    } else {
        0.0
    };

    // Get GPU usage via nvidia-smi (if available)
    let gpu_output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output();

    let gpu_usage = if let Ok(output) = gpu_output {
        if !output.status.success() {
            sys_debug!(
                "[SYSTEM USAGE] GPU command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .and_then(|line| line.trim().parse::<f32>().ok())
            .unwrap_or(0.0)
    } else {
        0.0
    };

    // Detect hardware totals (only once, when still at defaults)
    {
        let mut hw = HARDWARE_TOTALS.lock().unwrap();
        if hw.0 == 0.0 {
            // Total RAM via WMI (returns KB)
            if let Ok(output) = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command",
                    "gwmi Win32_OperatingSystem | % { $_.TotalVisibleMemorySize }"])
                .output()
            {
                if let Ok(kb) = String::from_utf8_lossy(&output.stdout).trim().parse::<f64>() {
                    hw.0 = (kb / 1_048_576.0) as f32; // KB → GB
                }
            }
            // Total VRAM via nvidia-smi
            if let Ok(output) = Command::new("nvidia-smi")
                .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
                .output()
            {
                if let Some(mb) = String::from_utf8_lossy(&output.stdout)
                    .lines().next()
                    .and_then(|l| l.trim().parse::<f64>().ok())
                {
                    hw.1 = (mb / 1024.0) as f32; // MB → GB
                }
            }
            sys_debug!("[SYSTEM] Detected hardware: {:.1} GB RAM, {:.1} GB VRAM", hw.0, hw.1);
        }
    }

    // Update cache
    *last = (Instant::now(), cpu_usage, ram_usage, gpu_usage);

    (cpu_usage, ram_usage, gpu_usage)
}

#[cfg(not(target_os = "windows"))]
pub fn get_windows_system_usage() -> (f32, f32, f32) {
    // Return placeholder values on non-Windows platforms
    (0.0, 0.0, 0.0)
}
