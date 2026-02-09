use std::env;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command};
use sysinfo::System;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let build = args.iter().any(|a| a == "--build" || a == "-b");
    let debug = args.iter().any(|a| a == "--debug" || a == "-d");
    let help = args.iter().any(|a| a == "--help" || a == "-h");

    // Parse --gpu <backend>
    let gpu = args
        .windows(2)
        .find(|w| w[0] == "--gpu" || w[0] == "-g")
        .map(|w| w[1].as_str())
        .unwrap_or("cpu");

    let gpu = match gpu {
        "cuda" | "vulkan" | "cpu" => gpu,
        other => {
            eprintln!("\x1b[31mUnknown GPU backend: {other}\x1b[0m");
            eprintln!("Valid options: cuda, vulkan, cpu");
            process::exit(1);
        }
    };

    if help {
        println!("start-dev: Launch backend + frontend dev server");
        println!();
        println!("Usage: start-dev [OPTIONS]");
        println!();
        println!("Options:");
        println!("  -b, --build          Rebuild before starting");
        println!("  -g, --gpu <BACKEND>  GPU backend: cuda, vulkan, cpu (default: cpu)");
        println!("  -d, --debug          Use debug profile (default is release)");
        println!("  -h, --help           Show this help");
        println!();
        println!("Examples:");
        println!("  start-dev --build --gpu cuda     Build + run with CUDA");
        println!("  start-dev --build --gpu vulkan   Build + run with Vulkan");
        println!("  start-dev --build                Build + run CPU-only");
        println!("  start-dev                        Run existing binary (instant start)");
        return;
    }

    let project_root = find_project_root();
    let profile = if debug { "debug" } else { "release" };

    // 1. Kill existing processes
    println!("\x1b[36m[1/3] Cleaning up old processes...\x1b[0m");
    kill_by_name("llama_chat_web");
    kill_port_holders(4000);
    std::thread::sleep(std::time::Duration::from_secs(1));

    // 2. Optionally rebuild
    if build {
        let gpu_label = if gpu == "cpu" {
            "CPU".to_string()
        } else {
            gpu.to_uppercase()
        };
        println!("\x1b[36m[2/3] Building ({profile}, {gpu_label})...\x1b[0m");

        // Ensure cmake is available
        let cmake = ensure_cmake::ensure_cmake(Some(&project_root.join("target")))
            .unwrap_or_else(|e| {
                eprintln!("\x1b[31mFailed to ensure cmake: {e}\x1b[0m");
                process::exit(1);
            });

        let mut cmd = Command::new("cargo");
        cmd.current_dir(&project_root)
            .args(["build", "--bin", "llama_chat_web"]);

        if gpu != "cpu" {
            cmd.args(["--features", gpu]);
        }
        if !debug {
            cmd.arg("--release");
        }

        cmake.apply_to_command(&mut cmd);
        let status = cmd.status().expect("Failed to run cargo build");
        if !status.success() {
            eprintln!("\x1b[31mBuild failed!\x1b[0m");
            process::exit(1);
        }
    } else {
        let exe = backend_exe_path(&project_root, profile);
        if !exe.exists() {
            eprintln!(
                "\x1b[31mNo binary at {} — run with --build first\x1b[0m",
                exe.display()
            );
            process::exit(1);
        }
        println!("\x1b[33m[2/3] Skipping build (using existing {profile} binary)\x1b[0m");
    }

    // 3. Start both services
    println!("\x1b[36m[3/3] Starting backend + frontend...\x1b[0m");

    let exe = backend_exe_path(&project_root, profile);
    let backend = Command::new(&exe)
        .current_dir(&project_root)
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!("\x1b[31mFailed to start backend: {e}\x1b[0m");
            process::exit(1);
        });

    // Wait for backend to be ready
    wait_for_port(8000, 15);

    let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
    let frontend = Command::new(npx)
        .current_dir(&project_root)
        .args(["vite", "--host", "--port", "4000"])
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!("\x1b[31mFailed to start frontend: {e}\x1b[0m");
            process::exit(1);
        });

    // Wait for frontend to be ready
    wait_for_port(4000, 10);

    println!();
    println!("\x1b[32mReady!\x1b[0m");
    println!("  Backend:  http://localhost:8000");
    println!("  Frontend: http://localhost:4000");
    println!();
    println!("\x1b[90mPress Ctrl+C to stop both.\x1b[0m");

    wait_and_cleanup(backend, frontend);
}

fn find_project_root() -> PathBuf {
    let start = env::current_dir().unwrap();
    let mut dir = start.as_path();
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("package.json").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => {
                eprintln!("Could not find project root (Cargo.toml + package.json)");
                process::exit(1);
            }
        }
    }
}

fn backend_exe_path(root: &Path, profile: &str) -> PathBuf {
    let name = if cfg!(windows) {
        "llama_chat_web.exe"
    } else {
        "llama_chat_web"
    };
    root.join("target").join(profile).join(name)
}

fn kill_by_name(name: &str) {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let mut killed = 0;
    for (pid, proc_) in sys.processes() {
        if proc_.name().to_string_lossy().contains(name) {
            if proc_.kill() {
                killed += 1;
                println!("  Killed {} (PID {})", proc_.name().to_string_lossy(), pid);
            }
        }
    }
    if killed == 0 {
        println!("  No {name} processes found");
    }
}

/// Kill whatever is holding a TCP port by asking the OS.
fn kill_port_holders(port: u16) {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let pids = find_pids_on_port(port);
    if pids.is_empty() {
        println!("  Port {port} is free");
        return;
    }

    for pid in pids {
        let spid = sysinfo::Pid::from_u32(pid);
        if let Some(proc_) = sys.process(spid) {
            if proc_.kill() {
                println!(
                    "  Killed {} (PID {pid}) on port {port}",
                    proc_.name().to_string_lossy()
                );
            }
        }
    }
}

fn find_pids_on_port(port: u16) -> Vec<u32> {
    let output = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", &format!("netstat -ano | findstr :{port} | findstr LISTENING")])
            .output()
    } else {
        Command::new("sh")
            .args(["-c", &format!("lsof -ti :{port}")])
            .output()
    };

    let Ok(output) = output else {
        return vec![];
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut pids = Vec::new();

    if cfg!(windows) {
        for line in text.lines() {
            if let Some(pid_str) = line.split_whitespace().last() {
                if let Ok(pid) = pid_str.parse::<u32>() {
                    if !pids.contains(&pid) {
                        pids.push(pid);
                    }
                }
            }
        }
    } else {
        for line in text.lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                if !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
        }
    }

    pids
}

fn wait_for_port(port: u16, timeout_secs: u64) {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    while start.elapsed() < timeout {
        if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    eprintln!("\x1b[33mWarning: port {port} not ready after {timeout_secs}s\x1b[0m");
}

fn wait_and_cleanup(mut backend: Child, mut frontend: Child) {
    loop {
        if let Ok(Some(_)) = backend.try_wait() {
            println!("\x1b[33mBackend exited. Stopping frontend...\x1b[0m");
            let _ = frontend.kill();
            break;
        }
        if let Ok(Some(_)) = frontend.try_wait() {
            println!("\x1b[33mFrontend exited. Stopping backend...\x1b[0m");
            let _ = backend.kill();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}
