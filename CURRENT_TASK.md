# DB Schema Cleanup

## Findings

### 1. `conversations.system_prompt` — ACTIVE DUPLICATION
Both `conversations` and `conversation_config` store `system_prompt` + `system_prompt_type`.

**Current state:**
- `conversations.system_prompt` is set on INSERT and read in SELECT queries (`conversation/mod.rs:87-89`, `:120`, `:146`)
- `conversation_config.system_prompt` is the per-conversation override used by the engine
- `conversation_context.system_prompt_text` is a cached rendered version (fine, it's a cache)

**Plan:**
- Audit which one the engine reads as the "active" system prompt — likely `conversation_config` takes precedence
- If `conversations.system_prompt` is only used as the initial value when creating a conversation, migrate it:
  - On conversation CREATE, also INSERT into `conversation_config` with the system_prompt
  - Stop writing to `conversations.system_prompt`
  - Add ALTER TABLE migration to drop the column once all reads are migrated
- **Status:** NEEDS INVESTIGATION before changing

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
