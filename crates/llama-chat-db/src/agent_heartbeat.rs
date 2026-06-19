// Per-conversation agent heartbeat configuration.
// Heartbeat settings are stored as columns on the conversations table.

use crate::{current_timestamp_secs, db_error, SharedDatabase};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// Whether the heartbeat loop is active for this conversation.
    pub enabled: bool,
    /// How often to fire (minutes).
    pub interval_minutes: u32,
    /// The prompt sent to the model each heartbeat turn.
    pub prompt: String,
    /// Unix timestamp of last firing (0 = never).
    pub last_fired_at: u64,
    /// Last model response text (None = never fired or last was IDLE).
    pub last_result: Option<String>,
    /// Whether the last result was non-idle (badge indicator for the UI).
    pub has_unread: bool,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 30,
            prompt: DEFAULT_HEARTBEAT_PROMPT.to_string(),
            last_fired_at: 0,
            last_result: None,
            has_unread: false,
        }
    }
}

pub const DEFAULT_HEARTBEAT_PROMPT: &str =
    "You are running a background heartbeat check. Review the conversation so far \
     and any ongoing tasks or items you were working on. If something needs \
     the user's attention, report it concisely. \
     If nothing requires attention, respond with exactly: IDLE";

/// Read heartbeat config for a conversation. Returns defaults if no row exists.
pub fn read_heartbeat_config(db: &SharedDatabase, conversation_id: &str) -> HeartbeatConfig {
    let conn = db.connection();
    conn.query_row(
        "SELECT heartbeat_enabled, heartbeat_interval_minutes, heartbeat_prompt,
                heartbeat_last_fired_at, heartbeat_last_result, heartbeat_has_unread
         FROM conversations WHERE id = ?1",
        [conversation_id],
        |row| {
            Ok(HeartbeatConfig {
                enabled: row.get::<_, i32>(0)? != 0,
                interval_minutes: row.get::<_, u32>(1)?,
                prompt: row
                    .get::<_, Option<String>>(2)?
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| DEFAULT_HEARTBEAT_PROMPT.to_string()),
                last_fired_at: row.get::<_, i64>(3)? as u64,
                last_result: row.get(4)?,
                has_unread: row.get::<_, i32>(5)? != 0,
            })
        },
    )
    .unwrap_or_default()
}

/// Persist heartbeat config for a conversation.
pub fn write_heartbeat_config(
    db: &SharedDatabase,
    conversation_id: &str,
    cfg: &HeartbeatConfig,
) -> Result<(), String> {
    let conn = db.connection();
    conn.execute(
        "UPDATE conversations SET
           heartbeat_enabled = ?1,
           heartbeat_interval_minutes = ?2,
           heartbeat_prompt = ?3,
           heartbeat_last_fired_at = ?4,
           heartbeat_last_result = ?5,
           heartbeat_has_unread = ?6,
           updated_at = ?7
         WHERE id = ?8",
        rusqlite::params![
            cfg.enabled as i32,
            cfg.interval_minutes,
            cfg.prompt,
            cfg.last_fired_at as i64,
            cfg.last_result,
            cfg.has_unread as i32,
            crate::current_timestamp_millis(),
            conversation_id,
        ],
    )
    .map_err(db_error("write heartbeat config"))?;
    Ok(())
}

/// Returns all conversations that have heartbeat enabled, with their config.
pub fn list_enabled_heartbeats(db: &SharedDatabase) -> Vec<(String, HeartbeatConfig)> {
    let conn = db.connection();
    let mut stmt = match conn.prepare(
        "SELECT id, heartbeat_interval_minutes, heartbeat_prompt,
                heartbeat_last_fired_at, heartbeat_last_result, heartbeat_has_unread
         FROM conversations WHERE heartbeat_enabled = 1",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |row| {
        let conv_id: String = row.get(0)?;
        let cfg = HeartbeatConfig {
            enabled: true,
            interval_minutes: row.get::<_, u32>(1)?,
            prompt: row
                .get::<_, Option<String>>(2)?
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| DEFAULT_HEARTBEAT_PROMPT.to_string()),
            last_fired_at: row.get::<_, i64>(3)? as u64,
            last_result: row.get(4)?,
            has_unread: row.get::<_, i32>(5)? != 0,
        };
        Ok((conv_id, cfg))
    })
    .ok()
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// Record a heartbeat firing result. Updates last_fired_at, last_result, has_unread.
pub fn record_heartbeat_result(
    db: &SharedDatabase,
    conversation_id: &str,
    result: Option<&str>,
) -> Result<(), String> {
    let now = current_timestamp_secs() as i64;
    let has_unread = result.is_some();
    let conn = db.connection();
    conn.execute(
        "UPDATE conversations SET
            heartbeat_last_fired_at = ?1,
            heartbeat_last_result   = ?2,
            heartbeat_has_unread    = ?3
         WHERE id = ?4",
        rusqlite::params![now, result, has_unread as i32, conversation_id],
    )
    .map_err(db_error("record heartbeat result"))?;
    Ok(())
}

/// Clear the unread badge (called when user opens the heartbeat modal).
pub fn clear_heartbeat_unread(db: &SharedDatabase, conversation_id: &str) {
    let conn = db.connection();
    let _ = conn.execute(
        "UPDATE conversations SET heartbeat_has_unread = 0
         WHERE id = ?1",
        [conversation_id],
    );
}
