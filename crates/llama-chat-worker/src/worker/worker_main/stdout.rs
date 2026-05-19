use std::io::Write;

use super::super::ipc_types::WorkerResponse;

/// Redirect the C-level stdout file descriptor to stderr so native code cannot
/// pollute the JSON Lines IPC pipe.
#[cfg(windows)]
pub(super) fn steal_stdout_for_ipc() -> std::fs::File {
    use std::os::windows::io::FromRawHandle;

    extern "C" {
        fn _dup(fd: i32) -> i32;
        fn _dup2(src: i32, dst: i32) -> i32;
    }

    unsafe {
        let ipc_fd = _dup(1);
        assert!(ipc_fd >= 0, "Failed to _dup stdout");
        _dup2(2, 1);
        let handle = libc_fd_to_handle(ipc_fd);
        std::fs::File::from_raw_handle(handle as *mut _)
    }
}

#[cfg(windows)]
unsafe fn libc_fd_to_handle(fd: i32) -> usize {
    extern "C" {
        fn _get_osfhandle(fd: i32) -> isize;
    }
    unsafe { _get_osfhandle(fd) as usize }
}

#[cfg(not(windows))]
pub(super) fn steal_stdout_for_ipc() -> std::fs::File {
    use std::os::unix::io::FromRawFd;

    unsafe {
        let ipc_fd = libc::dup(1);
        assert!(ipc_fd >= 0, "Failed to dup stdout");
        libc::dup2(2, 1);
        std::fs::File::from_raw_fd(ipc_fd)
    }
}

pub(super) fn write_response_no_flush(
    writer: &mut impl Write,
    response: &WorkerResponse,
) {
    let line = serde_json::to_string(response)
        .expect("failed to serialize worker response");
    let _ = writer.write_all(line.as_bytes());
    let _ = writer.write_all(b"\n");
}

pub(super) fn write_response(
    writer: &mut impl Write,
    response: &WorkerResponse,
) {
    write_response_no_flush(writer, response);
    let _ = writer.flush();
}
