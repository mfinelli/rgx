/* rgx: command line regexp tester
 * Copyright 2026 Mario Finelli
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

//! Session management: owns the active session and mediates undo/redo.
//!
//! `SessionManager` wraps `Db` and tracks which session is active, where
//! in the undo stack we are, and whether there are unsaved changes.
//! It is the single source of truth for session state during a run.

use anyhow::Result;

use crate::db::store::{Db, ListFilter, Session, SessionState, SessionSummary};

/// Manages the active session and its undo/redo stack.
pub struct SessionManager {
    db: Db,
    session_id: i64,
    /// Current position in the undo stack (matches `session.undo_cursor`).
    cursor: i64,
}

impl SessionManager {
    /// Resume an existing session, incrementing its use count.
    pub fn resume(db: Db, session: Session) -> Result<Self> {
        db.increment_use_count(session.id)?;
        Ok(Self {
            session_id: session.id,
            cursor: session.undo_cursor,
            db,
        })
    }

    /// Start a new session.
    pub fn new_session(
        db: Db,
        language: &str,
        flavor: Option<&str>,
    ) -> Result<Self> {
        let id = db.create_session(language, flavor)?;
        Ok(Self {
            session_id: id,
            cursor: 0,
            db,
        })
    }

    /// The id of the active session.
    pub fn session_id(&self) -> i64 {
        self.session_id
    }

    /// The current undo cursor position.
    pub fn cursor(&self) -> i64 {
        self.cursor
    }

    /// Load the state at the current cursor position.
    /// Returns `None` if the session has no states yet.
    pub fn current_state(&self) -> Result<Option<SessionState>> {
        if self.cursor == 0 {
            return Ok(None);
        }
        self.db.get_state(self.session_id, self.cursor)
    }

    /// Push a new state and advance the cursor.
    ///
    /// If cursor < max_seq (we've undone some states), forward history is
    /// truncated (see `Db::push_state`). Full fork logic is deferred to
    /// phase 19.
    pub fn push(&mut self, state: &SessionState) -> Result<()> {
        let new_seq =
            self.db.push_state(self.session_id, self.cursor, state)?;
        self.cursor = new_seq;
        self.db.set_undo_cursor(self.session_id, self.cursor)?;
        Ok(())
    }

    /// Move the cursor back by one (undo). Returns the state to load,
    /// or `None` if already at the beginning.
    pub fn undo(&mut self) -> Result<Option<SessionState>> {
        if self.cursor <= 1 {
            return Ok(None);
        }
        self.cursor -= 1;
        self.db.set_undo_cursor(self.session_id, self.cursor)?;
        self.db.get_state(self.session_id, self.cursor)
    }

    /// Move the cursor forward by one (redo). Returns the state to load,
    /// or `None` if already at the head.
    pub fn redo(&mut self) -> Result<Option<SessionState>> {
        let max = self.db.max_seq(self.session_id)?;
        if self.cursor >= max {
            return Ok(None);
        }
        self.cursor += 1;
        self.db.set_undo_cursor(self.session_id, self.cursor)?;
        self.db.get_state(self.session_id, self.cursor)
    }

    /// Whether undo is available.
    pub fn can_undo(&self) -> bool {
        self.cursor > 1
    }

    /// Whether redo is available (requires a DB query).
    pub fn can_redo(&self) -> Result<bool> {
        Ok(self.cursor < self.db.max_seq(self.session_id)?)
    }

    /// List sessions for the history panel.
    pub fn list_sessions(
        &self,
        filter: &ListFilter,
    ) -> Result<Vec<SessionSummary>> {
        self.db.list_sessions(filter)
    }

    /// Rename the current session. Pass `None` to clear the name.
    pub fn rename_current(&self, name: Option<&str>) -> Result<()> {
        self.db.rename_session(self.session_id, name)
    }

    /// Rename any session by id.
    pub fn rename_session(&self, id: i64, name: Option<&str>) -> Result<()> {
        self.db.rename_session(id, name)
    }

    /// Delete a session by id. Refuses to delete the active session.
    pub fn delete_session(&self, id: i64) -> Result<()> {
        if id == self.session_id {
            anyhow::bail!("cannot delete the active session");
        }
        self.db.delete_session(id)
    }

    /// Switch to a different session. Returns the state to load into the app,
    /// or `None` if the target session has no states yet.
    ///
    /// Updates `session_id` and `cursor` to reflect the new active session.
    pub fn switch_to(&mut self, id: i64) -> Result<Option<SessionState>> {
        if id == self.session_id {
            return self.current_state();
        }
        let session = self
            .db
            .get_session(id)?
            .ok_or_else(|| anyhow::anyhow!("session {} not found", id))?;
        self.db.increment_use_count(id)?;
        self.session_id = session.id;
        self.cursor = session.undo_cursor;
        self.current_state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::store::Db;
    use std::path::PathBuf;

    fn temp_db() -> (Db, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "rgx_session_test_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Db::open(&path).unwrap();
        (db, path)
    }

    fn cleanup(path: &PathBuf) {
        std::fs::remove_file(path).ok();
        std::fs::remove_file(path.with_extension("db-wal")).ok();
        std::fs::remove_file(path.with_extension("db-shm")).ok();
    }

    fn state(pattern: &str) -> SessionState {
        SessionState {
            pattern: Some(pattern.to_string()),
            mode: "match".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn undo_moves_cursor_back() {
        let (db, path) = temp_db();
        {
            let mut mgr =
                SessionManager::new_session(db, "rust", None).unwrap();
            mgr.push(&state("a")).unwrap(); // seq 1
            mgr.push(&state("b")).unwrap(); // seq 2
            mgr.push(&state("c")).unwrap(); // seq 3

            let s = mgr.undo().unwrap().unwrap();
            assert_eq!(s.pattern.as_deref(), Some("b")); // back to seq 2
            assert_eq!(mgr.cursor(), 2);
        }
        cleanup(&path);
    }

    #[test]
    fn redo_after_undo_restores_forward_state() {
        let (db, path) = temp_db();
        {
            let mut mgr =
                SessionManager::new_session(db, "rust", None).unwrap();
            mgr.push(&state("a")).unwrap(); // seq 1
            mgr.push(&state("b")).unwrap(); // seq 2
            mgr.push(&state("c")).unwrap(); // seq 3

            mgr.undo().unwrap(); // -> seq 2
            let s = mgr.redo().unwrap().unwrap();
            assert_eq!(s.pattern.as_deref(), Some("c")); // back to seq 3
            assert_eq!(mgr.cursor(), 3);
        }
        cleanup(&path);
    }

    #[test]
    fn multiple_undo_steps() {
        let (db, path) = temp_db();
        {
            let mut mgr =
                SessionManager::new_session(db, "rust", None).unwrap();
            mgr.push(&state("a")).unwrap(); // seq 1
            mgr.push(&state("b")).unwrap(); // seq 2
            mgr.push(&state("c")).unwrap(); // seq 3

            mgr.undo().unwrap(); // -> seq 2
            let s = mgr.undo().unwrap().unwrap();
            assert_eq!(s.pattern.as_deref(), Some("a")); // -> seq 1
            assert_eq!(mgr.cursor(), 1);
        }
        cleanup(&path);
    }

    #[test]
    fn undo_at_beginning_returns_none() {
        let (db, path) = temp_db();
        {
            let mut mgr =
                SessionManager::new_session(db, "rust", None).unwrap();
            mgr.push(&state("a")).unwrap(); // seq 1

            mgr.undo().unwrap(); // -> below seq 1: returns None (at cursor=1, can't go lower)
            let result = mgr.undo().unwrap();
            assert!(result.is_none());
        }
        cleanup(&path);
    }

    #[test]
    fn redo_at_head_returns_none() {
        let (db, path) = temp_db();
        {
            let mut mgr =
                SessionManager::new_session(db, "rust", None).unwrap();
            mgr.push(&state("a")).unwrap(); // seq 1

            let result = mgr.redo().unwrap();
            assert!(result.is_none());
        }
        cleanup(&path);
    }

    #[test]
    fn push_after_undo_truncates_forward_history() {
        let (db, path) = temp_db();
        {
            let mut mgr =
                SessionManager::new_session(db, "rust", None).unwrap();
            mgr.push(&state("a")).unwrap(); // seq 1
            mgr.push(&state("b")).unwrap(); // seq 2
            mgr.push(&state("c")).unwrap(); // seq 3

            mgr.undo().unwrap(); // -> seq 2
            // Pushing new state while not at head truncates seq 3
            mgr.push(&state("d")).unwrap(); // new seq 3

            // Redo is no longer possible (we're at the head)
            assert!(!mgr.can_redo().unwrap());
            let s = mgr.current_state().unwrap().unwrap();
            assert_eq!(s.pattern.as_deref(), Some("d"));
        }
        cleanup(&path);
    }
}
