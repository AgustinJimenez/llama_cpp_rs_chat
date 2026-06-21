//! Command execution, directory listing, git tools, and LSP/ctags helpers.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;

use crate::utils::silent_command;

// ─── ctags cache for lsp_query ─────────────────────────────────────────────────
static CTAGS_CACHE: OnceLock<StdMutex<HashMap<String, (std::time::Instant, String)>>> = OnceLock::new();

fn ctags_cache() -> &'static StdMutex<HashMap<String, (std::time::Instant, String)>> {
    CTAGS_CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Generate or retrieve cached ctags output for a project/file path.
/// Returns None if ctags is not installed or fails.
pub fn get_ctags(path: &str) -> Option<String> {
    let cache_ttl = std::time::Duration::from_secs(300); // 5 min cache

    // Check cache
    if let Ok(cache) = ctags_cache().lock() {
        if let Some((time, data)) = cache.get(path) {
            if time.elapsed() < cache_ttl {
                return Some(data.clone());
            }
        }
    }

    // Try to generate with ctags/universal-ctags (JSON output for easy parsing)
    let output = std::process::Command::new("ctags")
        .args(["--recurse", "--output-format=json", "--fields=+n", "-f", "-"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let result = String::from_utf8_lossy(&output.stdout).to_string();
    if result.trim().is_empty() {
        return None;
    }

    // Cache it
    if let Ok(mut cache) = ctags_cache().lock() {
        cache.insert(path.to_string(), (std::time::Instant::now(), result.clone()));
    }

    Some(result)
}

/// ripgrep-based definition search (fallback when ctags unavailable)
pub fn lsp_ripgrep_definition(symbol: &str, path: &str) -> String {
    let escaped = regex::escape(symbol);
    let patterns = [
        format!(r"(fn|pub fn|pub\(crate\) fn|async fn|pub async fn)\s+{escaped}\s*[\(<]"),
        format!(r"(struct|enum|trait|type|pub struct|pub enum|pub trait|pub type)\s+{escaped}\s*[\{{<]"),
        format!(r"(class|interface|function|const|let|var|def|defn?)\s+{escaped}\s*[\({{:<= ]"),
        format!(r"(impl|impl<[^>]*>)\s+{escaped}\s"),
    ];
    let combined = patterns.join("|");
    let combined_escaped = combined.replace('"', "\\\"");
    let cmd = format!(
        "rg -n -e \"{combined_escaped}\" \"{path}\" --type-add \"code:*.{{rs,ts,tsx,js,jsx,py,go,java,c,cpp,h,hpp,nim,ex}}\" -t code --max-count 20"
    );
    llama_chat_command::execute_command(&cmd)
}

/// ripgrep-based symbol listing (fallback when ctags unavailable)
pub fn lsp_ripgrep_symbols(target: &str) -> String {
    let cmd = format!(
        "rg -n \"(fn |struct |enum |trait |class |interface |function |def |const |type |impl )\" \"{target}\" --max-count 50"
    );
    llama_chat_command::execute_command(&cmd)
}

/// Execute Python code by writing to a temp file and running it.
/// This completely bypasses shell quoting — the code goes directly to a .py file.
pub fn tool_execute_python(args: &Value) -> String {
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'code' argument is required".to_string(),
    };

    // Write code to a temp file
    let temp_dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_file = temp_dir.join(format!("llama_tool_{nanos}.py"));

    if let Err(e) = std::fs::write(&temp_file, code) {
        return format!("Error writing temp file: {e}");
    }

    // Run python on the temp file — no shell involved
    let result = silent_command("python")
        .arg(&temp_file)
        .output();

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_file);

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                format!("{stdout}\nStderr: {stderr}")
            } else if stdout.is_empty() {
                "Python script executed successfully (no output)".to_string()
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error running Python: {e}"),
    }
}

/// List directory contents with name, size, and type.
pub fn tool_list_directory(args: &Value) -> String {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => return format!("Error reading directory '{path}': {e}"),
    };

    let mut lines = Vec::new();
    lines.push(format!("Directory listing: {path}"));
    lines.push(format!("{:<40} {:>10} {}", "Name", "Size", "Type"));
    lines.push("-".repeat(60));

    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = entry.metadata();
        let (size, file_type) = match metadata {
            Ok(m) => {
                let ft = if m.is_dir() {
                    "<DIR>"
                } else if m.is_symlink() {
                    "<LINK>"
                } else {
                    "<FILE>"
                };
                (m.len(), ft)
            }
            Err(_) => (0, "<?>"),
        };
        lines.push(format!("{name:<40} {size:>10} {file_type}"));
    }

    lines.join("\n")
}

