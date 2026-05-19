//! Windows registry read helpers.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use super::types::*;

/// Read a string or DWORD value from the Windows registry.
pub fn read_registry_value(hkey_root: HKEY, subkey: &str, value_name: &str) -> Result<String, String> {
    unsafe {
        let subkey_w = to_wide(subkey);
        let mut hkey: HKEY = 0;
        let status = RegOpenKeyExW(hkey_root, subkey_w.as_ptr(), 0, KEY_READ, &mut hkey);
        if status != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW failed (error {status})"));
        }

        let value_w = to_wide(value_name);
        let mut reg_type: DWORD = 0;
        let mut data_size: DWORD = 0;

        // Query size first
        let status = RegQueryValueExW(
            hkey, value_w.as_ptr(), std::ptr::null_mut(),
            &mut reg_type, std::ptr::null_mut(), &mut data_size,
        );
        if status != ERROR_SUCCESS {
            RegCloseKey(hkey);
            return Err(format!("RegQueryValueExW failed (error {status})"));
        }

        let mut data = vec![0u8; data_size as usize];
        let status = RegQueryValueExW(
            hkey, value_w.as_ptr(), std::ptr::null_mut(),
            &mut reg_type, data.as_mut_ptr(), &mut data_size,
        );
        RegCloseKey(hkey);

        if status != ERROR_SUCCESS {
            return Err(format!("RegQueryValueExW read failed (error {status})"));
        }

        match reg_type {
            REG_SZ => {
                let wide: &[u16] = std::slice::from_raw_parts(
                    data.as_ptr() as *const u16,
                    data_size as usize / 2,
                );
                // Trim trailing null
                let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
                Ok(OsString::from_wide(&wide[..len]).to_string_lossy().into_owned())
            }
            REG_DWORD => {
                if data_size >= 4 {
                    let val = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                    Ok(val.to_string())
                } else {
                    Err("REG_DWORD data too small".to_string())
                }
            }
            other => Err(format!("Unsupported registry type: {other}")),
        }
    }
}
