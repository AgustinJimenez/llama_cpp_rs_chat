use serde_json::Value;

use crate::command_tools;
use crate::file_tools;
use crate::mcp_tools;
use crate::search_tools;
use crate::{DispatchContext, McpManagerOps};
use super::todo_store;

// RTK compresses output for specific developer tooling only. Applying it to arbitrary
// commands (mkdir, python, cd, etc.) breaks Windows shell builtins and `&&` chains
// because RTK doesn't know those commands and its fallback can't find non-exe builtins.
fn rtk_prefix_for_tool(cmd: &str) -> String {
    const RTK_COMMANDS: &[&str] = &[
        "git", "cargo", "npm", "npx", "pnpm", "yarn", "bun",
        "tsc", "vitest", "playwright", "jest", "pytest",
        "prettier", "eslint", "biome", "lint",
        "docker", "kubectl", "gh",
    ];
    let has_shell_ops = cmd.contains("&&") || cmd.contains("||")
        || cmd.contains(" | ") || cmd.contains(';')
        || cmd.contains('>') || cmd.contains('<');
    let first = cmd.split_whitespace().next().unwrap_or("");
    if !has_shell_ops && RTK_COMMANDS.contains(&first) {
        llama_chat_command::rtk_prefix(cmd)
    } else {
        cmd.to_string()
    }
}

