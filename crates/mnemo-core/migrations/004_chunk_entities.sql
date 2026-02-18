CREATE TABLE IF NOT EXISTS memory_chunk_entities (
    chunk_id        TEXT NOT NULL REFERENCES memory_chunks(id) ON DELETE CASCADE,
    entity_id       TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    mention_text    TEXT NOT NULL,
    confidence      REAL NOT NULL DEFAULT 1.0,
    PRIMARY KEY (chunk_id, entity_id)
);

CREATE INDEX IF NOT EXISTS idx_ce_chunk ON memory_chunk_entities(chunk_id);
CREATE INDEX IF NOT EXISTS idx_ce_entity ON memory_chunk_entities(entity_id);
