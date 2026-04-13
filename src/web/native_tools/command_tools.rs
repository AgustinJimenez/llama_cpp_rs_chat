//! Command execution, directory listing, git tools, and LSP/ctags helpers.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;

use crate::web::utils::silent_command;

// ─── ctags cache for lsp_query ─────────────────────────────────────────────────
static CTAGS_CACHE: OnceLock<StdMutex<HashMap<String, (std::time::Instant, String)>>> = OnceLock::new();

fn ctags_cache() -> &'static StdMutex<HashMap<String, (std::time::Instant, String)>> {
    CTAGS_CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Generate or retrieve cached ctags output for a project/file path.
/// Returns None if ctags is not installed or fails.
pub(super) fn get_ctags(path: &str) -> Option<String> {
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
pub(super) fn lsp_ripgrep_definition(symbol: &str, path: &str) -> String {
    let escaped = regex::escape(symbol);
    let patterns = [
        format!(r"(fn|pub fn|pub\(crate\) fn|async fn|pub async fn)\s+{}\s*[\(<]", escaped),
        format!(r"(struct|enum|trait|type|pub struct|pub enum|pub trait|pub type)\s+{}\s*[\{{<]", escaped),
        format!(r"(class|interface|function|const|let|var|def|defn?)\s+{}\s*[\({{:<= ]", escaped),
        format!(r"(impl|impl<[^>]*>)\s+{}\s", escaped),
    ];
    let combined = patterns.join("|");
    let cmd = format!(
        "rg -n -e \"{}\" \"{}\" --type-add \"code:*.{{rs,ts,tsx,js,jsx,py,go,java,c,cpp,h,hpp,nim,ex}}\" -t code --max-count 20",
        combined.replace('"', "\\\""), path
    );
    super::super::command::execute_command(&cmd)
}

/// ripgrep-based symbol listing (fallback when ctags unavailable)
pub(super) fn lsp_ripgrep_symbols(target: &str) -> String {
    let cmd = format!(
        "rg -n \"(fn |struct |enum |trait |class |interface |function |def |const |type |impl )\" \"{}\" --max-count 50",
        target
    );
    super::super::command::execute_command(&cmd)
}

/// Execute Python code by writing to a temp file and running it.
/// This completely bypasses shell quoting — the code goes directly to a .py file.
pub(super) fn tool_execute_python(args: &Value) -> String {
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'code' argument is required".to_string(),
    };

    // Write code to a temp file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!(
        "llama_tool_{}.py",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

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
pub(super) fn tool_list_directory(args: &Value) -> String {
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
pub(super) fn tool_git_status(args: &Value) -> String {
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
                return format!("Error (exit {}): {}", output.status.code().unwrap_or(-1), stderr.trim());
            }
            if stdout.trim().is_empty() {
                "Working tree clean (no changes)".to_string()
            } else {
                format!("Git status:\n{}", stdout)
            }
        }
        Err(e) => format!("Error running git: {e}"),
    }
}

/// Show git diff for files.
pub(super) fn tool_git_diff(args: &Value) -> String {
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
        // If path looks like a repo dir, use current_dir; otherwise it's a file arg
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
                return format!("Error (exit {}): {}", output.status.code().unwrap_or(-1), stderr.trim());
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
pub(super) fn tool_git_commit(args: &Value) -> String {
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
                format!("Error (exit {}): {}\n{}", output.status.code().unwrap_or(-1), stderr.trim(), stdout.trim())
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error running git: {e}"),
    }
}

pub(super) fn tool_list_background_processes() -> String {
    let procs = super::super::background::list_all_background_processes();
    if procs.is_empty() {
        return "No background processes are currently tracked.".to_string();
    }
    let mut lines = vec![format!("Background processes ({}):", procs.len())];
    for (pid, cmd, _alive, status) in &procs {
        lines.push(format!("  PID {}: {} [{}]", pid, cmd, status));
    }
    lines.join("\n")
}
