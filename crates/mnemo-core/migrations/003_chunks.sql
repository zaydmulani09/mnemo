CREATE TABLE IF NOT EXISTS memory_chunks (
    id          TEXT PRIMARY KEY,
    content     TEXT NOT NULL,
    source      TEXT NOT NULL,
    session_id  TEXT,
    embedding   TEXT,
    metadata    TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chunks_session ON memory_chunks(session_id);
CREATE INDEX IF NOT EXISTS idx_chunks_source ON memory_chunks(source);
CREATE INDEX IF NOT EXISTS idx_chunks_created ON memory_chunks(created_at);
