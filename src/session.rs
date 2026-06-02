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

use crate::db::store::{Db, Session, SessionState};

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
}
