//! Process enumeration, termination, and resource info.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use super::types::*;

/// Enumerate all running processes. Returns Vec of (pid, exe_name).
pub fn enumerate_processes() -> Result<Vec<(DWORD, String)>, String> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("CreateToolhelp32Snapshot failed".to_string());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dw_size = std::mem::size_of::<PROCESSENTRY32W>() as DWORD;

        let mut result = Vec::new();
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let name_len = entry.sz_exe_file.iter().position(|&c| c == 0).unwrap_or(260);
                let name = OsString::from_wide(&entry.sz_exe_file[..name_len])
                    .to_string_lossy()
                    .into_owned();
                result.push((entry.th32_process_id, name));
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        Ok(result)
    }
}

/// Terminate a process by PID. Refuses system-critical processes.
pub fn terminate_process(pid: DWORD) -> Result<(), String> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle == 0 {
            return Err(format!("OpenProcess failed for PID {pid} (access denied or not found)"));
        }
        let ok = TerminateProcess(handle, 1);
        CloseHandle(handle);
        if ok == 0 {
            return Err(format!("TerminateProcess failed for PID {pid}"));
        }
        Ok(())
    }
}

/// Get process resource info: working set (memory) and CPU times.
/// Returns (working_set_bytes, kernel_time_ms, user_time_ms).
pub fn get_process_resource_info(pid: DWORD) -> Result<(usize, u64, u64), String> {
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            0,
            pid,
        );
        if handle == 0 {
            return Err(format!("OpenProcess failed for PID {pid}"));
        }

        // Memory info
        let mut mem: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        mem.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as DWORD;
        let mem_ok = GetProcessMemoryInfo(handle, &mut mem, mem.cb);
        let working_set = if mem_ok != 0 {
            mem.working_set_size
        } else {
            0
        };

        // CPU times
        let mut creation: FILETIME = std::mem::zeroed();
        let mut exit: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        let time_ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);

        let (kernel_ms, user_ms) = if time_ok != 0 {
            let k = ((kernel.dw_high_date_time as u64) << 32 | kernel.dw_low_date_time as u64)
                / 10_000; // 100-ns units to ms
            let u = ((user.dw_high_date_time as u64) << 32 | user.dw_low_date_time as u64)
                / 10_000;
            (k, u)
        } else {
            (0, 0)
        };

        Ok((working_set, kernel_ms, user_ms))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_process_resource_info_self() {
        let pid = std::process::id();
        let result = get_process_resource_info(pid);
        assert!(result.is_ok(), "Should get info for own process: {:?}", result);
        let (mem, _kernel, _user) = result.unwrap();
        assert!(mem > 0, "Own process should use some memory");
    }
}
