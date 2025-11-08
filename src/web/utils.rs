use serde_json;

#[allow(dead_code)]
pub fn get_available_tools_json() -> String {
    // Detect OS and provide appropriate command examples
    let os_name = std::env::consts::OS;
    let (description, example_commands) = match os_name {
        "windows" => (
            "Execute shell commands on Windows. Use 'dir' to list files, 'type' to read files, 'cd' to change directory, and other Windows cmd.exe commands.",
            "dir E:\\repo, type file.txt, cd C:\\Users"
        ),
        "linux" => (
            "Execute shell commands on Linux. Use 'ls' to list files, 'cat' to read files, 'cd' to change directory, and other bash commands.",
            "ls /home, cat file.txt, pwd"
        ),
        "macos" => (
            "Execute shell commands on macOS. Use 'ls' to list files, 'cat' to read files, 'cd' to change directory, and other bash commands.",
            "ls /Users, cat file.txt, pwd"
        ),
        _ => (
            "Execute shell commands on the system. Use this to interact with the filesystem, run programs, and check system information.",
            "Use appropriate commands for your operating system"
        )
    };

    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "bash",
                "description": format!("{} OS: {}", description, os_name),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": format!("The shell command to execute. Examples: {}", example_commands)
                        }
                    },
                    "required": ["command"]
                }
            }
        }
    ]).to_string()
}
