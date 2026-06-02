CREATE TABLE sessions (
    id              INTEGER PRIMARY KEY,
    name            TEXT    CHECK (name IS NULL OR name != ''),
    language        TEXT    NOT NULL CHECK (language != ''),
    flavor          TEXT    CHECK (flavor IS NULL OR flavor != ''),
    created_at      TEXT    NOT NULL,
    updated_at      TEXT    NOT NULL,
    use_count       INTEGER NOT NULL DEFAULT 1,
    undo_cursor     INTEGER NOT NULL DEFAULT 0,
    forked_from_id  INTEGER REFERENCES sessions(id),
    forked_at_seq   INTEGER
) STRICT;

CREATE TABLE session_states (
    id           INTEGER PRIMARY KEY,
    session_id   INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    pattern      TEXT    CHECK (pattern IS NULL OR pattern != ''),
    options      TEXT    CHECK (options IS NULL OR options != ''),
    input        TEXT    CHECK (input IS NULL OR input != ''),
    replacement  TEXT    CHECK (replacement IS NULL OR replacement != ''),
    mode         TEXT    NOT NULL DEFAULT 'match'
                         CHECK (mode IN ('match', 'replace')),
    source_file  TEXT    CHECK (source_file IS NULL OR source_file != ''),
    file_dirty   INTEGER NOT NULL DEFAULT 0,
    UNIQUE(session_id, seq)
) STRICT;

CREATE INDEX idx_sessions_updated_at ON sessions(updated_at DESC);

-- Partial index for named session lookups and the "Named" filter in the
-- history panel. Most sessions are unnamed so a partial index is more
-- efficient than indexing all rows.
CREATE INDEX idx_sessions_name
    ON sessions(name)
    WHERE name IS NOT NULL;

-- Composite index covering both language-only and language+flavor queries.
-- SQLite supports prefix lookups so WHERE language = ? uses this index
-- even without a flavor condition.
CREATE INDEX idx_sessions_language_flavor
    ON sessions(language, flavor);

-- Partial index for ancestry reconstruction when walking fork chains.
-- Most sessions are not forks so only index rows where it applies.
CREATE INDEX idx_sessions_forked_from_id
    ON sessions(forked_from_id)
    WHERE forked_from_id IS NOT NULL;
