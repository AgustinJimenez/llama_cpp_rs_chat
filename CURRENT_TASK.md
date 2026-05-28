# DB Schema Cleanup

## Findings

### 1. `conversations.system_prompt` — ACTIVE DUPLICATION ✅ FIXED
Both `conversations` and `conversation_config` stored `system_prompt`.

**Finding:** `conversations.system_prompt` was dead code — the engine exclusively reads from
`conversation_config.system_prompt`. The column was never read for inference, and the
`ConversationRecord` struct had `#[allow(dead_code)]` on it.

**Fix:**
- Removed `system_prompt` column from `CREATE TABLE conversations` in sql.rs
- Dropped `system_prompt` param from `create_conversation()` (all callers passed `None`)
- Removed `system_prompt` field from `ConversationRecord`
- Updated all SELECT queries (column index shift)
- Added `ALTER TABLE conversations DROP COLUMN system_prompt` migration for existing DBs
- **Status:** DONE

---

### 2. `agent_heartbeat` table — ORPHANED ✅ FIXED
Global single-row heartbeat table. Fully migrated to per-conversation `heartbeat_*` columns in `conversation_config`.
No SQL reads/writes found in any Rust code.

**Fix:** Remove from CREATE TABLE list in schema.rs + add DROP TABLE IF EXISTS migration for existing DBs.
**Status:** DONE

---

### 3. `message_queue` — MISSING INDEX ✅ FIXED
Queried by `conversation_id` (`WHERE conversation_id = ?1`) but no index on that column.

**Fix:** Added `CREATE INDEX idx_message_queue_conversation ON message_queue(conversation_id)`.
**Status:** DONE

---

### 4. `background_processes` — MISSING INDEX ✅ FIXED
Queried by `session_id` in three places but no index on that column.

**Fix:** Added `CREATE INDEX idx_background_processes_session ON background_processes(session_id)`.
**Status:** DONE

---

### 5. `hub_downloads.downloaded_at` — NAMING (low priority)
Column name is misleading — it's written at INSERT (start) AND updated at completion. It's effectively `updated_at`.
No functional impact. Rename would require a migration. Leave for now.

---

### 6. `config` vs `conversation_config` — STRUCTURAL DUPLICATION (accepted)
~25 sampling/hardware params duplicated across both tables (intentional: global defaults vs per-conversation overrides).
Adding a new param requires touching both tables + both Rust structs + ALTER TABLE migrations.

**Long-term option:** Replace per-param overrides with a `sampler_overrides TEXT` JSON blob in `conversation_config`, and only store per-param columns in `config`. Trade-off: loses column-level type safety and indexability.
**Status:** ACCEPTED AS-IS for now, document for future refactor consideration.