/// Show git status of a repository.
pub fn tool_git_status(args: &Value) -> String {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let mut cmd = silent_command("git");
    cmd.arg("status").arg("--short");
    cmd.current_dir(path);
    cmd.stdin(std::process::Stdio::null());
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                let code = output.status.code().unwrap_or(-1);
                let stderr = stderr.trim();
                return format!("Error (exit {code}): {stderr}");
            }
            if stdout.trim().is_empty() {
                "Working tree clean (no changes)".to_string()
            } else {
                format!("Git status:\n{stdout}")
            }
        }
        Err(e) => format!("Error running git: {e}"),
    }
}

/// Show git diff for files.
pub fn tool_git_diff(args: &Value) -> String {
    let path = args.get("path").and_then(|v| v.as_str());
    let staged = args.get("staged").and_then(|v| {
        v.as_bool().or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true")))
    }).unwrap_or(false);

    let mut cmd = silent_command("git");
    cmd.arg("diff");
    if staged {
        cmd.arg("--staged");
    }
    if let Some(p) = path {
        if std::path::Path::new(p).is_dir() {
            cmd.current_dir(p);
        } else {
            cmd.arg("--").arg(p);
        }
    }
    cmd.stdin(std::process::Stdio::null());
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                let code = output.status.code().unwrap_or(-1);
                let stderr = stderr.trim();
                return format!("Error (exit {code}): {stderr}");
            }
            if stdout.trim().is_empty() {
                "No differences found".to_string()
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error running git: {e}"),
    }
}

/// Commit staged changes with a message.
pub fn tool_git_commit(args: &Value) -> String {
    let message = match args.get("message").and_then(|v| v.as_str()) {
        Some(m) if !m.is_empty() => m,
        _ => return "Error: 'message' argument is required".to_string(),
    };
    let all = args.get("all").and_then(|v| {
        v.as_bool().or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true")))
    }).unwrap_or(false);

    let mut cmd = silent_command("git");
    cmd.arg("commit");
    if all {
        cmd.arg("-a");
    }
    cmd.arg("-m").arg(message);
    cmd.stdin(std::process::Stdio::null());
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                let code = output.status.code().unwrap_or(-1);
                let stderr = stderr.trim();
                let stdout = stdout.trim();
                format!("Error (exit {code}): {stderr}\n{stdout}")
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error running git: {e}"),
    }
}

/// Find an executable by name: checks PATH first, then common installation directories.
/// Returns a human-readable result with the resolved path or a list of searched locations.
pub fn tool_find_executable(args: &Value) -> String {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) if !n.is_empty() => n,
        _ => return "Error: 'name' argument is required".to_string(),
    };

    // 1. Try `where` (Windows) / `which` (Unix) — checks PATH
    #[cfg(target_os = "windows")]
    let path_result = silent_command("where").arg(name).stdin(std::process::Stdio::null()).output();
    #[cfg(not(target_os = "windows"))]
    let path_result = silent_command("which").arg(name).stdin(std::process::Stdio::null()).output();

    if let Ok(out) = path_result {
        let found = String::from_utf8_lossy(&out.stdout);
        let found = found.trim();
        if out.status.success() && !found.is_empty() {
            let first = found.lines().next().unwrap_or(found);
            return format!("Found in PATH: {first}");
        }
    }

    // 2. Probe common installation directories
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let local_app = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let app_data = std::env::var("APPDATA").unwrap_or_default();

    #[cfg(target_os = "windows")]
    let candidates: Vec<String> = {
        let exe = format!("{name}.cmd");
        let exe2 = format!("{name}.exe");
        let bare = name.to_string();
        vec![
            format!(r"{home}\{name}\bin\{exe}"),
            format!(r"{home}\{name}\bin\{exe2}"),
            format!(r"{home}\apache-{name}\bin\{exe}"),
            format!(r"{home}\{name}-*\bin\{exe}"),
            format!(r"{local_app}\Programs\{name}\{exe2}"),
            format!(r"{local_app}\Programs\{name}\bin\{exe2}"),
            format!(r"{app_data}\{name}\bin\{exe}"),
            format!(r"C:\Program Files\{name}\bin\{exe2}"),
            format!(r"C:\Program Files (x86)\{name}\bin\{exe2}"),
            format!(r"C:\{name}\bin\{exe}"),
            format!(r"{home}\.{name}\bin\{bare}"),
            format!(r"{home}\scoop\apps\{name}\current\bin\{exe2}"),
        ]
    };
    #[cfg(not(target_os = "windows"))]
    let candidates: Vec<String> = vec![
        format!("/usr/local/bin/{name}"),
        format!("/usr/bin/{name}"),
        format!("/opt/homebrew/bin/{name}"),
        format!("{home}/.local/bin/{name}"),
        format!("{home}/.{name}/bin/{name}"),
        format!("/opt/{name}/bin/{name}"),
    ];

    let mut searched = Vec::new();
    for candidate in &candidates {
        // Expand simple glob (* at end of directory segment)
        if candidate.contains('*') {
            let (prefix, _) = candidate.split_once('*').unwrap_or((candidate, ""));
            let parent = std::path::Path::new(prefix).parent().unwrap_or(std::path::Path::new(prefix));
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let suffix = candidate.splitn(2, '*').nth(1).unwrap_or("");
                    let full = entry.path().to_string_lossy().to_string() + suffix;
                    let p = std::path::Path::new(&full);
                    if p.exists() {
                        return format!("Found (common location): {full}");
                    }
                    searched.push(full);
                }
            }
            continue;
        }
        let p = std::path::Path::new(candidate);
        if p.exists() {
            return format!("Found (common location): {candidate}");
        }
        searched.push(candidate.clone());
    }

    format!(
        "'{name}' not found in PATH or common locations.\nSearched:\n{}",
        searched.iter().take(8).map(|s| format!("  {s}")).collect::<Vec<_>>().join("\n")
    )
}

