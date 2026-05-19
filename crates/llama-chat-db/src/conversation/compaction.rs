use super::*;

impl Database {
    /// Record a compaction summary covering all messages up to `covers_to_sequence`.
    ///
    /// Any previous summary for this conversation is replaced — each new summary is
    /// comprehensive (the compaction engine re-summarizes the full history), so older
    /// partial summaries are redundant.
    ///
    /// Returns the number of user/assistant messages in the covered range.
    pub fn compact_messages(
        &self,
        conversation_id: &str,
        covers_to_sequence: i32,
        summary: &str,
    ) -> Result<usize, String> {
        let conn = self.connection();

        let message_count: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM messages \
                 WHERE conversation_id = ?1 AND sequence_order <= ?2 AND role != 'system'",
                params![conversation_id, covers_to_sequence],
                |row| row.get::<_, i32>(0),
            )
            .map_err(db_error("count compacted messages"))? as usize;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        conn.execute(
            "DELETE FROM compaction_summaries WHERE conversation_id = ?1",
            params![conversation_id],
        )
        .map_err(db_error("delete old compaction summaries"))?;

        conn.execute(
            "INSERT INTO compaction_summaries \
             (id, conversation_id, covers_from_sequence, covers_to_sequence, message_count, summary_text, created_at) \
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6)",
            params![
                uuid::Uuid::new_v4().to_string(),
                conversation_id,
                covers_to_sequence,
                message_count as i32,
                summary,
                timestamp,
            ],
        )
        .map_err(db_error("insert compaction summary"))?;

        Ok(message_count)
    }

    pub fn update_compaction_summary(
        &self,
        conversation_id: &str,
        summary_text: &str,
    ) -> Result<(), String> {
        let conn = self.connection();
        let updated = conn
            .execute(
                "UPDATE compaction_summaries SET summary_text = ?1 WHERE conversation_id = ?2",
                params![summary_text, conversation_id],
            )
            .map_err(db_error("update compaction summary"))?;
        if updated == 0 {
            Err("No compaction summary found".to_string())
        } else {
            Ok(())
        }
    }

    pub fn delete_compaction_summary(&self, conversation_id: &str) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "DELETE FROM compaction_summaries WHERE conversation_id = ?1",
            params![conversation_id],
        )
        .map_err(db_error("delete compaction summary"))?;
        Ok(())
    }
}
