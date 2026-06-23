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
        ("schema_migrations", CREATE_SCHEMA_MIGRATIONS_TABLE),
        ("conversations", CREATE_CONVERSATIONS_TABLE),
        ("messages", CREATE_MESSAGES_TABLE),
        ("messages_index", CREATE_MESSAGES_INDEX),
        ("streaming_buffer", CREATE_STREAMING_BUFFER_TABLE),
        ("config", CREATE_CONFIG_TABLE),
        ("model_history", CREATE_MODEL_HISTORY_TABLE),
        ("hub_downloads", CREATE_HUB_DOWNLOADS_TABLE),
        ("mcp_servers", CREATE_MCP_SERVERS_TABLE),
        ("background_processes", CREATE_BACKGROUND_PROCESSES_TABLE),
        ("conversation_context", CREATE_CONVERSATION_CONTEXT_TABLE),
        ("logs", CREATE_LOGS_TABLE),
        ("logs_conversation_index", CREATE_LOGS_CONVERSATION_INDEX),
        ("logs_timestamp_index", CREATE_LOGS_TIMESTAMP_INDEX),
        ("app_errors", CREATE_APP_ERRORS_TABLE),
        (
            "app_errors_timestamp_index",
            CREATE_APP_ERRORS_TIMESTAMP_INDEX,
        ),
        ("message_queue", CREATE_MESSAGE_QUEUE_TABLE),
        (
            "message_queue_conversation_index",
            CREATE_MESSAGE_QUEUE_CONVERSATION_INDEX,
        ),
        ("compaction_summaries", CREATE_COMPACTION_SUMMARIES_TABLE),
        (
            "compaction_summaries_index",
            CREATE_COMPACTION_SUMMARIES_INDEX,
        ),
        ("agents", CREATE_AGENTS_TABLE),
    ];

    for (name, sql) in statements.iter() {
        conn.execute(sql, [])
            .map_err(db_error(&format!("create {name}")))?;
    }

    // Add disable_file_logging column if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN disable_file_logging INTEGER DEFAULT 1",
        [],
    );

    // Persist provider-side conversation/session handles (e.g. Claude Code --resume)
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN provider_session_id TEXT",
        [],
    );
    let _ = conn.execute("ALTER TABLE conversations ADD COLUMN provider_id TEXT", []);
    let _ = conn.execute("ALTER TABLE conversations ADD COLUMN worker_id TEXT", []);

    // Agent-based config: conversations reference an agent by ID.
    // overrides = sparse JSON of per-conversation param deltas from the agent baseline.
    let _ = conn.execute("ALTER TABLE conversations ADD COLUMN agent_id TEXT", []);
    let _ = conn.execute("ALTER TABLE conversations ADD COLUMN overrides TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN heartbeat_enabled INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN heartbeat_interval_minutes INTEGER DEFAULT 30",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN heartbeat_prompt TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN heartbeat_last_fired_at INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN heartbeat_last_result TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE conversations ADD COLUMN heartbeat_has_unread INTEGER DEFAULT 0",
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
    let _ = conn.execute("ALTER TABLE hub_downloads ADD COLUMN etag TEXT", []);

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
    let _ = conn.execute("ALTER TABLE config ADD COLUMN models_directory TEXT", []);

    // Add timing columns to messages table for per-message generation stats
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN prompt_tok_per_sec REAL",
        [],
    );
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN gen_tok_per_sec REAL", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN gen_eval_ms REAL", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN gen_tokens INTEGER", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN prompt_eval_ms REAL", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN prompt_tokens INTEGER", []);

    // Compaction column (legacy): was used to mark summarized messages, now derived from
    // compaction_summaries ranges. Keep ADD for old DBs, then immediately drop it.
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN compacted INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute("ALTER TABLE messages DROP COLUMN compacted", []);

    // Per-message token count cache for accurate context budgeting
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN token_count INTEGER", []);

    // Structured message parts (JSON array of {type, content, tool_name?, tool_args?})
    // Written by the remote provider loop; null for local-model messages until backfill.
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN parts TEXT", []);

    // Add Telegram notification settings columns if missing
    let _ = conn.execute("ALTER TABLE config ADD COLUMN telegram_bot_token TEXT", []);
    let _ = conn.execute("ALTER TABLE config ADD COLUMN telegram_chat_id TEXT", []);

    // Provider API keys (JSON blob: {"groq": {"api_key": "..."}, "gemini": {"api_key": "...", "base_url": "..."}, ...})
    let _ = conn.execute("ALTER TABLE config ADD COLUMN provider_api_keys TEXT", []);

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

    // Active provider preference (persisted so API clients can query it)
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN active_provider TEXT DEFAULT 'local'",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN active_provider_model TEXT",
        [],
    );

    // Drop old per-conversation prompt column; effective prompts now resolve from agents plus
    // sparse conversation overrides. Requires SQLite 3.35+; errors ignored.
    let _ = conn.execute("ALTER TABLE conversations DROP COLUMN system_prompt", []);

    // Drop orphaned agent_heartbeat table (migrated to per-conversation heartbeat_* columns
    // on conversations). Errors ignored — table may already be absent on fresh DBs.
    let _ = conn.execute("DROP TABLE IF EXISTS agent_heartbeat", []);

    // Add session index to background_processes if missing (for startup cleanup queries)
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_background_processes_session ON background_processes(session_id)",
        [],
    );

    // Drop deprecated columns (web search was never implemented; requires SQLite 3.35+)
    // Errors are ignored — column may already be absent on fresh DBs
    let _ = conn.execute("ALTER TABLE config DROP COLUMN web_search_provider", []);
    let _ = conn.execute("ALTER TABLE config DROP COLUMN web_search_api_key", []);

    // Remote access token (generated once, used for Bearer auth from non-localhost clients)
    let _ = conn.execute("ALTER TABLE config ADD COLUMN remote_access_token TEXT", []);

    // Per-message LLM-generated title (≤50 chars, user messages only, set by background title gen)
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN title TEXT", []);

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

    migrate_conversation_config_to_agents(conn)?;
    strip_global_config_model_columns(conn);

    Ok(())
}

