# mnemo Architecture

## Overview

mnemo is a local-first AI memory layer. It accepts raw text through a REST API, extracts structured knowledge (entities and relationships) using a configurable LLM, persists everything to SQLite, and retrieves relevant context in under 50 ms using a combination of full-text search, entity matching, and knowledge graph traversal.

---

## Crate structure

| Crate | Type | Role | Key files |
|-------|------|------|-----------|
| `mnemo-core` | lib | All business logic. No I/O dependencies beyond SQLite and HTTP. | `db.rs`, `graph.rs`, `extractor.rs`, `retrieval.rs`, `models.rs`, `provider.rs` |
| `mnemo-api` | bin | Thin Axum handler layer. Wires AppState, delegates everything to mnemo-core. | `main.rs`, `tests.rs` |
| `mnemo-cli` | bin | CLI tool using blocking reqwest. All output is human-readable text. | `main.rs` |
| `mnemo-bench` | bin | 12 benchmark suites with colored ASCII output and optional JSON export. | `main.rs` |

The dependency graph is strictly one-way: `mnemo-api`, `mnemo-cli`, and `mnemo-bench` all depend on `mnemo-core`. `mnemo-core` has no knowledge of the HTTP layer.

---

## Data flow: Ingestion

When your application calls `POST /ingest` with a body `{"content": "...", "source": "..."}`:

1. **Handler** (`ingest_handler` in `main.rs`) deserializes the request and constructs a `MemoryChunk` with a fresh UUID and the current UTC timestamp.
2. **Persist chunk** — `db.insert_chunk()` writes the chunk to the `memory_chunks` table in SQLite. The chunk is stored *before* extraction so it is never lost even if the LLM is unavailable.
3. **Entity extraction** — `extractor.extract_with_fallback()` sends the chunk content to the configured LLM using a structured system prompt and JSON schema. The response is parsed into `ExtractedEntity` and `ExtractedRelation` structs. If the LLM is unreachable or returns invalid JSON, `extract_with_fallback` returns an empty `ExtractionResult` (no panic, no error response to caller).
4. **Graph ingestion** — the handler acquires a write lock on `SharedGraph` and calls `graph.ingest()`. This:
   a. Calls `resolve_or_create_entity()` for each extracted entity, which deduplicates by `name + entity_type` (see below).
   b. Writes a `memory_chunk_entities` join-table row linking the chunk to each entity.
   c. Resolves each extracted relation's `from_entity` and `to_entity` names to UUIDs via the local `name_to_id` map. Relations with unknown entity names are skipped with a `tracing::warn!`.
   d. Calls `db.upsert_relation()` which uses an `ON CONFLICT` clause to increment `weight = MIN(1.0, weight + 0.1)` on repeated sightings of the same relation.
   e. Updates the in-memory `petgraph::DiGraph` with the new nodes and edges.
5. **Response** — `IngestResponse` is returned: `chunk_id`, `entities_extracted`, `relations_extracted`, `processing_time_ms`.

---

## Data flow: Retrieval

When your application calls `POST /retrieve` with a `RetrievalQuery`:

1. **Stage 1 — Chunk retrieval.** `db.search_chunks_by_content(text, 50)` runs a `LIKE %text%` query. If `session_id` is set, `db.list_chunks_by_session()` is also called and results are merged by chunk UUID.
2. **Stage 2 — Entity retrieval.** `db.search_entities_by_name(text, 50)` runs a `LIKE %text%` query. For the top-10 scored chunks from stage 1, `db.get_entities_for_chunk()` is also called and results are merged by entity UUID.
3. **Stage 3 — Graph expansion.** If `include_graph` is true, the top `max_entities` scored entities are used as starting nodes. `graph.get_subgraph_entities(ids, graph_depth)` performs BFS up to `graph_depth` hops from each starting node, deduplicating by entity UUID across all starting nodes. Graph-expanded entities receive a score penalty of `0.5×` their base score.
4. **Stage 4 — Relation filter.** All relations where *both* `from_entity_id` and `to_entity_id` are in the accumulated entity set are included. This ensures the relation graph shown in context is internally consistent.
5. **Stage 5 — Scoring and truncation.** Chunks and entities are sorted by score descending, filtered below `min_confidence`, and truncated to `max_chunks` / `max_entities`.
6. **Stage 6 — Context assembly.** `build_context_prompt()` formats the result as a structured string with three optional sections: `[RELEVANT FACTS]` (entities), `[RELATIONSHIPS]`, and `[RELEVANT MEMORIES]` (chunks). Empty sections are omitted. Chunk content is truncated at 500 characters.

---

## Knowledge graph

The graph is a `petgraph::DiGraph<GraphNode, GraphEdge>` held entirely in memory, wrapped in `Arc<RwLock<KnowledgeGraph>>` (type alias: `SharedGraph`).

```rust
pub struct GraphNode {
    pub entity_id: Uuid,
    pub name: String,
    pub entity_type: EntityType,
    pub confidence: f32,
}

pub struct GraphEdge {
    pub relation_id: Uuid,
    pub relation_type: String,
    pub weight: f32,
}
```

On startup, `load_from_db()` hydrates the graph in two passes: all entities as nodes first, then all relations as edges. This guarantees no edge references a missing node.

`get_neighbors(entity_id, depth)` performs BFS up to `depth` hops. A `visited: HashMap<NodeIndex, usize>` prevents revisiting nodes, which handles cycles correctly without any risk of infinite loops.

`reload()` clears the graph and node index, then re-runs `load_from_db()`. It is called after `DELETE /wipe` to synchronize in-memory state with the now-empty database.

---

## Entity deduplication

