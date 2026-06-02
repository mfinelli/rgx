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

//! SQLite persistence layer.
//!
//! All database access goes through `Db`. The schema is versioned via
//! `rusqlite_migration` and applied automatically on open. WAL mode and
//! foreign key enforcement are enabled for every connection.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};

/// Initial schema migration.
const M1: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE sessions (
    id              INTEGER PRIMARY KEY,
    name            TEXT,
    language        TEXT NOT NULL DEFAULT 'rust',
    flavor          TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    use_count       INTEGER NOT NULL DEFAULT 1,
    undo_cursor     INTEGER NOT NULL DEFAULT 0,
    forked_from_id  INTEGER REFERENCES sessions(id),
    forked_at_seq   INTEGER
);

CREATE TABLE session_states (
    id           INTEGER PRIMARY KEY,
    session_id   INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    pattern      TEXT NOT NULL DEFAULT '',
    options      TEXT NOT NULL DEFAULT '',
    input        TEXT NOT NULL DEFAULT '',
    replacement  TEXT NOT NULL DEFAULT '',
    mode         TEXT NOT NULL DEFAULT 'match',
    source_file  TEXT,
    file_dirty   INTEGER NOT NULL DEFAULT 0,
    UNIQUE(session_id, seq)
);
";

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(M1)])
}

/// A persisted session row.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    pub name: Option<String>,
    pub language: String,
    pub flavor: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub use_count: i64,
    pub undo_cursor: i64,
}

/// A persisted state snapshot within a session.
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    pub seq: i64,
    pub pattern: String,
    pub options: String, // JSON array of active flag names
    pub input: String,
    pub replacement: String,
    pub mode: String, // "match" | "replace"
    pub source_file: Option<String>,
    pub file_dirty: bool,
}

/// Handle to the SQLite database. All mutations go through this type.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database at `path`, running any pending migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create db directory: {}", parent.display())
            })?;
        }

        let mut conn = Connection::open(path).with_context(|| {
            format!("failed to open database: {}", path.display())
        })?;

        // WAL mode and foreign keys must be set before migrations run.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;",
        )
        .context("failed to configure database pragmas")?;

        migrations()
            .to_latest(&mut conn)
            .context("failed to apply database migrations")?;

        Ok(Self { conn })
    }

    /// Create a new session and return its id.
    pub fn create_session(
        &self,
        language: &str,
        flavor: Option<&str>,
    ) -> Result<i64> {
        let now = now_iso8601();
        self.conn.execute(
            "INSERT INTO sessions (language, flavor, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![language, flavor, now],
        ).context("failed to create session")?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Load a session by id.
    pub fn get_session(&self, id: i64) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, language, flavor, created_at, updated_at, use_count, undo_cursor
             FROM sessions WHERE id = ?1"
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Session {
                id: row.get(0)?,
                name: row.get(1)?,
                language: row.get(2)?,
                flavor: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                use_count: row.get(6)?,
                undo_cursor: row.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Return the most recently updated session, if any.
    pub fn most_recent_session(&self) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, language, flavor, created_at, updated_at, use_count, undo_cursor
             FROM sessions ORDER BY updated_at DESC LIMIT 1"
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Session {
                id: row.get(0)?,
                name: row.get(1)?,
                language: row.get(2)?,
                flavor: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                use_count: row.get(6)?,
                undo_cursor: row.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Persist the undo cursor position for a session.
    pub fn set_undo_cursor(&self, session_id: i64, cursor: i64) -> Result<()> {
        let now = now_iso8601();
        self.conn.execute(
            "UPDATE sessions SET undo_cursor = ?1, updated_at = ?2 WHERE id = ?3",
            params![cursor, now, session_id],
        ).context("failed to update undo cursor")?;
        Ok(())
    }

    /// Increment the use count for a session (called on resume).
    pub fn increment_use_count(&self, session_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET use_count = use_count + 1, updated_at = ?1 WHERE id = ?2",
            params![now_iso8601(), session_id],
        ).context("failed to increment use count")?;
        Ok(())
    }

    /// Append a new state to a session, returning its seq number.
    ///
    /// Any states with seq > current cursor are deleted first (non-head edit).
    /// This implements the implicit fork (but we defer the actual fork logic
    /// to phase 19); for now we just truncate the forward history.
    pub fn push_state(
        &self,
        session_id: i64,
        after_seq: i64,
        state: &SessionState,
    ) -> Result<i64> {
        // Truncate any states ahead of the current cursor.
        self.conn
            .execute(
                "DELETE FROM session_states WHERE session_id = ?1 AND seq > ?2",
                params![session_id, after_seq],
            )
            .context("failed to truncate forward history")?;

        let next_seq = after_seq + 1;
        self.conn.execute(
            "INSERT INTO session_states
             (session_id, seq, pattern, options, input, replacement, mode, source_file, file_dirty)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session_id, next_seq,
                state.pattern, state.options, state.input,
                state.replacement, state.mode,
                state.source_file, state.file_dirty as i64,
            ],
        ).context("failed to push session state")?;

        Ok(next_seq)
    }

    /// Load the state at a given seq number.
    pub fn get_state(
        &self,
        session_id: i64,
        seq: i64,
    ) -> Result<Option<SessionState>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, pattern, options, input, replacement, mode, source_file, file_dirty
             FROM session_states WHERE session_id = ?1 AND seq = ?2"
        )?;
        let mut rows = stmt.query(params![session_id, seq])?;
        if let Some(row) = rows.next()? {
            Ok(Some(SessionState {
                seq: row.get(0)?,
                pattern: row.get(1)?,
                options: row.get(2)?,
                input: row.get(3)?,
                replacement: row.get(4)?,
                mode: row.get(5)?,
                source_file: row.get(6)?,
                file_dirty: row.get::<_, i64>(7)? != 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Return the maximum seq for a session (the head of the undo stack).
    pub fn max_seq(&self, session_id: i64) -> Result<i64> {
        let seq: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM session_states WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        ).context("failed to query max seq")?;
        Ok(seq)
    }
}

/// Returns the XDG data directory path for the database.
pub fn default_db_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("rgx").join("history.db")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("rgx")
            .join("history.db")
    } else {
        PathBuf::from(".local")
            .join("share")
            .join("rgx")
            .join("history.db")
    }
}