fn migration_applied(conn: &Connection, name: &str) -> Result<bool, String> {
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .map_err(db_error("check schema migration"))?;
    Ok(count > 0)
}

fn table_exists(conn: &Connection, table_name: &str) -> Result<bool, String> {
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table_name],
            |row| row.get(0),
        )
        .map_err(db_error("check table existence"))?;
    Ok(count > 0)
}

fn mark_migration_applied(conn: &Connection, name: &str) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (name, applied_at) VALUES (?1, ?2)",
        rusqlite::params![name, super::current_timestamp_millis()],
    )
    .map_err(db_error("mark schema migration"))?;
    Ok(())
}

fn migrate_conversation_config_to_agents(conn: &Connection) -> Result<(), String> {
    const MIGRATION_NAME: &str = "conversation_config_to_agents_v1";
    if migration_applied(conn, MIGRATION_NAME)? {
        return Ok(());
    }
    if !table_exists(conn, "conversation_config")? {
        return mark_migration_applied(conn, MIGRATION_NAME);
    }

    ensure_legacy_conversation_config_columns(conn);
    let _ = conn.execute("ALTER TABLE config ADD COLUMN model_path TEXT", []);

    conn.execute(
        r#"INSERT OR IGNORE INTO agents (
            id, name, provider_id, model_path, provider_model,
            system_prompt, system_prompt_type,
            sampler_type, temperature, top_p, top_k, mirostat_tau, mirostat_eta,
            repeat_penalty, min_p, typical_p, frequency_penalty, presence_penalty,
            penalty_last_n, dry_multiplier, dry_base, dry_allowed_length, dry_penalty_last_n,
            top_n_sigma, flash_attention, cache_type_k, cache_type_v, n_batch, context_size,
            seed, n_ubatch, n_threads, n_threads_batch, rope_freq_base, rope_freq_scale,
            use_mlock, use_mmap, main_gpu, split_mode,
            stop_tokens, tag_pairs,
            tool_tag_exec_open, tool_tag_exec_close, tool_tag_output_open, tool_tag_output_close,
            proactive_compaction, safe_tool_injection, thinking_mode,
            heartbeat_enabled, heartbeat_interval_minutes, heartbeat_prompt,
            created_at, updated_at
        )
        SELECT
            'agent_imported_' || cc.conversation_id,
            'Imported Agent - ' || COALESCE(NULLIF(c.title, ''), cc.conversation_id),
            'local',
            (SELECT model_path FROM config WHERE id = 1),
            NULL,
            cc.system_prompt,
            COALESCE(cc.system_prompt_type, 'Custom'),
            COALESCE(cc.sampler_type, 'Greedy'),
            COALESCE(cc.temperature, 0.7),
            COALESCE(cc.top_p, 0.95),
            COALESCE(cc.top_k, 20),
            COALESCE(cc.mirostat_tau, 5.0),
            COALESCE(cc.mirostat_eta, 0.1),
            COALESCE(cc.repeat_penalty, 1.0),
            COALESCE(cc.min_p, 0.0),
            COALESCE(cc.typical_p, 1.0),
            COALESCE(cc.frequency_penalty, 0.0),
            COALESCE(cc.presence_penalty, 0.0),
            COALESCE(cc.penalty_last_n, 64),
            COALESCE(cc.dry_multiplier, 0.0),
            COALESCE(cc.dry_base, 1.75),
            COALESCE(cc.dry_allowed_length, 2),
            COALESCE(cc.dry_penalty_last_n, -1),
            COALESCE(cc.top_n_sigma, -1.0),
            COALESCE(cc.flash_attention, 1),
            COALESCE(cc.cache_type_k, 'f16'),
            COALESCE(cc.cache_type_v, 'f16'),
            COALESCE(cc.n_batch, 2048),
            cc.context_size,
            COALESCE(cc.seed, -1),
            COALESCE(cc.n_ubatch, 512),
            COALESCE(cc.n_threads, 0),
            COALESCE(cc.n_threads_batch, 0),
            COALESCE(cc.rope_freq_base, 0.0),
            COALESCE(cc.rope_freq_scale, 0.0),
            COALESCE(cc.use_mlock, 0),
            COALESCE(cc.use_mmap, 1),
            COALESCE(cc.main_gpu, 0),
            COALESCE(cc.split_mode, 'layer'),
            cc.stop_tokens,
            cc.tag_pairs,
            cc.tool_tag_exec_open,
            cc.tool_tag_exec_close,
            cc.tool_tag_output_open,
            cc.tool_tag_output_close,
            1,
            0,
            NULL,
            COALESCE(cc.heartbeat_enabled, 0),
            COALESCE(cc.heartbeat_interval_minutes, 30),
            cc.heartbeat_prompt,
            COALESCE(cc.updated_at, c.created_at),
            COALESCE(cc.updated_at, c.updated_at)
        FROM conversation_config cc
        JOIN conversations c ON c.id = cc.conversation_id
        WHERE c.agent_id IS NULL"#,
        [],
    )
    .map_err(db_error("migrate conversation configs to agents"))?;

    conn.execute(
        r#"UPDATE conversations
           SET agent_id = 'agent_imported_' || id,
               heartbeat_enabled = COALESCE((
                   SELECT heartbeat_enabled FROM conversation_config WHERE conversation_id = conversations.id
               ), heartbeat_enabled),
               heartbeat_interval_minutes = COALESCE((
                   SELECT heartbeat_interval_minutes FROM conversation_config WHERE conversation_id = conversations.id
               ), heartbeat_interval_minutes),
               heartbeat_prompt = COALESCE((
                   SELECT heartbeat_prompt FROM conversation_config WHERE conversation_id = conversations.id
               ), heartbeat_prompt),
               heartbeat_last_fired_at = COALESCE((
                   SELECT heartbeat_last_fired_at FROM conversation_config WHERE conversation_id = conversations.id
               ), heartbeat_last_fired_at),
               heartbeat_last_result = COALESCE((
                   SELECT heartbeat_last_result FROM conversation_config WHERE conversation_id = conversations.id
               ), heartbeat_last_result),
               heartbeat_has_unread = COALESCE((
                   SELECT heartbeat_has_unread FROM conversation_config WHERE conversation_id = conversations.id
               ), heartbeat_has_unread)
         WHERE agent_id IS NULL
           AND EXISTS (SELECT 1 FROM conversation_config WHERE conversation_id = conversations.id)"#,
        [],
    )
    .map_err(db_error("assign imported agents to conversations"))?;

    conn.execute("DROP TABLE IF EXISTS conversation_config", [])
        .map_err(db_error("drop migrated conversation_config"))?;

    mark_migration_applied(conn, MIGRATION_NAME)
}