`resolve_or_create_entity()` (in `graph.rs`) is called for every extracted entity during ingestion. It calls `db.get_entity_by_name(name)` — if a row exists with the same name, the existing entity is returned and its aliases and attributes are merged with the new extraction. If no row exists, a new entity is created with a fresh UUID.

The deduplication key is **name only** (case-sensitive). Two extractions of `"Alice"` as `Person` and `"Alice"` as `Organization` are treated as the same entity — the first extraction's type wins. This is a deliberate simplification; the alternative (dedup on name+type) produces duplicate nodes for common ambiguous names in practice.

`db.upsert_entity()` performs the merge at the SQL layer:
- If a row with `(name, entity_type)` already exists, it does an `UPDATE` that increments `source_count`, merges the alias lists (union, deduplicated), and overlays the new attributes onto the existing ones.
- Otherwise, it does an `INSERT`.

---

## LLM provider abstraction

`ProviderType` (in `provider.rs`) is an enum with four variants: `Ollama`, `OpenAi`, `Anthropic`, `Custom`. Each variant provides:
- `default_base_url()` — sensible default endpoint
- `default_model()` — sensible default model name
- `requires_api_key()` — whether a real API key is expected

`LlmProvider::build_request()` handles the two diverging API formats:
- **Anthropic** — `POST /messages`, `x-api-key` header, `anthropic-version` header, body uses `max_tokens` at the top level and `content[]` in the response.
- **OpenAI-compatible** (Ollama, OpenAI, Custom) — `POST /chat/completions`, `Authorization: Bearer` header, body uses `messages[]` and `choices[0].message.content` in the response.

`complete()` retries up to `max_retries` times (default 3) on network errors with 500 ms backoff. JSON parse failures from the LLM response are propagated as `MnemoError::Extraction` and cause `extract_with_fallback` to return an empty result rather than an error.

---

## Database schema

Four tables in SQLite, created by four numbered migration files in `crates/mnemo-core/migrations/`:

### `entities`
```sql
id           TEXT PRIMARY KEY,   -- UUID as text
name         TEXT NOT NULL,
entity_type  TEXT NOT NULL,      -- JSON-serialized EntityType enum
aliases      TEXT NOT NULL,      -- JSON array of strings
attributes   TEXT NOT NULL,      -- JSON object
confidence   REAL NOT NULL,
source_count INTEGER NOT NULL DEFAULT 1,
created_at   TEXT NOT NULL,      -- RFC3339
updated_at   TEXT NOT NULL
```
Index: `(name, entity_type)` — used for upsert deduplication.

### `relations`
```sql
id               TEXT PRIMARY KEY,
from_entity_id   TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
to_entity_id     TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
relation_type    TEXT NOT NULL,
weight           REAL NOT NULL DEFAULT 0.5,  -- capped at 1.0 by ON CONFLICT
attributes       TEXT NOT NULL,
created_at       TEXT NOT NULL,
updated_at       TEXT NOT NULL,
UNIQUE(from_entity_id, to_entity_id, relation_type)
```
The `ON CONFLICT` clause: `weight = MIN(1.0, weight + 0.1)` — each repeat sighting of the same relation increases confidence, capped at 1.0.

### `memory_chunks`
```sql
id          TEXT PRIMARY KEY,
content     TEXT NOT NULL,
source      TEXT NOT NULL,
session_id  TEXT,               -- nullable
embedding   TEXT,               -- JSON array of f32, nullable
metadata    TEXT NOT NULL,      -- JSON object
created_at  TEXT NOT NULL
```
Index: `content` — full-text LIKE search. Index: `session_id` — session filtering.

### `memory_chunk_entities`
```sql
chunk_id      TEXT NOT NULL REFERENCES memory_chunks(id) ON DELETE CASCADE,
entity_id     TEXT NOT NULL REFERENCES entities(id)      ON DELETE CASCADE,
mention_text  TEXT NOT NULL,
confidence    REAL NOT NULL,
PRIMARY KEY (chunk_id, entity_id)
```
This is the join table linking chunks to the entities they mention. Both foreign keys have `ON DELETE CASCADE` so deleting a chunk or entity automatically cleans up the join rows.

SQLite is configured with `PRAGMA journal_mode=WAL`, `PRAGMA foreign_keys=ON`, and `PRAGMA busy_timeout=5000` on every connection.

---

## Scoring algorithm

### Chunk scoring (`score_chunk`)

```
score = 0.5                                      (base)
      + min(keyword_overlap_count × 0.1, 0.4)   (keyword match, capped)
      + 0.1  if age < 24 hours                  (recency bonus)
      + 0.05 if age < 7 days                    (recency bonus, weaker)
      + 0.15 if chunk.session_id == query.session_id
```

Final score is clamped to `[0.0, 1.0]`.

### Entity scoring (`score_entity`)

```
score = entity.confidence × 0.5                  (base)
      + 0.3  if query_text contains entity.name  (name match)
      + 0.2  if query_text contains any alias     (alias match, first hit only)
      + min(entity.source_count × 0.02, 0.2)     (popularity bonus, capped)
```

Final score is clamped to `[0.0, 1.0]`.

---

## Configuration

Configuration is loaded in this precedence order (highest wins):

1. **Environment variables** — `MNEMO_DB_PATH`, `MNEMO_PORT`, `MNEMO_LLM_BASE_URL`, `MNEMO_LLM_MODEL`, `MNEMO_LLM_API_KEY`, `MNEMO_LLM_PROVIDER`
2. **TOML config file** — passed via `--config path/to/config.toml`
3. **Compiled-in defaults** — Ollama at `localhost:11434`, port 8080, `mnemo.db`

`MnemoConfig::from_env_or_file()` tries env vars first, then falls back to the TOML file if `--config` was provided. The active source is stored in `AppState.config_source` and returned in `GET /health` as `config_source: "env"` or `config_source: "file:path"`.
