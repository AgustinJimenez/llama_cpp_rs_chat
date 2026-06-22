use super::{current_timestamp_millis, db_error, Database};

impl Database {
    /// Create the pending_approvals table if it doesn't exist (called once on first use).
    fn ensure_pending_approvals_table(&self) {
        let conn = self.connection();
        let _ = conn.execute(
            "CREATE TABLE IF NOT EXISTS pending_approvals (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                tool TEXT NOT NULL,
                args_json TEXT NOT NULL,
                reason TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at INTEGER NOT NULL
            )",
            [],
        );
    }

    /// Record a pending approval request. Returns an error string on DB failure.
    pub fn create_pending_approval(
        &self,
        id: &str,
        conversation_id: &str,
        tool: &str,
        args_json: &str,
        reason: &str,
    ) -> Result<(), String> {
        self.ensure_pending_approvals_table();
        self.connection()
            .execute(
                "INSERT INTO pending_approvals (id, conversation_id, tool, args_json, reason, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
                rusqlite::params![id, conversation_id, tool, args_json, reason, current_timestamp_millis()],
            )
            .map_err(db_error("create pending approval"))?;
        Ok(())
    }

    /// Check the current status of a pending approval ('pending', 'approved', 'rejected').
    /// Returns None if the record doesn't exist.
    pub fn get_pending_approval_status(&self, id: &str) -> Option<String> {
        self.ensure_pending_approvals_table();
        self.connection()
            .query_row(
                "SELECT status FROM pending_approvals WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .ok()
    }

    /// Set the status of a pending approval (e.g. 'approved' or 'rejected').
    pub fn resolve_pending_approval(&self, id: &str, status: &str) -> Result<(), String> {
        self.ensure_pending_approvals_table();
        self.connection()
            .execute(
                "UPDATE pending_approvals SET status = ?1 WHERE id = ?2",
                rusqlite::params![status, id],
            )
            .map_err(db_error("resolve pending approval"))?;
        Ok(())
    }

    /// Purge approval records older than `max_age_secs`. Called opportunistically on startup.
    pub fn purge_old_pending_approvals(&self, max_age_secs: u64) {
        self.ensure_pending_approvals_table();
        let cutoff = current_timestamp_millis() - (max_age_secs as i64 * 1000);
        let _ = self.connection().execute(
            "DELETE FROM pending_approvals WHERE created_at < ?1",
            [cutoff],
        );
    }
}
