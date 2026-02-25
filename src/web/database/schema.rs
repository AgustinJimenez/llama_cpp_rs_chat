// Database schema definitions for LLaMA Chat

use super::db_error;
use rusqlite::Connection;

/// SQL statements to create all tables
const CREATE_CONVERSATIONS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    system_prompt TEXT,
    title TEXT
)
"#;

const CREATE_MESSAGES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    sequence_order INTEGER NOT NULL,
    is_streaming INTEGER DEFAULT 0,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
)
"#;

const CREATE_MESSAGES_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_messages_conversation
ON messages(conversation_id, sequence_order)
"#;

const CREATE_STREAMING_BUFFER_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS streaming_buffer (
    conversation_id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    partial_content TEXT NOT NULL,
    tokens_used INTEGER DEFAULT 0,
    max_tokens INTEGER DEFAULT 0,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
)
"#;

const CREATE_CONFIG_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    sampler_type TEXT DEFAULT 'Greedy',
    temperature REAL DEFAULT 0.7,
    top_p REAL DEFAULT 0.95,
    top_k INTEGER DEFAULT 20,
    mirostat_tau REAL DEFAULT 5.0,
    mirostat_eta REAL DEFAULT 0.1,
    repeat_penalty REAL DEFAULT 1.0,
    min_p REAL DEFAULT 0.0,
    model_path TEXT,
    system_prompt TEXT,
    system_prompt_type TEXT DEFAULT 'Default',
    context_size INTEGER DEFAULT 32768,
    stop_tokens TEXT,
    updated_at INTEGER NOT NULL
)
"#;

const CREATE_MODEL_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS model_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_path TEXT UNIQUE NOT NULL,
    last_used INTEGER NOT NULL,
    display_order INTEGER NOT NULL
)
"#;

const CREATE_CONVERSATION_CONFIG_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS conversation_config (
    conversation_id TEXT PRIMARY KEY,
    sampler_type TEXT DEFAULT 'Greedy',
    temperature REAL DEFAULT 0.7,
    top_p REAL DEFAULT 0.95,
    top_k INTEGER DEFAULT 20,
    mirostat_tau REAL DEFAULT 5.0,
    mirostat_eta REAL DEFAULT 0.1,
    repeat_penalty REAL DEFAULT 1.0,
    min_p REAL DEFAULT 0.0,
    typical_p REAL DEFAULT 1.0,
    frequency_penalty REAL DEFAULT 0.0,
    presence_penalty REAL DEFAULT 0.0,
    penalty_last_n INTEGER DEFAULT 64,
    dry_multiplier REAL DEFAULT 0.0,
    dry_base REAL DEFAULT 1.75,
    dry_allowed_length INTEGER DEFAULT 2,
    dry_penalty_last_n INTEGER DEFAULT -1,
    top_n_sigma REAL DEFAULT -1.0,
    flash_attention INTEGER DEFAULT 0,
    cache_type_k TEXT DEFAULT 'f16',
    cache_type_v TEXT DEFAULT 'f16',
    n_batch INTEGER DEFAULT 2048,
    context_size INTEGER DEFAULT 32768,
    system_prompt TEXT,
    system_prompt_type TEXT DEFAULT 'Default',
    stop_tokens TEXT,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
)
"#;

const CREATE_HUB_DOWNLOADS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS hub_downloads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    dest_path TEXT NOT NULL,
    file_size INTEGER NOT NULL DEFAULT 0,
    bytes_downloaded INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    etag TEXT,
    downloaded_at INTEGER NOT NULL,
    UNIQUE(model_id, filename, dest_path)
)
"#;

const CREATE_LOGS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE SET NULL
)
"#;

const CREATE_LOGS_CONVERSATION_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_logs_conversation
ON logs(conversation_id, timestamp)
"#;

const CREATE_LOGS_TIMESTAMP_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_logs_timestamp
ON logs(timestamp)
"#;

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
        ("logs", CREATE_LOGS_TABLE),
        ("logs_conversation_index", CREATE_LOGS_CONVERSATION_INDEX),
        ("logs_timestamp_index", CREATE_LOGS_TIMESTAMP_INDEX),
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

    // Add web search provider column if missing
    let _ = conn.execute(
        "ALTER TABLE config ADD COLUMN web_search_provider TEXT DEFAULT 'DuckDuckGo'",
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

    // Insert default config row if it doesn't exist
    conn.execute(
        "INSERT OR IGNORE INTO config (id, updated_at) VALUES (1, ?1)",
        [super::current_timestamp_millis()],
    )
    .map_err(db_error("insert default config"))?;

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
