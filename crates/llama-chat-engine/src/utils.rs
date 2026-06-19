use std::process::{Command, Stdio};

/// Create a Command that won't open a visible console window on Windows.
/// Always sets stdin to null — prevents IPC pipe inheritance hang when called
/// from a process that has piped stdin (e.g. the worker subprocess).
#[allow(dead_code)]
pub fn silent_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}
