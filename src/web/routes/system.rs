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
    let (cpu_usage, ram_usage, gpu_usage, cpu_perf_pct) = {
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
    let (cpu_usage, ram_usage, gpu_usage, cpu_perf_pct) = (0.0_f32, 0.0_f32, 0.0_f32, 100.0_f32);

    // Get hardware totals (cached alongside usage)
    #[cfg(target_os = "windows")]
    let (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz) = {
        let last = HARDWARE_TOTALS.lock().unwrap();
        (last.0, last.1, last.2, last.3)
    };
    #[cfg(not(target_os = "windows"))]
    let (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz) = (0.0_f32, 0.0_f32, 0_u32, 0_u32);

    // Current CPU speed = base_mhz * (perf% / 100)
    #[cfg(target_os = "windows")]
    let cpu_ghz = (cpu_base_mhz as f32) * cpu_perf_pct / 100.0 / 1000.0;
    #[cfg(not(target_os = "windows"))]
    let cpu_ghz = 0.0_f32;

    let response = serde_json::json!({
        "cpu": cpu_usage,
        "gpu": gpu_usage,
        "ram": ram_usage,
        "total_ram_gb": total_ram_gb,
        "total_vram_gb": total_vram_gb,
        "cpu_cores": cpu_cores,
        "cpu_ghz": cpu_ghz,
    });

    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&response).unwrap(),
    ))
}

#[cfg(target_os = "windows")]
lazy_static::lazy_static! {
    /// Cached usage: (timestamp, cpu%, ram%, gpu%, cpu_perf_pct)
    static ref LAST_USAGE: Mutex<(Instant, f32, f32, f32, f32)> =
        Mutex::new((Instant::now(), 0.0, 0.0, 0.0, 100.0));
    /// Cached hardware totals: (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz)
    static ref HARDWARE_TOTALS: Mutex<(f32, f32, u32, u32)> = Mutex::new((0.0, 0.0, 0, 0));
}

#[cfg(target_os = "windows")]
pub fn get_cached_windows_system_usage() -> (f32, f32, f32, f32) {
    let last = LAST_USAGE.lock().unwrap();
    (last.1, last.2, last.3, last.4)
}

#[cfg(target_os = "windows")]
pub fn get_windows_system_usage() -> (f32, f32, f32, f32) {
    // Cache for 500ms to allow smooth real-time updates
    let mut last = LAST_USAGE.lock().unwrap();
    if last.0.elapsed() < Duration::from_millis(500) {
        return (last.1, last.2, last.3, last.4);
    }

    // Get CPU usage + performance percentage via PowerShell (single call)
    let cpu_output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-Counter @('\\Processor(_Total)\\% Processor Time','\\Processor Information(_Total)\\% Processor Performance')).CounterSamples | ForEach-Object { $_.CookedValue }"
        ])
        .output();

    let (cpu_usage, cpu_perf_pct) = if let Ok(output) = cpu_output {
        if !output.status.success() {
            sys_debug!(
                "[SYSTEM USAGE] CPU command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let lines: Vec<f32> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|l| l.trim().parse::<f32>().ok())
            .collect();
        (lines.first().copied().unwrap_or(0.0), lines.get(1).copied().unwrap_or(100.0))
    } else {
        (0.0, 100.0)
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
            // CPU logical processors + base clock
            if let Ok(output) = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command",
                    "Get-CimInstance Win32_Processor | ForEach-Object { $_.NumberOfLogicalProcessors; $_.MaxClockSpeed }"])
                .output()
            {
                let lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                if let Some(cores) = lines.first().and_then(|s| s.parse::<u32>().ok()) {
                    hw.2 = cores;
                }
                if let Some(mhz) = lines.get(1).and_then(|s| s.parse::<u32>().ok()) {
                    hw.3 = mhz;
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
    *last = (Instant::now(), cpu_usage, ram_usage, gpu_usage, cpu_perf_pct);

    (cpu_usage, ram_usage, gpu_usage, cpu_perf_pct)
}

#[cfg(not(target_os = "windows"))]
pub fn get_windows_system_usage() -> (f32, f32, f32, f32) {
    // Return placeholder values on non-Windows platforms
    (0.0, 0.0, 0.0, 100.0)
}
