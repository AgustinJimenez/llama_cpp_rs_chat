use super::*;
use crate::background::{register_streaming_process, unregister_streaming_process};

pub fn execute_command_streaming(
    cmd: &str,
    cancel: Option<Arc<AtomicBool>>,
    mut on_line: impl FnMut(&str),
) -> String {
    execute_command_streaming_with_timeout(cmd, cancel, None, &mut on_line)
}

pub fn execute_command_streaming_with_timeout(
    cmd: &str,
    cancel: Option<Arc<AtomicBool>>,
    timeout_override: Option<u64>,
    on_line: &mut dyn FnMut(&str),
) -> String {
    let trimmed = cmd.trim();

    let parts = parse_command_with_quotes(trimmed);
    if parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    let command_name = &parts[0];
    if command_name == "cd" {
        return execute_command(cmd);
    }

    let original_cwd = std::env::current_dir().unwrap_or_default();
    let has_shell_ops = trimmed.contains("&&")
        || trimmed.contains("||")
        || trimmed.contains(" | ")
        || trimmed.contains(';')
        || trimmed.contains('>')
        || trimmed.contains('<');

    if has_shell_ops && !cfg!(target_os = "windows") {
        if let Some(result) = try_native_echo_redirect(trimmed) {
            return result;
        }
    }

    let env_vars = [
        ("PYTHONUNBUFFERED", "1"),
        ("COMPOSER_PROCESS_TIMEOUT", "0"),
        ("GIT_FLUSH", "1"),
        ("CI", "true"),
    ];
    let persisted_env = get_shell_env();

    #[cfg(target_os = "windows")]
    let child_result = {
        let path = enriched_windows_path();
        let mut cmd = silent_command("cmd");
        cmd.raw_arg(format!("/C {trimmed} 2>&1")).env("PATH", &path);
        for (k, v) in &env_vars {
            cmd.env(k, v);
        }
        for (k, v) in &persisted_env {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    };

    #[cfg(not(target_os = "windows"))]
    let child_result = {
        let mut cmd = silent_command("sh");
        cmd.arg("-c").arg(format!("{trimmed} 2>&1"));
        for (k, v) in &env_vars {
            cmd.env(k, v);
        }
        for (k, v) in &persisted_env {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    };

    match child_result {
        Ok(mut child) => {
            let mut output = String::new();
            let child_pid = child.id();
            eprintln!(
                "[STREAM] Started: pid={} cmd={}",
                child_pid,
                &trimmed[..trimmed.len().min(100)]
            );
            // Register immediately so the PID survives a worker crash/restart.
            register_streaming_process(child_pid, &trimmed[..trimmed.len().min(200)]);
            let stdout_pipe = child.stdout.take();

            let inactivity_timeout_secs: u64 = timeout_override.unwrap_or(120);
            const POLL_INTERVAL_MS: u64 = 200;
            const MAX_WALL_CLOCK_SECS: u64 = 120;

            let mut was_cancelled = false;
            let mut inactivity_killed = false;
            let total_timeout_killed = false;
            let wall_start = Instant::now();

            if let Some(stdout) = stdout_pipe {
                let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
                std::thread::spawn(move || {
                    let mut reader = std::io::BufReader::new(stdout);
                    let mut buf = [0u8; 4096];
                    loop {
                        match std::io::Read::read(&mut reader, &mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                if tx.send(buf[..n].to_vec()).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });

                let mut line_buf = String::new();
                let mut last_data = Instant::now();

                loop {
                    if let Some(ref flag) = cancel {
                        if flag.load(Ordering::Relaxed) {
                            eprintln!("[STREAM] Cancelled by user, killing pid={child_pid}");
                            kill_process_tree(child_pid);
                            was_cancelled = true;
                            break;
                        }
                    }

                    let elapsed_secs = wall_start.elapsed().as_secs();
                    if elapsed_secs > 0
                        && elapsed_secs.is_multiple_of(60)
                        && elapsed_secs / 60 != (elapsed_secs - 1) / 60
                    {
                        eprintln!(
                            "[STREAM] Still running: {elapsed_secs}s elapsed, pid={child_pid}"
                        );
                    }

                    let elapsed = wall_start.elapsed().as_secs();
                    if elapsed == 60 || elapsed == 110 {
                        eprintln!(
                            "[STREAM] Wall-clock check: {elapsed}s / {MAX_WALL_CLOCK_SECS}s limit, pid={child_pid}"
                        );
                    }
                    if elapsed >= MAX_WALL_CLOCK_SECS {
                        let pid = child_pid;
                        eprintln!(
                            "[STREAM] Wall-clock limit ({MAX_WALL_CLOCK_SECS}s) reached, detaching from pid={pid}"
                        );
                        let lines: Vec<&str> = output.lines().collect();
                        if lines.len() > 40 {
                            output = format!(
                                "[...{} earlier lines truncated...]\n{}",
                                lines.len() - 40,
                                lines[lines.len() - 40..].join("\n")
                            );
                        }
                        output.push_str(&format!(
                            "\n[Command still running after {MAX_WALL_CLOCK_SECS}s (PID {pid}). It may be stuck or very slow. You can kill it with: taskkill /F /T /PID {pid}]\n"
                        ));
                        kill_process_tree(pid);
                        unregister_streaming_process(pid);
                        return output;
                    }

                    match rx.recv_timeout(std::time::Duration::from_millis(POLL_INTERVAL_MS)) {
                        Ok(data) => {
                            last_data = Instant::now();
                            let chunk = String::from_utf8_lossy(&data);
                            for ch in chunk.chars() {
                                if ch == '\n' || ch == '\r' {
                                    if !line_buf.is_empty() {
                                        on_line(&line_buf);
                                        output.push_str(&line_buf);
                                        output.push('\n');
                                        line_buf.clear();
                                    }
                                } else {
                                    line_buf.push(ch);
                                }
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            if last_data.elapsed().as_secs() >= inactivity_timeout_secs {
                                eprintln!(
                                    "[STREAM] Inactivity timeout ({inactivity_timeout_secs}s no output), killing pid={child_pid}"
                                );
                                kill_process_tree(child_pid);
                                inactivity_killed = true;
                                break;
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            {
                            let elapsed = wall_start.elapsed().as_secs();
                            eprintln!(
                                "[STREAM] Pipe disconnected after {elapsed}s, pid={child_pid}"
                            );
                        }
                            break;
                        }
                    }
                }

                if !line_buf.is_empty() {
                    on_line(&line_buf);
                    output.push_str(&line_buf);
                    output.push('\n');
                }
            }

            const POST_PIPE_WALL_LIMIT: u64 = MAX_WALL_CLOCK_SECS;
            if wall_start.elapsed().as_secs() >= POST_PIPE_WALL_LIMIT {
                {
                let elapsed = wall_start.elapsed().as_secs();
                eprintln!(
                    "[STREAM] Wall-clock exceeded after pipe closed ({elapsed}s), killing pid={child_pid}"
                );
            }
                kill_process_tree(child_pid);
                unregister_streaming_process(child_pid);
                {
                    let elapsed = wall_start.elapsed().as_secs();
                    output.push_str(&format!(
                        "\n[Command killed after {elapsed}s wall-clock limit]\n"
                    ));
                }
                return output;
            }

            let mut exit_code = -1i32;
            let mut success = false;
            let reap_deadline = std::time::Duration::from_secs(if wall_start.elapsed().as_secs()
                >= POST_PIPE_WALL_LIMIT
            {
                1
            } else {
                5
            });
            let reap_start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(s)) => {
                        exit_code = s.code().unwrap_or(-1);
                        success = s.success();
                        break;
                    }
                    Ok(None)
                        if reap_start.elapsed() < reap_deadline
                            && wall_start.elapsed().as_secs() < MAX_WALL_CLOCK_SECS =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                    _ => {
                        eprintln!("[STREAM] Killing unreaped child pid={child_pid}");
                        kill_process_tree(child_pid);
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        if let Ok(Some(s)) = child.try_wait() {
                            exit_code = s.code().unwrap_or(-1);
                            success = s.success();
                        }
                        break;
                    }
                }
            }

            // Process is dead (exited, killed, or cancelled) — remove from DB.
            unregister_streaming_process(child_pid);

            if was_cancelled {
                output.push_str("\n[Cancelled by user]\n");
            } else if total_timeout_killed || inactivity_killed {
                {
                    let double_timeout = inactivity_timeout_secs * 2;
                    output.push_str(&format!(
                        "\n[Process killed: no output for {inactivity_timeout_secs}s. TIP: Use \"timeout\": {double_timeout} in your tool call for slow commands, or \"background\": true for servers/daemons.]\n"
                    ));
                }
            }

            track_cwd_change(trimmed);
            capture_env_from_command(trimmed);
            let annotation = cwd_annotation(&original_cwd).unwrap_or_default();

            if output.trim().is_empty() {
                if success {
                    format!("Command executed successfully (no output){annotation}")
                } else {
                    format!(
                        "Command failed with exit code {exit_code} and produced no output.{annotation}"
                    )
                }
            } else {
                format!("{output}{annotation}")
            }
        }
        Err(e) => {
            if cfg!(target_os = "windows")
                && !has_shell_ops
                && e.kind() == std::io::ErrorKind::NotFound
            {
                execute_command(cmd)
            } else {
                format!("Failed to execute command: {e}")
            }
        }
    }
}