fn ensure_legacy_conversation_config_columns(conn: &Connection) {
    let legacy_columns = [
        "ALTER TABLE conversation_config ADD COLUMN typical_p REAL DEFAULT 1.0",
        "ALTER TABLE conversation_config ADD COLUMN frequency_penalty REAL DEFAULT 0.0",
        "ALTER TABLE conversation_config ADD COLUMN presence_penalty REAL DEFAULT 0.0",
        "ALTER TABLE conversation_config ADD COLUMN penalty_last_n INTEGER DEFAULT 64",
        "ALTER TABLE conversation_config ADD COLUMN dry_multiplier REAL DEFAULT 0.0",
        "ALTER TABLE conversation_config ADD COLUMN dry_base REAL DEFAULT 1.75",
        "ALTER TABLE conversation_config ADD COLUMN dry_allowed_length INTEGER DEFAULT 2",
        "ALTER TABLE conversation_config ADD COLUMN dry_penalty_last_n INTEGER DEFAULT -1",
        "ALTER TABLE conversation_config ADD COLUMN top_n_sigma REAL DEFAULT -1.0",
        "ALTER TABLE conversation_config ADD COLUMN flash_attention INTEGER DEFAULT 1",
        "ALTER TABLE conversation_config ADD COLUMN cache_type_k TEXT DEFAULT 'f16'",
        "ALTER TABLE conversation_config ADD COLUMN cache_type_v TEXT DEFAULT 'f16'",
        "ALTER TABLE conversation_config ADD COLUMN n_batch INTEGER DEFAULT 2048",
        "ALTER TABLE conversation_config ADD COLUMN tool_tag_exec_open TEXT",
        "ALTER TABLE conversation_config ADD COLUMN tool_tag_exec_close TEXT",
        "ALTER TABLE conversation_config ADD COLUMN tool_tag_output_open TEXT",
        "ALTER TABLE conversation_config ADD COLUMN tool_tag_output_close TEXT",
        "ALTER TABLE conversation_config ADD COLUMN tag_pairs TEXT",
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_enabled INTEGER DEFAULT 0",
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_interval_minutes INTEGER DEFAULT 30",
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_prompt TEXT",
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_last_fired_at INTEGER DEFAULT 0",
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_last_result TEXT",
        "ALTER TABLE conversation_config ADD COLUMN heartbeat_has_unread INTEGER DEFAULT 0",
        "ALTER TABLE conversation_config ADD COLUMN seed INTEGER DEFAULT -1",
        "ALTER TABLE conversation_config ADD COLUMN n_ubatch INTEGER DEFAULT 512",
        "ALTER TABLE conversation_config ADD COLUMN n_threads INTEGER DEFAULT 0",
        "ALTER TABLE conversation_config ADD COLUMN n_threads_batch INTEGER DEFAULT 0",
        "ALTER TABLE conversation_config ADD COLUMN rope_freq_base REAL DEFAULT 0.0",
        "ALTER TABLE conversation_config ADD COLUMN rope_freq_scale REAL DEFAULT 0.0",
        "ALTER TABLE conversation_config ADD COLUMN use_mlock INTEGER DEFAULT 0",
        "ALTER TABLE conversation_config ADD COLUMN use_mmap INTEGER DEFAULT 1",
        "ALTER TABLE conversation_config ADD COLUMN main_gpu INTEGER DEFAULT 0",
        "ALTER TABLE conversation_config ADD COLUMN split_mode TEXT DEFAULT 'layer'",
    ];

    for sql in legacy_columns {
        let _ = conn.execute(sql, []);
    }
}

