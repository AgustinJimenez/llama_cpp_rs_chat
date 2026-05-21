pub(super) const CREATE_CONVERSATIONS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    system_prompt TEXT,
    title TEXT,
    worker_id TEXT,
    provider_id TEXT,
    provider_session_id TEXT
)
"#;

pub(super) const CREATE_MESSAGES_TABLE: &str = r#"
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

pub(super) const CREATE_MESSAGES_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_messages_conversation
ON messages(conversation_id, sequence_order)
"#;

pub(super) const CREATE_STREAMING_BUFFER_TABLE: &str = r#"
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

pub(super) const CREATE_CONFIG_TABLE: &str = r#"
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

pub(super) const CREATE_MODEL_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS model_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_path TEXT UNIQUE NOT NULL,
    last_used INTEGER NOT NULL,
    display_order INTEGER NOT NULL
)
"#;

pub(super) const CREATE_CONVERSATION_CONFIG_TABLE: &str = r#"
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
    flash_attention INTEGER DEFAULT 1,
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

pub(super) const CREATE_HUB_DOWNLOADS_TABLE: &str = r#"
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

pub(super) const CREATE_MCP_SERVERS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS mcp_servers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    transport TEXT NOT NULL,
    command TEXT,
    args TEXT,
    env_vars TEXT,
    url TEXT,
    enabled INTEGER DEFAULT 1,
    created_at INTEGER,
    updated_at INTEGER
)
"#;

pub(super) const CREATE_BACKGROUND_PROCESSES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS background_processes (
    pid INTEGER PRIMARY KEY,
    command TEXT NOT NULL,
    conversation_id TEXT,
    started_at INTEGER NOT NULL,
    session_id TEXT NOT NULL
)
"#;

pub(super) const CREATE_CONVERSATION_CONTEXT_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS conversation_context (
    conversation_id TEXT PRIMARY KEY,
    system_prompt_text TEXT,
    system_prompt_tokens INTEGER DEFAULT 0,
    tool_definitions_json TEXT,
    tool_definitions_tokens INTEGER DEFAULT 0,
    content_hash TEXT,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
)
"#;

pub(super) const CREATE_LOGS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE SET NULL
)
"#;

pub(super) const CREATE_LOGS_CONVERSATION_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_logs_conversation
ON logs(conversation_id, timestamp)
"#;

pub(super) const CREATE_LOGS_TIMESTAMP_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_logs_timestamp
ON logs(timestamp)
"#;

pub(super) const CREATE_APP_ERRORS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS app_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    level TEXT NOT NULL,
    source TEXT NOT NULL,
    message TEXT NOT NULL,
    details TEXT,
    timestamp INTEGER NOT NULL
)
"#;

pub(super) const CREATE_APP_ERRORS_TIMESTAMP_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_app_errors_timestamp
ON app_errors(timestamp)
"#;

pub(super) const CREATE_MESSAGE_QUEUE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS message_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
)
"#;

pub(super) const CREATE_COMPACTION_SUMMARIES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS compaction_summaries (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    covers_from_sequence INTEGER NOT NULL,
    covers_to_sequence INTEGER NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 0,
    summary_text TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
)
"#;

pub(super) const CREATE_COMPACTION_SUMMARIES_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_compaction_summaries_conversation
ON compaction_summaries(conversation_id, covers_to_sequence)
"#;

pub(super) const CREATE_AGENT_HEARTBEAT_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS agent_heartbeat (
    id INTEGER PRIMARY KEY DEFAULT 1,
    enabled INTEGER NOT NULL DEFAULT 0,
    interval_minutes INTEGER NOT NULL DEFAULT 5,
    prompt TEXT NOT NULL DEFAULT '',
    conversation_id TEXT,
    last_fired_at INTEGER NOT NULL DEFAULT 0,
    last_result TEXT,
    has_unread INTEGER NOT NULL DEFAULT 0
)
"#;
