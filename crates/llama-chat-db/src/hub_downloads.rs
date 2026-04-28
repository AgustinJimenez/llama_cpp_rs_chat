// Database methods for tracking HuggingFace Hub downloads

use super::{current_timestamp_millis, db_error, Database};
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HubDownloadRecord {
    pub id: i64,
    pub model_id: String,
    pub filename: String,
    pub dest_path: String,
    pub file_size: i64,
    pub bytes_downloaded: i64,
    pub status: String, // "pending" | "completed"
    pub etag: Option<String>,
    pub downloaded_at: i64,
}

impl Database {
    /// Insert or update a download record (used when starting a download)
    pub fn save_hub_download(
        &self,
        model_id: &str,
        filename: &str,
        dest_path: &str,
        file_size: i64,
        status: &str,
        etag: Option<&str>,
    ) -> Result<i64, String> {
        let conn = self.connection();
        conn.execute(
            "INSERT OR REPLACE INTO hub_downloads (model_id, filename, dest_path, file_size, bytes_downloaded, status, etag, downloaded_at) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7)",
            params![model_id, filename, dest_path, file_size, status, etag, current_timestamp_millis()],
        )
        .map_err(db_error("save hub download"))?;
        Ok(conn.last_insert_rowid())
    }

    /// Update download progress checkpoint (called periodically during download)
    pub fn update_download_progress(&self, id: i64, bytes_downloaded: i64) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "UPDATE hub_downloads SET bytes_downloaded = ?1 WHERE id = ?2",
            params![bytes_downloaded, id],
        )
        .map_err(db_error("update download progress"))?;
        Ok(())
    }

    /// Mark a download as completed
    pub fn mark_download_completed(&self, id: i64, file_size: i64) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "UPDATE hub_downloads SET status = 'completed', file_size = ?1, bytes_downloaded = ?1, downloaded_at = ?2 WHERE id = ?3",
            params![file_size, current_timestamp_millis(), id],
        )
        .map_err(db_error("mark download completed"))?;
        Ok(())
    }

    /// Find a pending download record for the given model/file/dest
    pub fn find_pending_download(
        &self,
        model_id: &str,
        filename: &str,
        dest_path: &str,
    ) -> Result<Option<HubDownloadRecord>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare("SELECT id, model_id, filename, dest_path, file_size, bytes_downloaded, status, etag, downloaded_at FROM hub_downloads WHERE model_id = ?1 AND filename = ?2 AND dest_path = ?3 AND status = 'pending'")
            .map_err(db_error("prepare find pending download"))?;

        let mut rows = stmt
            .query_map(params![model_id, filename, dest_path], |row| {
                Ok(HubDownloadRecord {
                    id: row.get(0)?,
                    model_id: row.get(1)?,
                    filename: row.get(2)?,
                    dest_path: row.get(3)?,
                    file_size: row.get(4)?,
                    bytes_downloaded: row.get(5)?,
                    status: row.get(6)?,
                    etag: row.get(7)?,
                    downloaded_at: row.get(8)?,
                })
            })
            .map_err(db_error("query pending download"))?;

        match rows.next() {
            Some(Ok(rec)) => Ok(Some(rec)),
            _ => Ok(None),
        }
    }

    /// Get all download records
    pub fn get_hub_downloads(&self) -> Result<Vec<HubDownloadRecord>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare("SELECT id, model_id, filename, dest_path, file_size, bytes_downloaded, status, etag, downloaded_at FROM hub_downloads ORDER BY downloaded_at DESC")
            .map_err(db_error("prepare hub downloads query"))?;

        let records = stmt
            .query_map([], |row| {
                Ok(HubDownloadRecord {
                    id: row.get(0)?,
                    model_id: row.get(1)?,
                    filename: row.get(2)?,
                    dest_path: row.get(3)?,
                    file_size: row.get(4)?,
                    bytes_downloaded: row.get(5)?,
                    status: row.get(6)?,
                    etag: row.get(7)?,
                    downloaded_at: row.get(8)?,
                })
            })
            .map_err(db_error("query hub downloads"))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(records)
    }

    /// Delete download records by IDs (batch)
    pub fn delete_hub_downloads_by_ids(&self, ids: &[i64]) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self.connection();
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "DELETE FROM hub_downloads WHERE id IN ({})",
            placeholders.join(",")
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        conn.execute(&sql, params.as_slice())
            .map_err(db_error("delete hub downloads"))?;
        Ok(())
    }
}
