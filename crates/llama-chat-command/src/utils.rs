use std::process::Command;

/// Create a Command that won't open a visible console window on Windows.
/// Use this instead of `Command::new()` for any subprocess spawned from
/// the Tauri desktop app to prevent terminal windows flashing on screen.
pub fn silent_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}
