//! System metrics tool: CPU, memory, disk usage snapshot.

use crate::NativeToolResult;

/// Return system resource usage snapshot (CPU, memory, disk). No required params.
pub fn tool_get_system_metrics(_args: &serde_json::Value) -> NativeToolResult {
    use sysinfo::System;

    let mut sys = System::new();
    sys.refresh_cpu_all();
    std::thread::sleep(std::time::Duration::from_millis(200));
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_usage: f32 = if sys.cpus().is_empty() {
        0.0
    } else {
        sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / sys.cpus().len() as f32
    };

    let total_mem = sys.total_memory();
    let used_mem = sys.used_memory();
    let total_gb = total_mem as f64 / (1024.0 * 1024.0 * 1024.0);
    let used_gb = used_mem as f64 / (1024.0 * 1024.0 * 1024.0);
    let mem_pct = if total_mem > 0 {
        (used_mem as f64 / total_mem as f64) * 100.0
    } else {
        0.0
    };

    let disk_info = get_disk_info();

    let mut result = format!(
        "CPU: {:.0}%\nMemory: {:.1} / {:.1} GB ({:.0}%)",
        cpu_usage, used_gb, total_gb, mem_pct
    );
    if let Some(disk) = disk_info {
        result.push_str(&format!("\nDisk: {}", disk));
    }

    NativeToolResult::text_only(result)
}

/// Get disk usage for the system drive.
fn get_disk_info() -> Option<String> {
    use sysinfo::Disks;

    let disks = Disks::new_with_refreshed_list();

    #[cfg(windows)]
    let system_mount = "C:\\";
    #[cfg(not(windows))]
    let system_mount = "/";

    for disk in disks.list() {
        let mount = disk.mount_point().to_string_lossy();
        if mount == system_mount || mount.starts_with(system_mount) {
            let total = disk.total_space();
            let available = disk.available_space();
            if total > 0 {
                let total_gb = total as f64 / (1024.0 * 1024.0 * 1024.0);
                let avail_gb = available as f64 / (1024.0 * 1024.0 * 1024.0);
                let used_pct = ((total - available) as f64 / total as f64) * 100.0;
                return Some(format!(
                    "{:.0} / {:.0} GB free ({:.0}% used)",
                    avail_gb, total_gb, used_pct
                ));
            }
        }
    }

    if let Some(disk) = disks.list().first() {
        let total = disk.total_space();
        let available = disk.available_space();
        if total > 0 {
            let total_gb = total as f64 / (1024.0 * 1024.0 * 1024.0);
            let avail_gb = available as f64 / (1024.0 * 1024.0 * 1024.0);
            let used_pct = ((total - available) as f64 / total as f64) * 100.0;
            return Some(format!(
                "{:.0} / {:.0} GB free ({:.0}% used)",
                avail_gb, total_gb, used_pct
            ));
        }
    }

    None
}