/// Returns the current UTC time as an ISO 8601 string.
fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as YYYY-MM-DDTHH:MM:SSZ without pulling in chrono.
    let (y, mo, d, h, mi, s) = secs_to_datetime(secs);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, mi, s)
}

/// Converts Unix timestamp to (year, month, day, hour, minute, second).
/// Minimal implementation (no DST, no timezone, UTC only).
fn secs_to_datetime(mut secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    secs /= 60;
    let mi = secs % 60;
    secs /= 60;
    let h = secs % 24;
    secs /= 24;
    // Days since epoch (1970-01-01)
    let mut days = secs;
    let mut year = 1970u64;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let months = [
        31u64,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for dm in &months {
        if days < *dm {
            break;
        }
        days -= dm;
        month += 1;
    }
    (year, month, days + 1, h, mi, s)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100))
        || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_db() -> (Db, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "rgx_test_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Db::open(&path).unwrap();
        (db, path)
    }

    #[test]
    fn create_and_retrieve_session() {
        let (db, path) = temp_db();
        let id = db.create_session("rust", Some("regex")).unwrap();
        let session = db.get_session(id).unwrap().unwrap();
        assert_eq!(session.language, "rust");
        assert_eq!(session.flavor.as_deref(), Some("regex"));
        assert_eq!(session.undo_cursor, 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn push_and_retrieve_state() {
        let (db, path) = temp_db();
        let sid = db.create_session("rust", None).unwrap();
        let state = SessionState {
            pattern: "hello".to_string(),
            input: "hello world".to_string(),
            ..Default::default()
        };
        let seq = db.push_state(sid, 0, &state).unwrap();
        assert_eq!(seq, 1);
        let loaded = db.get_state(sid, seq).unwrap().unwrap();
        assert_eq!(loaded.pattern, "hello");
        assert_eq!(loaded.input, "hello world");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn push_state_truncates_forward_history() {
        let (db, path) = temp_db();
        let sid = db.create_session("rust", None).unwrap();
        let s = |p: &str| SessionState {
            pattern: p.to_string(),
            ..Default::default()
        };

        db.push_state(sid, 0, &s("a")).unwrap(); // seq 1
        db.push_state(sid, 1, &s("b")).unwrap(); // seq 2
        db.push_state(sid, 2, &s("c")).unwrap(); // seq 3

        // Undo to seq 1, then push; seq 2 and 3 should be gone
        db.push_state(sid, 1, &s("d")).unwrap(); // seq 2 (replaces old 2 and 3)

        assert!(db.get_state(sid, 3).unwrap().is_none());
        let loaded = db.get_state(sid, 2).unwrap().unwrap();
        assert_eq!(loaded.pattern, "d");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn undo_cursor_persists() {
        let (db, path) = temp_db();
        let sid = db.create_session("rust", None).unwrap();
        db.set_undo_cursor(sid, 5).unwrap();
        let session = db.get_session(sid).unwrap().unwrap();
        assert_eq!(session.undo_cursor, 5);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn now_iso8601_is_reasonable() {
        let s = now_iso8601();
        assert!(s.starts_with("20"), "expected year 20xx, got: {}", s);
        assert!(s.ends_with('Z'));
        assert_eq!(s.len(), 20); // YYYY-MM-DDTHH:MM:SSZ
    }
}
