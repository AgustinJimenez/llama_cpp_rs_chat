// System monitoring route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;

use crate::web::database::SharedDatabase;
use crate::web::response_helpers::{json_raw, json_response, json_error};
#[cfg(target_os = "windows")]
use crate::{sys_debug, sys_warn};

#[cfg(target_os = "windows")]
use std::sync::Mutex;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};
#[cfg(target_os = "windows")]
use tokio::task::spawn_blocking;
#[cfg(target_os = "windows")]
use tokio::time::timeout;

/// GET /api/info — app and system information
pub async fn handle_app_info() -> Result<Response<Body>, Infallible> {
    let info = serde_json::json!({
        "app": "llama-chat",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "features": {
            "vision": cfg!(feature = "vision"),
            "cuda": cfg!(feature = "cuda"),
        },
    });
    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&info).unwrap(),
    ))
}

/// GET /api/docs — list all API endpoints
pub async fn handle_api_docs() -> Result<Response<Body>, Infallible> {
    let docs = serde_json::json!({
        "endpoints": [
            {"method": "GET", "path": "/health", "description": "Health check"},
            {"method": "GET", "path": "/api/info", "description": "App and system info"},
            {"method": "GET", "path": "/api/docs", "description": "This endpoint — API documentation"},

            {"method": "POST", "path": "/api/chat", "description": "Send message (local model)", "body": {"message": "string", "conversation_id": "string?"}},
            {"method": "POST", "path": "/api/chat/stream", "description": "Send message with SSE streaming (local model)"},
            {"method": "POST", "path": "/api/chat/cancel", "description": "Cancel current generation"},

            {"method": "GET", "path": "/api/conversations", "description": "List all conversations (optional ?q=term to search by title)"},
            {"method": "POST", "path": "/api/conversations", "description": "Create new conversation", "body": {"title": "string?"}},
            {"method": "GET", "path": "/api/conversation/{id}", "description": "Get conversation messages"},
            {"method": "DELETE", "path": "/api/conversations/{id}", "description": "Delete a conversation"},
            {"method": "PATCH", "path": "/api/conversations/{id}/title", "description": "Rename conversation", "body": {"title": "string"}},
            {"method": "POST", "path": "/api/conversations/{id}/truncate", "description": "Truncate conversation at message"},
            {"method": "GET", "path": "/api/conversations/{id}/events", "description": "Get conversation event log"},
            {"method": "GET", "path": "/api/conversations/{id}/metrics", "description": "Get conversation metrics"},
            {"method": "GET", "path": "/api/conversation/{id}/export", "description": "Export conversation as markdown or JSON (?format=md|json)"},
            {"method": "DELETE", "path": "/api/conversations/batch", "description": "Delete multiple conversations", "body": {"ids": ["string"]}},

            {"method": "GET", "path": "/api/model/status", "description": "Current model status (loaded, generating, etc.)"},
            {"method": "GET", "path": "/api/model/info", "description": "Detailed model info (GGUF metadata)"},
            {"method": "POST", "path": "/api/model/load", "description": "Load a GGUF model", "body": {"model_path": "string"}},
            {"method": "POST", "path": "/api/model/unload", "description": "Unload current model"},
            {"method": "POST", "path": "/api/model/hard-unload", "description": "Force-kill worker to reclaim all VRAM"},
            {"method": "GET", "path": "/api/model/history", "description": "Recently used model paths"},

            {"method": "GET", "path": "/api/providers", "description": "List all providers with availability"},
            {"method": "GET", "path": "/api/providers/{id}/models", "description": "Fetch available models from provider API"},
            {"method": "POST", "path": "/api/providers/{id}/generate", "description": "Generate with cloud provider (blocking)"},
            {"method": "POST", "path": "/api/providers/{id}/stream", "description": "Generate with cloud provider (SSE streaming)"},

            {"method": "GET", "path": "/api/config", "description": "Get sampler/app configuration"},
            {"method": "POST", "path": "/api/config", "description": "Update configuration"},
            {"method": "GET", "path": "/api/config/provider-keys", "description": "Get configured provider API keys (masked)"},
            {"method": "POST", "path": "/api/config/provider-keys", "description": "Set a provider API key", "body": {"provider": "string", "api_key": "string", "base_url": "string?"}},

            {"method": "GET", "path": "/api/tools/available", "description": "List available tools with schemas"},
            {"method": "POST", "path": "/api/tools/execute", "description": "Execute a tool call"},
            {"method": "GET", "path": "/api/tools/web-fetch", "description": "Fetch web page as text"},

            {"method": "GET", "path": "/api/mcp/servers", "description": "List MCP servers"},
            {"method": "POST", "path": "/api/mcp/servers", "description": "Add MCP server"},
            {"method": "DELETE", "path": "/api/mcp/servers/{id}", "description": "Remove MCP server"},
            {"method": "POST", "path": "/api/mcp/servers/{id}/toggle", "description": "Enable/disable MCP server"},
            {"method": "POST", "path": "/api/mcp/refresh", "description": "Refresh MCP connections"},
            {"method": "GET", "path": "/api/mcp/tools", "description": "List discovered MCP tools"},

            {"method": "GET", "path": "/api/system/usage", "description": "CPU/memory/GPU usage"},
            {"method": "GET", "path": "/api/system/processes", "description": "List background processes"},
            {"method": "POST", "path": "/api/system/processes/kill", "description": "Kill a background process"},
            {"method": "POST", "path": "/api/desktop/abort", "description": "Abort current desktop automation"},

            {"method": "GET", "path": "/api/browse", "description": "Browse filesystem for model files"},
            {"method": "POST", "path": "/api/upload", "description": "Upload a model file"},

            {"method": "GET", "path": "/api/hub/search", "description": "Search HuggingFace Hub"},
            {"method": "POST", "path": "/api/hub/download", "description": "Download model from Hub"},
            {"method": "GET", "path": "/api/hub/downloads", "description": "List active downloads"},
        ]
    });
    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string_pretty(&docs).unwrap(),
    ))
}