pub(super) fn dispatch_text_tool(
    name: &str,
    args: &Value,
    mcp_manager: Option<&dyn McpManagerOps>,
    db: Option<&llama_chat_db::SharedDatabase>,
    ctx: &DispatchContext<'_>,
) -> Option<String> {
    Some(match name {
        "read_file" => file_tools::tool_read_file(args),
        "write_file" => file_tools::tool_write_file(args),
        "edit_file" => file_tools::tool_edit_file(args),
        "multi_edit" => file_tools::tool_multi_edit(args),
        "undo_edit" => file_tools::tool_undo_edit(args),
        "insert_text" => file_tools::tool_insert_text(args),
        "search_files" => search_tools::tool_search_files(args),
        "find_files" => search_tools::tool_find_files(args),
        "execute_python" => command_tools::tool_execute_python(args),
        "list_directory" => command_tools::tool_list_directory(args),
        "execute_command" => {
            let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if command.is_empty() {
                return Some("Error: 'command' argument is required".to_string());
            }
            let command = rtk_prefix_for_tool(command);
            let is_background = args
                .get("background")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_background {
                llama_chat_command::background::execute_command_background(&command, |_| {})
            } else {
                let timeout = args.get("timeout").and_then(|v| v.as_u64());
                llama_chat_command::execute_command_streaming_with_timeout(
                    &command,
                    None,
                    timeout,
                    &mut |_| {},
                )
            }
        }
        "execute_pty" => {
            let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if command.is_empty() {
                return Some("Error: 'command' argument is required".to_string());
            }
            let command = rtk_prefix_for_tool(command);
            llama_chat_command::execute_command_pty(&command, None, |_: &str| {})
        }
        "check_background_process" => {
            let pid = args
                .get("pid")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                })
                .unwrap_or(0) as u32;
            if pid == 0 {
                return Some(
                    "Error: 'pid' argument is required and must be a positive integer".to_string(),
                );
            }
            let wait_seconds = args
                .get("wait_seconds")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                })
                .unwrap_or(0);
            let max_checks = args
                .get("max_checks")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                })
                .unwrap_or(5) as usize;
            llama_chat_command::background::check_background_process(pid, wait_seconds, max_checks)
        }
        "wait" => {
            let seconds = args
                .get("seconds")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                })
                .unwrap_or(10)
                .min(30);
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            format!(
                "Waited {seconds} seconds. You can now check on background processes or continue.",
            )
        }
        "lsp_query" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("definition");
            let symbol = args.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
            let file = args.get("file").and_then(|v| v.as_str());
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            if symbol.is_empty() && action != "symbols" && action != "diagnostics" {
                "Error: 'symbol' is required".to_string()
            } else {
                let result = match action {
                    "definition" => {
                        if let Some(ctags) = command_tools::get_ctags(path) {
                            let matches: Vec<&str> = ctags
                                .lines()
                                .filter(|line| {
                                    line.contains(&format!("\"name\":\"{symbol}\""))
                                        || line.starts_with(&format!("{symbol}\t"))
                                })
                                .take(10)
                                .collect();
                            if !matches.is_empty() {
                                let matches_str = matches.join("\n");
                                format!("Definitions found via ctags:\n{matches_str}")
                            } else {
                                command_tools::lsp_ripgrep_definition(symbol, path)
                            }
                        } else {
                            command_tools::lsp_ripgrep_definition(symbol, path)
                        }
                    }
                    "references" => {
                        let cmd = format!(
                            "rg -n -w \"{symbol}\" \"{path}\" --type-add \"code:*.{{rs,ts,tsx,js,jsx,py,go,java,c,cpp,h,hpp,nim,ex}}\" -t code --max-count 30"
                        );
                        llama_chat_command::execute_command(&cmd)
                    }
                    "symbols" => {
                        let target = file.unwrap_or(path);
                        if let Some(ctags) = command_tools::get_ctags(target) {
                            let file_matches: Vec<&str> = ctags
                                .lines()
                                .filter(|line| file.is_none_or(|f| line.contains(f)))
                                .take(50)
                                .collect();
                            if !file_matches.is_empty() {
                                let matches_str = file_matches.join("\n");
                                format!("Symbols:\n{matches_str}")
                            } else {
                                command_tools::lsp_ripgrep_symbols(target)
                            }
                        } else {
                            command_tools::lsp_ripgrep_symbols(target)
                        }
                    }
                    "diagnostics" => {
                        let ext = file
                            .and_then(|f| std::path::Path::new(f).extension())
                            .and_then(|e| e.to_str())
                            .unwrap_or("");
                        match ext {
                            "rs" => llama_chat_command::execute_command(
                                "cargo check --message-format=short 2>&1 | head -30",
                            ),
                            "py" => {
                                if let Some(f) = file {
                                    llama_chat_command::execute_command(&format!(
                                        "python -m py_compile {f} 2>&1"
                                    ))
                                } else {
                                    "Error: 'file' is required for Python diagnostics".to_string()
                                }
                            }
                            "ts" | "tsx" => llama_chat_command::execute_command(
                                "npx tsc --noEmit 2>&1 | head -30",
                            ),
                            "nim" => {
                                if let Some(f) = file {
                                    llama_chat_command::execute_command(&format!(
                                        "nim check {f} 2>&1 | head -30"
                                    ))
                                } else {
                                    "Error: 'file' is required for Nim diagnostics".to_string()
                                }
                            }
                            _ => "No diagnostic tool available for this file type. Use execute_command to run your build tool.".to_string(),
                        }
                    }
                    "hover" => {
                        let escaped = regex::escape(symbol);
                        let pattern =
                            format!(r"(fn|struct|enum|trait|type|class|def|interface)\s+{escaped}");
                        let cmd = format!(
                            "rg -n -A 5 \"{pattern}\" \"{path}\" --type-add \"code:*.{{rs,ts,tsx,js,jsx,py,go,java,c,cpp,h,hpp}}\" -t code --max-count 5"
                        );
                        llama_chat_command::execute_command(&cmd)
                    }
                    _ => format!(
                        "Unknown action '{action}'. Use: definition, references, symbols, hover, diagnostics"
                    ),
                };
                if result.trim().is_empty() {
                    format!("No results found for '{symbol}' ({action}) in {path}")
                } else {
                    result
                }
            }
        }
        "git_status" => command_tools::tool_git_status(args),
        "git_diff" => command_tools::tool_git_diff(args),
        "git_commit" => command_tools::tool_git_commit(args),
        "list_mcp_servers" => mcp_tools::tool_list_mcp_servers(mcp_manager, db),
        "add_mcp_server" => mcp_tools::tool_add_mcp_server(args, mcp_manager, db),
        "remove_mcp_server" => mcp_tools::tool_remove_mcp_server(args, mcp_manager, db),
        "list_background_processes" => command_tools::tool_list_background_processes(),
        "sleep" => {
            let seconds = args.get("seconds").and_then(|v| v.as_u64()).unwrap_or(5).min(30);
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            format!("Waited {seconds} seconds")
        }
        "todo_write" => {
            let todos = args.get("todos").and_then(|v| v.as_str()).unwrap_or("[]");
            match serde_json::from_str::<serde_json::Value>(todos) {
                Ok(val) => {
                    let formatted =
                        serde_json::to_string_pretty(&val).unwrap_or_else(|_| todos.to_string());
                    if let Ok(mut store) = todo_store().lock() {
                        store.insert("default".to_string(), formatted.clone());
                    }
                    format!("Todo list updated:\n{formatted}")
                }
                Err(e) => format!(
                    "Error: Invalid JSON for todos: {e}. Expected array of {{id, task, status}} objects."
                ),
            }
        }
        "todo_read" => {
            let todos = todo_store()
                .lock()
                .ok()
                .and_then(|store| store.get("default").cloned())
                .unwrap_or_else(|| "[]".to_string());
            if todos == "[]" {
                "No todos yet. Use todo_write to create a task checklist.".to_string()
            } else {
                format!("Current todos:\n{todos}")
            }
        }
        "list_skills" => {
            let cwd = std::env::current_dir().unwrap_or_default();
            if let Some(discover) = ctx.discover_skills {
                let skills = discover(&cwd);
                if skills.is_empty() {
                    "No skills found. Create .md files in a 'skills/' directory with YAML frontmatter (name, description).".to_string()
                } else {
                    let skill_count = skills.len();
                    let mut output = format!("{skill_count} skills available:\n");
                    for s in &skills {
                        let s_name = &s.name;
                        let s_desc = &s.description;
                        output.push_str(&format!("  {s_name} — {s_desc}\n"));
                    }
                    output
                }
            } else {
                "Skills system not available".to_string()
            }
        }
        "use_skill" => {
            let skill_name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let template_args = args.get("args").and_then(|v| v.as_str()).unwrap_or("{}");
            let cwd = std::env::current_dir().unwrap_or_default();
            if let Some(get_skill_fn) = ctx.get_skill {
                match get_skill_fn(&cwd, skill_name) {
                    Some(skill) => {
                        let mut content = skill.content.clone();
                        if let Ok(args_map) =
                            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                                template_args,
                            )
                        {
                            for (key, val) in &args_map {
                                let placeholder = format!("{{{{{key}}}}}");
                                let replacement = match val.as_str() {
                                    Some(s) => s.to_string(),
                                    None => val.to_string(),
                                };
                                content = content.replace(&placeholder, &replacement);
                            }
                        }
                        format!("Skill '{skill_name}' loaded:\n\n{content}")
                    }
                    None => format!(
                        "Skill '{skill_name}' not found. Use list_skills to see available skills."
                    ),
                }
            } else {
                "Skills system not available".to_string()
            }
        }
        "set_response_style" => {
            let style = args.get("style").and_then(|v| v.as_str()).unwrap_or("detailed");
            match style {
                "brief" => "Response style set to BRIEF. From now on: be concise, skip explanations, show only results and actions. No preamble or summaries.".to_string(),
                _ => "Response style set to DETAILED. From now on: explain your reasoning, show context, and provide thorough responses.".to_string(),
            }
        }
        "open_url" => {
            let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if url.is_empty() {
                "Error: 'url' argument is required".to_string()
            } else if !url.starts_with("http://") && !url.starts_with("https://") {
                format!("Error: URL must start with http:// or https://, got: {url}")
            } else {
                #[cfg(target_os = "windows")]
                let result = std::process::Command::new("cmd")
                    .args(["/C", "start", "", url])
                    .spawn();
                #[cfg(target_os = "macos")]
                let result = std::process::Command::new("open").arg(url).spawn();
                #[cfg(target_os = "linux")]
                let result = std::process::Command::new("xdg-open").arg(url).spawn();
                match result {
                    Ok(_) => format!("Opened {url} in the default browser"),
                    Err(e) => format!("Failed to open URL: {e}"),
                }
            }
        }
        _ => return None,
    })
}
