use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(super) fn setup_background_noise(
    level: u32,
    running: &Arc<AtomicBool>,
    heartbeat: &Arc<AtomicU64>,
) -> (
    Option<tokio::runtime::Runtime>,
    Vec<std::thread::JoinHandle<()>>,
) {
    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();
    let rt_handle;

    match level {
        0 => rt_handle = None,
        1 => {
            rt_handle = Some(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(4)
                    .enable_all()
                    .build()
                    .unwrap(),
            );
        }
        2 => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            let r = running.clone();
            let hb = heartbeat.clone();
            rt.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                    hb.fetch_add(1, Ordering::Relaxed);
                }
            });

            let r2 = running.clone();
            rt.spawn(async move {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let _tx = tx;
                while r2.load(Ordering::Relaxed) {
                    tokio::select! {
                        _ = rx.recv() => {},
                        _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {},
                    }
                }
            });

            rt_handle = Some(rt);
        }
        3 => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            let r = running.clone();
            rt.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                }
            });

            for i in 0..2 {
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    while r.load(Ordering::Relaxed) {
                        let mut sum: u64 = 0;
                        for j in 0..10000 {
                            sum = sum.wrapping_add(j * (i as u64 + 1));
                        }
                        std::hint::black_box(sum);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }));
            }

            rt_handle = Some(rt);
        }
        4 => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            let r = running.clone();
            let hb = heartbeat.clone();
            rt.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                    hb.fetch_add(1, Ordering::Relaxed);
                }
            });

            for i in 0..2 {
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    while r.load(Ordering::Relaxed) {
                        let mut sum: u64 = 0;
                        for j in 0..10000 {
                            sum = sum.wrapping_add(j * (i as u64 + 1));
                        }
                        std::hint::black_box(sum);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }));
            }

            let r = running.clone();
            handles.push(std::thread::spawn(move || {
                while r.load(Ordering::Relaxed) {
                    std::hint::black_box(std::time::SystemTime::now());
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }));

            rt_handle = Some(rt);
        }
        5 => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            let r = running.clone();
            let hb = heartbeat.clone();
            rt.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                    hb.fetch_add(1, Ordering::Relaxed);
                }
            });

            let r = running.clone();
            handles.push(std::thread::spawn(move || {
                let (tx, rx) = std::sync::mpsc::channel::<String>();
                let r2 = r.clone();
                std::thread::spawn(move || {
                    while r2.load(Ordering::Relaxed) {
                        let _ = tx.send("{\"id\":1,\"command\":\"status\"}".to_string());
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                });
                while r.load(Ordering::Relaxed) {
                    if let Ok(_msg) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
                        std::hint::black_box(42);
                    }
                }
            }));

            let r = running.clone();
            handles.push(std::thread::spawn(move || {
                while r.load(Ordering::Relaxed) {
                    let response =
                        "{\"id\":1,\"type\":\"status\",\"data\":{\"ok\":true}}".to_string();
                    std::hint::black_box(response);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }));

            for i in 0..2 {
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    while r.load(Ordering::Relaxed) {
                        let mut sum: u64 = 0;
                        for j in 0..10000 {
                            sum = sum.wrapping_add(j * (i as u64 + 1));
                        }
                        std::hint::black_box(sum);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }));
            }

            rt_handle = Some(rt);
        }
        6 => rt_handle = None,
        7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24
        | 25 | 26 => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            let r = running.clone();
            let hb = heartbeat.clone();
            rt.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                    hb.fetch_add(1, Ordering::Relaxed);
                }
            });

            let r = running.clone();
            handles.push(std::thread::spawn(move || {
                while r.load(Ordering::Relaxed) {
                    let child = std::process::Command::new("cmd.exe")
                        .args(["/C", "echo test_output & timeout /t 1 /nobreak >nul"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .spawn();
                    if let Ok(mut child) = child {
                        let _ = child.wait();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }));

            if level >= 8 {
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    use std::io::BufRead;
                    while r.load(Ordering::Relaxed) {
                        let child = std::process::Command::new("cmd.exe")
                            .args([
                                "/C",
                                "echo line1 & echo line2 & echo line3 & ping -n 2 127.0.0.1 >nul",
                            ])
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                        if let Ok(mut child) = child {
                            if let Some(stdout) = child.stdout.take() {
                                let reader = std::io::BufReader::new(stdout);
                                for l in reader.lines().flatten() {
                                    std::hint::black_box(l);
                                }
                            }
                            let _ = child.wait();
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                }));
            }

            if level >= 9 {
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    let db_path = std::env::temp_dir().join("llama_test_deadlock.db");
                    let conn = match rusqlite::Connection::open(&db_path) {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    let _ = conn.execute(
                        "CREATE TABLE IF NOT EXISTS tokens (id INTEGER PRIMARY KEY, token TEXT, ts INTEGER)",
                        [],
                    );
                    let mut counter = 0u64;
                    while r.load(Ordering::Relaxed) {
                        counter += 1;
                        let _ = conn.execute(
                            "INSERT INTO tokens (token, ts) VALUES (?1, ?2)",
                            rusqlite::params![format!("tok_{counter}"), counter],
                        );
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    let _ = std::fs::remove_file(&db_path);
                }));
            }

            if level >= 14 {
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    use std::io::BufRead;
                    let child = std::process::Command::new("cmd.exe")
                        .args(["/C", "for /L %i in (1,1,3600) do @(echo heartbeat_%i & ping -n 2 127.0.0.1 >nul)"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    if let Ok(mut child) = child {
                        if let Some(stdout) = child.stdout.take() {
                            let reader = std::io::BufReader::new(stdout);
                            for line in reader.lines() {
                                if !r.load(Ordering::Relaxed) {
                                    break;
                                }
                                if let Ok(l) = line {
                                    std::hint::black_box(l);
                                }
                            }
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }));

                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    use std::io::BufRead;
                    let child = std::process::Command::new("cmd.exe")
                        .args(["/C", "for /L %i in (1,1,3600) do @(echo output_%i & ping -n 3 127.0.0.1 >nul)"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    if let Ok(mut child) = child {
                        if let Some(stdout) = child.stdout.take() {
                            let reader = std::io::BufReader::new(stdout);
                            for line in reader.lines() {
                                if !r.load(Ordering::Relaxed) {
                                    break;
                                }
                                if let Ok(l) = line {
                                    std::hint::black_box(l);
                                }
                            }
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }));
            }

            if level >= 16 {
                for i in 0..3 {
                    let r = running.clone();
                    handles.push(std::thread::spawn(move || {
                        use std::io::BufRead;
                        while r.load(Ordering::Relaxed) {
                            let child = std::process::Command::new("cmd.exe")
                                .args(["/C", &format!(
                                    "echo child_{i}_start & dir /s /b C:\\Windows\\System32\\*.dll 2>nul | find /c \"dll\" & echo child_{i}_done"
                                )])
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::piped())
                                .stderr(std::process::Stdio::null())
                                .spawn();
                            if let Ok(mut child) = child {
                                if let Some(stdout) = child.stdout.take() {
                                    let reader = std::io::BufReader::new(stdout);
                                    for l in reader.lines().flatten() {
                                        std::hint::black_box(l);
                                    }
                                }
                                let _ = child.wait();
                            }
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                    }));
                }
            }

            rt_handle = Some(rt);
        }
        15 => rt_handle = None,
        _ => rt_handle = None,
    }

    (rt_handle, handles)
}
