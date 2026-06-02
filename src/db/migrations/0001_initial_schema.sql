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
) STRICT;

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
) STRICT;

CREATE INDEX idx_sessions_updated_at ON sessions(updated_at DESC);