/// POST /api/desktop/abort — abort current desktop automation
pub async fn handle_desktop_abort() -> Result<Response<Body>, Infallible> {
    crate::web::desktop_tools::set_desktop_abort(true);
    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&serde_json::json!({
            "success": true,
            "message": "Desktop abort signal sent"
        }))
        .unwrap(),
    ))
}

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
    let (cpu_usage, ram_usage, gpu_usage, _cpu_perf_pct) = (0.0_f32, 0.0_f32, 0.0_f32, 100.0_f32);

    // Get hardware totals (cached alongside usage)
    #[cfg(target_os = "windows")]
    let (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz) = {
        let last = HARDWARE_TOTALS.lock().unwrap();
        (last.0, last.1, last.2, last.3)
    };
    #[cfg(target_os = "macos")]
    let (total_ram_gb, total_vram_gb, cpu_cores, _cpu_base_mhz) = {
        use std::process::Command;
        let ram = silent_command("sysctl").args(["-n", "hw.memsize"]).output()
            .ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
            .map(|b| b as f32 / 1_073_741_824.0).unwrap_or(0.0);
        let cores = silent_command("sysctl").args(["-n", "hw.ncpu"]).output()
            .ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
            .unwrap_or(0);
        // macOS unified memory — GPU shares RAM
        (ram, ram, cores, 0_u32)
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let (total_ram_gb, total_vram_gb, cpu_cores, _cpu_base_mhz) = (0.0_f32, 0.0_f32, 0_u32, 0_u32);

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

use crate::web::utils::silent_command;

#[cfg(target_os = "windows")]
pub fn get_windows_system_usage() -> (f32, f32, f32, f32) {
    // Cache for 500ms to allow smooth real-time updates
    let mut last = LAST_USAGE.lock().unwrap();
    if last.0.elapsed() < Duration::from_millis(500) {
        return (last.1, last.2, last.3, last.4);
    }

    // Get CPU usage + performance percentage via PowerShell (single call)
    let cpu_output = silent_command("powershell")
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
    let ram_output = silent_command("powershell")
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
    let gpu_output = silent_command("nvidia-smi")
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
            if let Ok(output) = silent_command("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command",
                    "gwmi Win32_OperatingSystem | % { $_.TotalVisibleMemorySize }"])
                .output()
            {
                if let Ok(kb) = String::from_utf8_lossy(&output.stdout).trim().parse::<f64>() {
                    hw.0 = (kb / 1_048_576.0) as f32; // KB → GB
                }
            }
            // CPU logical processors + base clock
            if let Ok(output) = silent_command("powershell")
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
            if let Ok(output) = silent_command("nvidia-smi")
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
#[allow(dead_code)]
pub fn get_windows_system_usage() -> (f32, f32, f32, f32) {
    // Return placeholder values on non-Windows platforms
    (0.0, 0.0, 0.0, 100.0)
}

// ── Background process endpoints ────────────────────────────────────────────

pub async fn handle_background_processes(
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.connection();

        // Query all background processes from DB
        let mut stmt = match conn.prepare(
            "SELECT pid, command, conversation_id, started_at, session_id FROM background_processes"
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows: Vec<(i64, String, Option<String>, i64, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .ok()
            .map(|iter| iter.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();

        let mut processes = Vec::new();
        let mut dead_pids = Vec::new();

        for (pid, command, conversation_id, started_at, _session_id) in rows {
            let alive = crate::web::background::is_process_alive(pid as u32);
            if !alive {
                dead_pids.push(pid);
            }
            processes.push(serde_json::json!({
                "pid": pid,
                "command": command,
                "conversationId": conversation_id,
                "startedAt": started_at,
                "alive": alive,
            }));
        }

        // Clean up dead records
        for pid in dead_pids {
            let _ = conn.execute("DELETE FROM background_processes WHERE pid = ?1", [pid]);
        }

        // Only return alive processes
        processes.retain(|p| p["alive"].as_bool().unwrap_or(false));
        processes
    })
    .await
    .unwrap_or_default();

    Ok(json_response(StatusCode::OK, &result))
}

pub async fn handle_kill_process(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let body = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };

    let parsed: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid JSON")),
    };

    let pid = match parsed["pid"].as_u64() {
        Some(p) => p as u32,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Missing pid")),
    };

    let result = tokio::task::spawn_blocking(move || {
        // Kill the process tree
        crate::web::background::kill_background_process_by_pid(pid);

        // Wait briefly for process to die, then verify
        std::thread::sleep(std::time::Duration::from_millis(500));
        let still_alive = crate::web::background::is_process_alive(pid);

        // Remove from DB regardless (stale entries get cleaned on next list)
        let conn = db.connection();
        let _ = conn.execute("DELETE FROM background_processes WHERE pid = ?1", [pid as i64]);

        !still_alive
    })
    .await
    .unwrap_or(false);

    if result {
        Ok(json_response(StatusCode::OK, &serde_json::json!({"success": true, "message": "Process killed"})))
    } else {
        Ok(json_response(StatusCode::OK, &serde_json::json!({"success": false, "message": "Process may not have been killed. It might require elevated permissions."})))
    }
}
