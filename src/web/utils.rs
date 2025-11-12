use serde_json;

#[allow(dead_code)]
pub fn get_available_tools_json() -> String {
    // Detect OS and provide appropriate command examples
    let os_name = std::env::consts::OS;
    let (bash_description, bash_examples, read_examples, write_examples, list_examples) = match os_name {
        "windows" => (
            "Execute any Windows shell command. Use for system operations, running programs, searching, etc.",
            "echo Hello, dir /s *.rs, cd C:\\Users, tasklist",
            "type E:\\repo\\package.json, type C:\\Users\\file.txt",
            "echo Hello World > output.txt, copy source.txt dest.txt",
            "dir E:\\repo, dir /s *.rs, tree /F"
        ),
        "linux" | "macos" => (
            "Execute any Linux/macOS shell command. Use for system operations, running programs, searching, etc.",
            "echo Hello, ls -la, cd /home, ps aux, find . -name '*.rs'",
            "cat /home/user/file.txt, cat package.json",
            "echo 'Hello World' > output.txt, cp source.txt dest.txt",
            "ls -la /home, find . -type f, tree"
        ),
        _ => (
            "Execute shell commands on the system.",
            "Use appropriate commands for your OS",
            "Read files using system commands",
            "Write files using system commands",
            "List files using system commands"
        )
    };

    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": format!("Read the complete contents of any file from anywhere in the local filesystem. You have full read access to the entire system. OS: {}", os_name),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": format!("Absolute or relative path to the file to read. Examples: {}", read_examples)
                        }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": format!("Write or create a file anywhere in the local filesystem. You have full write access. OS: {}", os_name),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path where the file should be written"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write to the file"
                        }
                    },
                    "required": ["path", "content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": format!("List all files and directories in any location. You have full access to browse the entire filesystem. OS: {}", os_name),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": format!("Path to the directory to list. Examples: {}", list_examples)
                        },
                        "recursive": {
                            "type": "boolean",
                            "description": "Whether to list files recursively (default: false)"
                        }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "bash",
                "description": format!("{} You can run ANY command. OS: {}", bash_description, os_name),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": format!("The shell command to execute. Examples: {}", bash_examples)
                        }
                    },
                    "required": ["command"]
                }
            }
        }
    ]).to_string()
}
