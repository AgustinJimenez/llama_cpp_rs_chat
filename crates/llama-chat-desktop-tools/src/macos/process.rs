//! macOS process management helpers (via sysinfo).

use super::{DWORD, HANDLE};

pub const INVALID_HANDLE_VALUE_PROCESS: HANDLE = -1; // re-exported for clarity

pub fn enumerate_processes() -> Result<Vec<(DWORD, String)>, String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut result = Vec::new();
    for (pid, proc_) in sys.processes() {
        result.push((pid.as_u32(), proc_.name().to_string_lossy().to_string()));
    }
    Ok(result)
}

pub fn terminate_process(pid: DWORD) -> Result<(), String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let spid = sysinfo::Pid::from_u32(pid);
    if let Some(proc_) = sys.process(spid) {
        if proc_.kill() {
            Ok(())
        } else {
            Err(format!("Failed to kill PID {pid}"))
        }
    } else {
        Err(format!("Process {pid} not found"))
    }
}

pub fn get_process_resource_info(pid: DWORD) -> Result<(usize, u64, u64), String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let spid = sysinfo::Pid::from_u32(pid);
    if let Some(proc_) = sys.process(spid) {
        let mem = proc_.memory() as usize;
        let cpu_time = proc_.run_time() * 1000; // seconds to ms
        Ok((mem, 0, cpu_time))
    } else {
        Err(format!("Process {pid} not found"))
    }
}

/// Check if a process with the given PID is still alive.
pub fn is_process_alive(pid: DWORD) -> bool {
    if let Ok(procs) = enumerate_processes() {
        procs.iter().any(|(p, _)| *p == pid)
    } else {
        false
    }
}
