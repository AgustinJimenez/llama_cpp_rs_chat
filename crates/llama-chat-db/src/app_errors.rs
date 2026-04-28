use rusqlite::params;

use super::{current_timestamp_millis, db_error, Database};

#[derive(Debug, Clone, serde::Serialize)]
pub struct AppErrorRecord {
    pub id: i64,
    pub level: String,
    pub source: String,
    pub message: String,
    pub details: Option<String>,
    pub timestamp: i64,
}

impl Database {
    pub fn record_app_error(
        &self,
        level: &str,
        source: &str,
        message: &str,
        details: Option<&str>,
        timestamp: Option<i64>,
    ) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "INSERT INTO app_errors (level, source, message, details, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                level,
                source,
                message,
                details,
                timestamp.unwrap_or_else(current_timestamp_millis),
            ],
        )
        .map_err(db_error("insert app error"))?;
        Ok(())
    }

    pub fn get_app_errors(&self, limit: usize) -> Result<Vec<AppErrorRecord>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, level, source, message, details, timestamp
                 FROM app_errors
                 ORDER BY timestamp DESC, id DESC
                 LIMIT ?1",
            )
            .map_err(db_error("prepare app errors query"))?;

        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(AppErrorRecord {
                    id: row.get(0)?,
                    level: row.get(1)?,
                    source: row.get(2)?,
                    message: row.get(3)?,
                    details: row.get(4)?,
                    timestamp: row.get(5)?,
                })
            })
            .map_err(db_error("query app errors"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(db_error("collect app errors"))
    }

    pub fn clear_app_errors(&self) -> Result<usize, String> {
        let conn = self.connection();
        conn.execute("DELETE FROM app_errors", [])
            .map_err(db_error("clear app errors"))
    }
}
