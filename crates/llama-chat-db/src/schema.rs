// Database schema definitions for LLaMA Chat

use super::db_error;
use rusqlite::Connection;
#[path = "schema/sql.rs"]
mod sql;
use sql::*;

/// Initialize the database schema (create all tables and indexes)
pub fn initialize(conn: &Connection) -> Result<(), String> {
    // Create tables in order (respecting foreign key constraints)
    let statements = [
        ("conversations", CREATE_CONVERSATIONS_TABLE),
        ("messages", CREATE_MESSAGES_TABLE),
        ("messages_index", CREATE_MESSAGES_INDEX),
        ("streaming_buffer", CREATE_STREAMING_BUFFER_TABLE),
        ("config", CREATE_CONFIG_TABLE),
        ("conversation_config", CREATE_CONVERSATION_CONFIG_TABLE),
        ("model_history", CREATE_MODEL_HISTORY_TABLE),
        ("hub_downloads", CREATE_HUB_DOWNLOADS_TABLE),
        ("mcp_servers", CREATE_MCP_SERVERS_TABLE),
        ("background_processes", CREATE_BACKGROUND_PROCESSES_TABLE),
        ("conversation_context", CREATE_CONVERSATION_CONTEXT_TABLE),
        ("logs", CREATE_LOGS_TABLE),
        ("logs_conversation_index", CREATE_LOGS_CONVERSATION_INDEX),
        ("logs_timestamp_index", CREATE_LOGS_TIMESTAMP_INDEX),
        ("app_errors", CREATE_APP_ERRORS_TABLE),
        ("app_errors_timestamp_index", CREATE_APP_ERRORS_TIMESTAMP_INDEX),
        ("message_queue", CREATE_MESSAGE_QUEUE_TABLE),
        ("compaction_summaries", CREATE_COMPACTION_SUMMARIES_TABLE),
        ("compaction_summaries_index", CREATE_COMPACTION_SUMMARIES_INDEX),
        ("agent_heartbeat", CREATE_AGENT_HEARTBEAT_TABLE),
    ];

    for (name, sql) in statements.iter() {
        conn.execute(sql, [])
            .map_err(db_error(&format!("create {name}")))?;
    }

    // Add system_prompt_type column if missing (schema migration for existing DBs)
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN system_prompt_type TEXT DEFAULT 'Default'",
        [],
    );

    // Add disable_file_logging column if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN disable_file_logging INTEGER DEFAULT 1",
        [],
    );

    // Add repeat_penalty column if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN repeat_penalty REAL DEFAULT 1.0",
        [],
    );

    // Add min_p column if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN min_p REAL DEFAULT 0.0",
        [],
    );

    // Add advanced context params columns if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN flash_attention INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN cache_type_k TEXT DEFAULT 'f16'",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN cache_type_v TEXT DEFAULT 'f16'",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN n_batch INTEGER DEFAULT 2048",
        [],
    );

    // Add extended sampling parameter columns if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN typical_p REAL DEFAULT 1.0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN frequency_penalty REAL DEFAULT 0.0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN presence_penalty REAL DEFAULT 0.0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN penalty_last_n INTEGER DEFAULT 64",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN dry_multiplier REAL DEFAULT 0.0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN dry_base REAL DEFAULT 1.75",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN dry_allowed_length INTEGER DEFAULT 2",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN dry_penalty_last_n INTEGER DEFAULT -1",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN top_n_sigma REAL DEFAULT -1.0",
        [],
    );

    // Add tool tag override columns if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN tool_tag_exec_open TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN tool_tag_exec_close TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN tool_tag_output_open TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN tool_tag_output_close TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN tool_tag_exec_open TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN tool_tag_exec_close TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN tool_tag_output_open TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN tool_tag_output_close TEXT",
        [],
    );

    // Persist provider-side conversation/session handles (e.g. Claude Code --resume)
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN provider_session_id TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN provider_id TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN worker_id TEXT",
        [],
    );

    // Add resume-tracking columns to hub_downloads if missing
    let _ = conn.execute(
        "ALTER TABLE hub_downloads ADD COLUMN bytes_downloaded INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE hub_downloads ADD COLUMN status TEXT NOT NULL DEFAULT 'completed'",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE hub_downloads ADD COLUMN etag TEXT",
        [],
    );

    // Add hardware / context / sampler params columns if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN seed INTEGER DEFAULT -1",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN n_ubatch INTEGER DEFAULT 512",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN n_threads INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN n_threads_batch INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN rope_freq_base REAL DEFAULT 0.0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN rope_freq_scale REAL DEFAULT 0.0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN use_mlock INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN use_mmap INTEGER DEFAULT 1",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN main_gpu INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN split_mode TEXT DEFAULT 'layer'",
        [],
    );

    // RTK (Rust Token Killer) output compression
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN use_rtk INTEGER DEFAULT 1",
        [],
    );

    // htmd web fetch (better markdown extraction)
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN use_htmd INTEGER DEFAULT 0",
        [],
    );

    // Browser backend for web_fetch / web_search
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN web_browser_backend TEXT DEFAULT 'chrome'",
        [],
    );

    // Models directory for hub downloads
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN models_directory TEXT",
        [],
    );

    // Add tag_pairs JSON column if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN tag_pairs TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN tag_pairs TEXT",
        [],
    );

    // Per-conversation heartbeat columns (migrated from global agent_heartbeat table)
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_enabled INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_interval_minutes INTEGER DEFAULT 30",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_prompt TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_last_fired_at INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_last_result TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_has_unread INTEGER DEFAULT 0",
        [],
    );

    // Same columns for conversation_config
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN seed INTEGER DEFAULT -1",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN n_ubatch INTEGER DEFAULT 512",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN n_threads INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN n_threads_batch INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN rope_freq_base REAL DEFAULT 0.0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN rope_freq_scale REAL DEFAULT 0.0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN use_mlock INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN use_mmap INTEGER DEFAULT 1",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN main_gpu INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversation_config ADD COLUMN split_mode TEXT DEFAULT 'layer'",
        [],
    );

    // Add timing columns to messages table for per-message generation stats
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN prompt_tok_per_sec REAL",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN gen_tok_per_sec REAL",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN gen_eval_ms REAL",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN gen_tokens INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN prompt_eval_ms REAL",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN prompt_tokens INTEGER",
        [],
    );

    // Compaction column (legacy): was used to mark summarized messages, now derived from
    // compaction_summaries ranges. Keep ADD for old DBs, then immediately drop it.
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN compacted INTEGER DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE messages DROP COLUMN compacted", []);

    // Per-message token count cache for accurate context budgeting
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN token_count INTEGER",
        [],
    );

    // Insert default config row if it doesn't exist
    // Add proactive_compaction column if missing, then enable it on existing rows
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN proactive_compaction INTEGER DEFAULT 1",
        [],
    );
    let _ = conn.execute(
        "UPDATE config SET proactive_compaction = 1 WHERE proactive_compaction = 0 OR proactive_compaction IS NULL",
        [],
    );

    // Add safe_tool_injection column if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN safe_tool_injection INTEGER DEFAULT 0",
        [],
    );

    // Add Telegram notification settings columns if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN telegram_bot_token TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN telegram_chat_id TEXT",
        [],
    );

    // Provider API keys (JSON blob: {"groq": {"api_key": "..."}, "gemini": {"api_key": "...", "base_url": "..."}, ...})
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN provider_api_keys TEXT",
        [],
    );

    // Max tool calls per remote provider turn (safety limit)
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN max_tool_calls INTEGER DEFAULT 2000",
        [],
    );
    // Loop detection: max consecutive identical tool calls before stopping
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN loop_detection_limit INTEGER DEFAULT 15",
        [],
    );

    // Thinking mode: NULL=use model default, 0=disabled, 1=enabled
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN thinking_mode INTEGER DEFAULT NULL",
        [],
    );

    // Active provider preference (persisted so API clients can query it)
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN active_provider TEXT DEFAULT 'local'",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN active_provider_model TEXT",
        [],
    );

    // Drop deprecated columns (web search was never implemented; requires SQLite 3.35+)
    // Errors are ignored — column may already be absent on fresh DBs
    let _ = conn.execute("ALTER TABLE config DROP COLUMN web_search_provider", []);
    let _ = conn.execute("ALTER TABLE config DROP COLUMN web_search_api_key", []);

    conn.execute(
        "INSERT OR IGNORE INTO config (id, updated_at) VALUES (1, ?1)",
        [super::current_timestamp_millis()],
    )
    .map_err(db_error("insert default config"))?;

    // Migrate old-style compaction summaries from messages table to compaction_summaries table.
    // Old design stored summaries as role='system' messages with content starting with
    // '[Conversation summary'. New design uses a dedicated table.
    let _ = conn.execute(
        r#"INSERT OR IGNORE INTO compaction_summaries (id, conversation_id, covers_from_sequence, covers_to_sequence, message_count, summary_text, created_at)
           SELECT
               m.id,
               m.conversation_id,
               1 AS covers_from_sequence,
               m.sequence_order - 1 AS covers_to_sequence,
               0 AS message_count,
               CASE
                   WHEN instr(m.content, char(10)) > 0
                   THEN substr(m.content, instr(m.content, char(10)) + 1)
                   ELSE ''
               END AS summary_text,
               m.timestamp AS created_at
           FROM messages m
           WHERE m.role = 'system' AND m.content LIKE '[Conversation summary%'"#,
        [],
    );

    // Remove the now-migrated summary messages from the messages table.
    let _ = conn.execute(
        "DELETE FROM messages WHERE role = 'system' AND content LIKE '[Conversation summary%'",
        [],
    );

    Ok(())
}

/// Drop all tables (for testing/reset)
#[allow(dead_code)]
pub fn drop_all_tables(conn: &Connection) -> Result<(), String> {
    let tables = [
        "streaming_buffer",
        "messages",
        "conversation_config",
        "logs",
        "model_history",
        "config",
        "conversations",
    ];

    for table in tables.iter() {
        conn.execute(&format!("DROP TABLE IF EXISTS {table}"), [])
            .map_err(db_error(&format!("drop {table}")))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();

        let result = initialize(&conn);
        assert!(result.is_ok(), "Schema initialization failed: {result:?}");

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"conversations".to_string()));
        assert!(tables.contains(&"messages".to_string()));
        assert!(tables.contains(&"config".to_string()));
        assert!(tables.contains(&"conversation_config".to_string()));
        assert!(tables.contains(&"logs".to_string()));
        assert!(tables.contains(&"model_history".to_string()));
        assert!(tables.contains(&"streaming_buffer".to_string()));
    }

    #[test]
    fn test_default_config_inserted() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        initialize(&conn).unwrap();

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM config", [], |row| row.get(0))
            .unwrap();

        assert_eq!(count, 1);
    }
}
