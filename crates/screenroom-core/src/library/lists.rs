use rusqlite::params;

use super::{Library, now_ms};
use crate::error::Result;

impl Library {
    pub fn set_favorite(&self, profile_id: &str, work_id: &str, value: bool) -> Result<()> {
        self.set_list_flag(profile_id, work_id, "favorite", value)
    }

    pub fn set_watchlisted(&self, profile_id: &str, work_id: &str, value: bool) -> Result<()> {
        self.set_list_flag(profile_id, work_id, "watchlisted", value)
    }

    pub fn mark_watched(&self, profile_id: &str, edition_id: &str, value: bool) -> Result<()> {
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        tx.execute(
            "UPDATE sessions SET status = 'closed' WHERE profile_id = ?1 AND edition_id = ?2",
            params![profile_id, edition_id],
        )?;
        tx.execute(
            "INSERT INTO progress(profile_id, edition_id, position_ms, duration_ms, watched,
             hidden, updated_ms) VALUES(?1, ?2, 0, 0, ?3, 0, ?4)
             ON CONFLICT(profile_id, edition_id) DO UPDATE SET
             position_ms = 0, watched = excluded.watched, hidden = 0,
             updated_ms = excluded.updated_ms",
            params![profile_id, edition_id, i64::from(value), now_ms()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn hide_continue(&self, profile_id: &str, edition_id: &str) -> Result<()> {
        self.connection().execute(
            "UPDATE progress SET hidden = 1 WHERE profile_id = ?1 AND edition_id = ?2",
            params![profile_id, edition_id],
        )?;
        Ok(())
    }

    fn set_list_flag(
        &self,
        profile_id: &str,
        work_id: &str,
        field: &str,
        value: bool,
    ) -> Result<()> {
        let sql = format!(
            "INSERT INTO watchlist(profile_id, work_id, {field}, created_ms)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(profile_id, work_id) DO UPDATE SET {field} = excluded.{field}"
        );
        self.connection().execute(
            &sql,
            params![profile_id, work_id, i64::from(value), now_ms()],
        )?;
        Ok(())
    }
}
