//! Clipboard read/write helpers (text, files, format detection).

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use super::types::*;

pub fn read_clipboard() -> Result<String, String> {
    unsafe {
        if OpenClipboard(0) == 0 {
            return Err("Failed to open clipboard".to_string());
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle == 0 {
            CloseClipboard();
            return Err("No text data in clipboard".to_string());
        }
        let ptr = GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            CloseClipboard();
            return Err("Failed to lock clipboard data".to_string());
        }
        // Find null terminator
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = OsString::from_wide(std::slice::from_raw_parts(ptr, len))
            .to_string_lossy()
            .into_owned();
        GlobalUnlock(handle);
        CloseClipboard();
        Ok(text)
    }
}

pub fn write_clipboard(text: &str) -> Result<(), String> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * 2;
    unsafe {
        if OpenClipboard(0) == 0 {
            return Err("Failed to open clipboard".to_string());
        }
        EmptyClipboard();
        let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if hmem == 0 {
            CloseClipboard();
            return Err("Failed to allocate clipboard memory".to_string());
        }
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            CloseClipboard();
            return Err("Failed to lock clipboard memory".to_string());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr, byte_len);
        GlobalUnlock(hmem);
        if SetClipboardData(CF_UNICODETEXT, hmem) == 0 {
            CloseClipboard();
            return Err("Failed to set clipboard data".to_string());
        }
        CloseClipboard();
        Ok(())
    }
}

/// Read file paths from clipboard (CF_HDROP format, e.g., from Windows Explorer copy).
pub fn read_clipboard_files() -> Result<Vec<String>, String> {
    unsafe {
        if OpenClipboard(0) == 0 {
            return Err("Failed to open clipboard".to_string());
        }
        let handle = GetClipboardData(CF_HDROP);
        if handle == 0 {
            CloseClipboard();
            return Err("No file drop data in clipboard".to_string());
        }
        // DragQueryFileW with index 0xFFFFFFFF returns the file count
        let count = DragQueryFileW(handle, 0xFFFFFFFF, std::ptr::null_mut(), 0);
        let mut files = Vec::with_capacity(count as usize);
        for i in 0..count {
            // Get required buffer size
            let len = DragQueryFileW(handle, i, std::ptr::null_mut(), 0);
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            DragQueryFileW(handle, i, buf.as_mut_ptr(), buf.len() as u32);
            let path = OsString::from_wide(&buf[..len as usize])
                .to_string_lossy()
                .into_owned();
            files.push(path);
        }
        CloseClipboard();
        Ok(files)
    }
}

/// Check which clipboard formats are available.
pub fn get_clipboard_formats() -> Vec<&'static str> {
    let mut formats = Vec::new();
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
            formats.push("text");
        }
        if IsClipboardFormatAvailable(CF_DIB) != 0 {
            formats.push("image");
        }
        if IsClipboardFormatAvailable(CF_HDROP) != 0 {
            formats.push("files");
        }
    }
    formats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_clipboard_files_no_crash() {
        // Just verify it doesn't crash — clipboard may or may not have files
        let _ = read_clipboard_files();
    }
}
