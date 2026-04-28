use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, OnceLock};

// ── Persistent shell environment ─────────────────────────────────────────────
// Each command runs in a fresh subshell, so `export`/`set` assignments are lost.
// We parse explicit assignments from commands and inject them into subsequent
// child processes, giving the illusion of persistent shell state.

static SHELL_ENV: OnceLock<StdMutex<HashMap<String, String>>> = OnceLock::new();

fn shell_env() -> &'static StdMutex<HashMap<String, String>> {
    SHELL_ENV.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Get the persisted shell environment as a HashMap.
pub fn get_shell_env() -> HashMap<String, String> {
    shell_env()
        .lock()
        .ok()
        .map(|env| env.clone())
        .unwrap_or_default()
}

/// Parse and persist explicit environment variable assignments from a command.
/// Recognises `set VAR=value` (Windows) and `export VAR=value` / `VAR=value` (Unix).
pub fn capture_env_from_command(cmd: &str) {
    let trimmed = cmd.trim();

    // Split on && and ; to handle chained commands
    for part in trimmed.split("&&").flat_map(|s| s.split(';')) {
        let part = part.trim();

        #[cfg(target_os = "windows")]
        {
            if let Some(rest) = part
                .strip_prefix("set ")
                .or_else(|| part.strip_prefix("SET "))
            {
                if let Some((key, value)) = rest.split_once('=') {
                    let key = key.trim().to_string();
                    let value = value.trim().trim_matches('"').to_string();
                    if !key.is_empty() {
                        eprintln!(
                            "[SHELL_ENV] Captured: {}={}",
                            key,
                            &value[..value.len().min(50)]
                        );
                        if let Ok(mut env) = shell_env().lock() {
                            env.insert(key, value);
                        }
                    }
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let assignment = part.strip_prefix("export ").unwrap_or(part);
            if let Some((key, value)) = assignment.split_once('=') {
                let key = key.trim().to_string();
                // Only capture if it looks like a variable name
                if key
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_alphabetic() || c == '_')
                    .unwrap_or(false)
                    && !key.contains(' ')
                {
                    let value = value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                    eprintln!(
                        "[SHELL_ENV] Captured: {}={}",
                        key,
                        &value[..value.len().min(50)]
                    );
                    if let Ok(mut env) = shell_env().lock() {
                        env.insert(key, value);
                    }
                }
            }
        }
    }
}
