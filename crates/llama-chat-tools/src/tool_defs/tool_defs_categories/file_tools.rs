//! File system, code execution, and code-intelligence tool definitions.

use super::{p, Params, RawParam, ToolDef};

pub static FILE_TOOLS: &[ToolDef] = &[
    // ─── read_file ───
    ToolDef {
        name: "read_file",
        description: "Read the contents of a file. Supports PDF, DOCX, XLSX, PPTX, EPUB, ODT, RTF, CSV, EML, ZIP, and non-UTF8 encoded files. Returns the file text (truncated at 100KB for large files). Binary files (exe, images, audio, etc.) are rejected — use a specialized tool for those.",
        params: Params::Simple(&[
            p("path", "string", "Path to the file to read"),
            p("offset", "integer", "Line number to start reading from (1-based). Use with limit to read specific portions of large files."),
            p("limit", "integer", "Maximum number of lines to read. Defaults to all lines."),
            p("pages", "string", "Page range for PDF files (e.g. '1-5', '3', '10-20'). Only for PDF files."),
            p("summary", "string", "'false' for raw content, 'true' (default) for automatic summary of large output, or a custom prompt (e.g. 'list all character names', 'summarize chapter 3'). Saves context tokens on large files."),
        ]),
        required: &["path"],
    },
    // ─── write_file ───
    ToolDef {
        name: "write_file",
        description: "Write content to a file. Creates parent directories if needed. Overwrites existing files.",
        params: Params::Simple(&[
            p("path", "string", "Path to write the file to"),
            p("content", "string", "The content to write to the file"),
        ]),
        required: &["path", "content"],
    },
    // ─── edit_file ───
    ToolDef {
        name: "edit_file",
        description: "Replace exact text in a file. old_string must match exactly once in the file. Use this for small edits instead of rewriting the whole file with write_file.",
        params: Params::Simple(&[
            p("path", "string", "Path to the file to edit"),
            p("old_string", "string", "Exact text to find in the file (must appear exactly once)"),
            p("new_string", "string", "Text to replace it with"),
        ]),
        required: &["path", "old_string", "new_string"],
    },
    // ─── multi_edit ───
    ToolDef {
        name: "multi_edit",
        description: "Apply multiple targeted edits across one or more files in a single call. Each edit replaces an exact string that must appear exactly once in its file. Edits are applied in order; if any edit fails the remaining edits are skipped. Use this instead of multiple edit_file calls when refactoring across files.",
        params: Params::Mixed(
            &[],
            &[RawParam {
                name: "edits",
                build: || serde_json::json!({
                    "type": "array",
                    "description": "List of edits to apply, in order.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path":       { "type": "string", "description": "Path to the file to edit" },
                            "old_string": { "type": "string", "description": "Exact text to find (must appear exactly once)" },
                            "new_string": { "type": "string", "description": "Text to replace it with" }
                        },
                        "required": ["path", "old_string", "new_string"]
                    }
                }),
            }],
        ),
        required: &["edits"],
    },
    // ─── undo_edit ───
    ToolDef {
        name: "undo_edit",
        description: "Revert the last edit_file operation on a file. Restores the file from its backup.",
        params: Params::Simple(&[
            p("path", "string", "Path to the file to restore"),
        ]),
        required: &["path"],
    },
    // ─── insert_text ───
    ToolDef {
        name: "insert_text",
        description: "Insert text at a specific line number in a file. Line is 1-based. The text is inserted before the specified line.",
        params: Params::Simple(&[
            p("path", "string", "Path to the file"),
            p("line", "integer", "Line number to insert at (1-based)"),
            p("text", "string", "Text to insert"),
        ]),
        required: &["path", "line", "text"],
    },
    // ─── search_files ───
    ToolDef {
        name: "search_files",
        description: "Search file contents across a directory by regex or literal pattern. Returns matching lines with file paths and line numbers. Use include to filter by file type (e.g. \"*.rs\").",
        params: Params::Simple(&[
            p("pattern", "string", "Regex or literal pattern to search for"),
            p("path", "string", "Directory to search in (default: current directory)"),
            p("include", "string", "Glob filter for file names (e.g. \"*.py\", \"*.rs\")"),
            p("context", "integer", "Number of context lines before/after each match (default: 0)"),
            p("exclude", "string", "Glob pattern to exclude (e.g. \"*_test.rs\", \"*.generated.*\")"),
        ]),
        required: &["pattern"],
    },
    // ─── find_files ───
    ToolDef {
        name: "find_files",
        description: "Find files by name pattern recursively. Returns a list of matching file paths.",
        params: Params::Simple(&[
            p("pattern", "string", "File name pattern (e.g. \"*.tsx\", \"Cargo.*\", \"README*\")"),
            p("path", "string", "Directory to search in (default: current directory)"),
            p("exclude", "string", "Glob pattern to exclude (e.g. \"*.min.js\", \"*_test.*\")"),
        ]),
        required: &["pattern"],
    },
    // ─── execute_python ───
    ToolDef {
        name: "execute_python",
        description: "Execute Python code. The code is written to a temp file and run with the Python interpreter. Supports multi-line code, imports, regex, and any valid Python. Returns stdout and stderr.",
        params: Params::Simple(&[
            p("code", "string", "The Python code to execute"),
        ]),
        required: &["code"],
    },
    // ─── execute_command ───
    ToolDef {
        name: "execute_command",
        description: "Execute a shell command (git, npm, curl, etc.). You MUST set the background flag for every call.",
        params: Params::Simple(&[
            p("command", "string", "The shell command to execute"),
            p("background", "boolean", "REQUIRED. Set true for long-running processes (dev servers, watchers, daemons like 'php artisan serve', 'npm run dev', 'python -m http.server'). Set false for everything else (installs, builds, one-shot commands). If true, returns after 5s with initial output and the PID."),
            p("timeout", "integer", "Optional. Max seconds of inactivity (no output) before the command is killed. Default 120 (2 min). Resets every time the command produces output. Use higher values for commands with long silent phases."),
            p("working_directory", "string", "Optional. Run the command in this directory instead of the default working directory. Equivalent to cd-ing into the directory first."),
        ]),
        required: &["command", "background"],
    },
    // ─── execute_pty ───
    ToolDef {
        name: "execute_pty",
        description: "Execute a shell command inside a real pseudo-terminal (PTY). Programs see a real TTY — this prevents stdout buffering in Python, Node.js, and similar runtimes. Use this instead of execute_command when a program's output is being buffered or swallowed (e.g. 'python server.py' that doesn't flush, interactive installers). Same timeout semantics as execute_command. Does not support background=true (use execute_command for daemons).",
        params: Params::Simple(&[
            p("command", "string", "The shell command to execute inside a PTY"),
            p("timeout", "integer", "Max seconds of inactivity before the command is killed (default 120)"),
        ]),
        required: &["command"],
    },
    // ─── list_directory ───
    ToolDef {
        name: "list_directory",
        description: "List files and directories in a path. Shows name, size, and type for each entry.",
        params: Params::Simple(&[
            p("path", "string", "Directory path to list (defaults to current directory)"),
        ]),
        required: &[],
    },
    // ─── lsp_query ───
    ToolDef {
        name: "lsp_query",
        description: "Query code intelligence: find definitions, references, symbols, diagnostics. Uses ctags (if available) with ripgrep fallback. For more precise results, you can install a real language server (rust-analyzer for Rust, typescript-language-server for TS, pyright for Python, gopls for Go, clangd for C/C++) and use execute_command to query it directly.",
        params: Params::Simple(&[
            p("action", "string", "Action: 'definition' (find where symbol is defined), 'references' (find all usages), 'symbols' (list symbols in file), 'hover' (get type info), 'diagnostics' (run language-specific type checker)"),
            p("symbol", "string", "Symbol name to query (e.g. 'MyStruct', 'my_function')"),
            p("file", "string", "File path for context (where the symbol is used)"),
            p("path", "string", "Project root directory to search in"),
        ]),
        required: &["action", "symbol"],
    },
];