fn strip_global_config_model_columns(conn: &Connection) {
    let model_columns = [
        "sampler_type",
        "temperature",
        "top_p",
        "top_k",
        "mirostat_tau",
        "mirostat_eta",
        "repeat_penalty",
        "min_p",
        "model_path",
        "system_prompt",
        "system_prompt_type",
        "context_size",
        "stop_tokens",
        "flash_attention",
        "cache_type_k",
        "cache_type_v",
        "n_batch",
        "typical_p",
        "frequency_penalty",
        "presence_penalty",
        "penalty_last_n",
        "dry_multiplier",
        "dry_base",
        "dry_allowed_length",
        "dry_penalty_last_n",
        "top_n_sigma",
        "tool_tag_exec_open",
        "tool_tag_exec_close",
        "tool_tag_output_open",
        "tool_tag_output_close",
        "seed",
        "n_ubatch",
        "n_threads",
        "n_threads_batch",
        "rope_freq_base",
        "rope_freq_scale",
        "use_mlock",
        "use_mmap",
        "main_gpu",
        "split_mode",
        "tag_pairs",
        "proactive_compaction",
        "safe_tool_injection",
        "thinking_mode",
    ];

    for column in model_columns {
        let _ = conn.execute(&format!("ALTER TABLE config DROP COLUMN {column}"), []);
    }
}

/// Drop all tables (for testing/reset)
#[allow(dead_code)]
pub fn drop_all_tables(conn: &Connection) -> Result<(), String> {
    let tables = [
        "streaming_buffer",
        "messages",
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

    #[test]
    fn test_config_table_is_app_level_only() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        initialize(&conn).unwrap();

        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(config)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(columns.contains(&"provider_api_keys".to_string()));
        assert!(columns.contains(&"models_directory".to_string()));
        assert!(!columns.contains(&"model_path".to_string()));
        assert!(!columns.contains(&"temperature".to_string()));
        assert!(!columns.contains(&"context_size".to_string()));
    }
}