/// Detect installed language runtimes and build tools in a single call.
/// Returns a compact table of tool name, detected version, and resolved path.
pub fn tool_check_environment(args: &Value) -> String {
    let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");

    // (display_name, binary_names_to_try, version_args)
    let probes: &[(&str, &[&str], &[&str])] = &[
        ("java",    &["java"],           &["-version"]),
        ("javac",   &["javac"],          &["-version"]),
        ("mvn",     &["mvn", "mvn.cmd"], &["--version"]),
        ("gradle",  &["gradle"],         &["--version"]),
        ("node",    &["node"],           &["--version"]),
        ("npm",     &["npm", "npm.cmd"], &["--version"]),
        ("python",  &["python", "python3"], &["--version"]),
        ("pip",     &["pip", "pip3"],    &["--version"]),
        ("rustc",   &["rustc"],          &["--version"]),
        ("cargo",   &["cargo"],          &["--version"]),
        ("go",      &["go"],             &["version"]),
        ("git",     &["git"],            &["--version"]),
        ("docker",  &["docker"],         &["--version"]),
        ("dotnet",  &["dotnet"],         &["--version"]),
        ("php",     &["php"],            &["--version"]),
        ("ruby",    &["ruby"],           &["--version"]),
    ];

    let mut rows: Vec<String> = Vec::new();
    for (name, binaries, version_args) in probes {
        if !filter.is_empty() && !name.contains(filter) {
            continue;
        }
        let mut found = false;
        for bin in *binaries {
            let Ok(out) = silent_command(bin)
                .args(*version_args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
            else {
                continue;
            };
            // Some tools (java -version) write to stderr
            let raw = if out.stdout.is_empty() {
                String::from_utf8_lossy(&out.stderr).to_string()
            } else {
                String::from_utf8_lossy(&out.stdout).to_string()
            };
            let version = raw.lines().next().unwrap_or("").trim().to_string();
            if !version.is_empty() {
                // Resolve the path via where/which
                #[cfg(target_os = "windows")]
                let path_out = silent_command("where").arg(bin).stdin(std::process::Stdio::null()).output();
                #[cfg(not(target_os = "windows"))]
                let path_out = silent_command("which").arg(bin).stdin(std::process::Stdio::null()).output();
                let path = path_out.ok()
                    .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().lines().next().map(str::to_string)?) } else { None })
                    .unwrap_or_else(|| bin.to_string());
                rows.push(format!("  {name:<10} {version:<40} {path}"));
                found = true;
                break;
            }
        }
        if !found && filter.is_empty() {
            rows.push(format!("  {name:<10} not found"));
        }
    }

    if rows.is_empty() {
        return "No matching runtimes found.".to_string();
    }
    format!("Environment ({} tools):\n{:<10} {:<40} {}\n{}\n{}",
        rows.len(),
        "Tool", "Version", "Path",
        "-".repeat(90),
        rows.join("\n")
    )
}

pub fn tool_list_background_processes() -> String {
    let procs = llama_chat_command::background::list_all_background_processes();
    if procs.is_empty() {
        return "No background processes are currently tracked.".to_string();
    }
    let proc_count = procs.len();
    let mut lines = vec![format!("Background processes ({proc_count}):")];
    for (pid, cmd, _alive, status) in &procs {
        lines.push(format!("  PID {pid}: {cmd} [{status}]"));
    }
    lines.join("\n")
}
