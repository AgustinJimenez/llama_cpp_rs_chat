use tauri::State;

use crate::web::background::{is_process_alive, kill_background_process_by_pid};
use crate::web::database::SharedDatabase;

/// List all alive background processes tracked in the database.
#[tauri::command]
pub fn get_background_processes(
    db: State<'_, SharedDatabase>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.connection();
    let mut stmt = conn
        .prepare("SELECT pid, command, conversation_id, started_at, session_id FROM background_processes")
        .map_err(|e| e.to_string())?;

    let rows: Vec<(i64, String, Option<String>, i64, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let mut processes = Vec::new();
    let mut dead_pids = Vec::new();

    for (pid, command, conversation_id, started_at, _session_id) in &rows {
        let pid_val = u32::try_from(*pid).unwrap_or(0);
        let alive = is_process_alive(pid_val);
        if !alive {
            dead_pids.push(*pid);
        }
        processes.push(serde_json::json!({
            "pid": pid,
            "command": command,
            "conversationId": conversation_id,
            "startedAt": started_at,
            "alive": alive,
        }));
    }

    // Clean up dead records
    for pid in &dead_pids {
        let _ = conn.execute("DELETE FROM background_processes WHERE pid = ?1", [pid]);
    }

    // Only return alive processes
    processes.retain(|p| p["alive"].as_bool().unwrap_or(false));

    Ok(processes)
}

/// Kill a background process by PID and remove it from the database.
#[tauri::command]
pub fn kill_background_process(
    pid: u32,
    db: State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    kill_background_process_by_pid(pid);

    // Wait briefly for process to die, then verify
    std::thread::sleep(std::time::Duration::from_millis(500));
    let still_alive = is_process_alive(pid);

    let conn = db.connection();
    let _ = conn.execute("DELETE FROM background_processes WHERE pid = ?1", [pid as i64]);

    if still_alive {
        Ok(serde_json::json!({"success": false, "message": "Process may not have been killed. It might require elevated permissions."}))
    } else {
        Ok(serde_json::json!({"success": true, "message": "Process killed"}))
    }
}
