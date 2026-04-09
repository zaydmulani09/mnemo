# mnemo API Reference

Base URL: `http://localhost:8080` (default)  
All endpoints accept and return `Content-Type: application/json`.  
Max request body: 10 MB.

---

## GET /health

Returns the server status, database connectivity, LLM reachability, and current configuration summary.

**Request:** No body.

**Response:**
```jsonc
{
  "status": "ok",
  "version": "0.1.0",
  "db_connected": true,
  "llm_reachable": true,
  "entity_count": 42,
  "chunk_count": 17,
  "uptime_seconds": 3600,
  "provider_type": "Ollama",
  "provider_model": "llama3",
  "config_source": "env"   // "env" or "file:/path/to/config.toml"
}
```

**Example:**
```bash
curl http://localhost:8080/health
```

**Errors:** None — always returns 200. `db_connected` and `llm_reachable` are `false` if those systems are unavailable.

---

## POST /ingest

Store a piece of text as a memory chunk. mnemo extracts entities and relationships from the content using the configured LLM and updates the knowledge graph.

**Request body:**
```jsonc
{
  "content": "string",           // required — the text to store
  "source": "string",            // required — origin label (e.g. "chat", "email")
  "session_id": "string | null", // optional — group related chunks
  "metadata": {}                 // optional — arbitrary JSON object
}
```

**Response:**
```jsonc
{
  "chunk_id": "550e8400-e29b-41d4-a716-446655440000",
  "entities_extracted": 3,
  "relations_extracted": 2,
  "processing_time_ms": 847
}
```

**Example:**
```bash
curl -X POST http://localhost:8080/ingest \
  -H "Content-Type: application/json" \
  -d '{
    "content": "Alice is a principal engineer at Stripe working on payment infrastructure.",
    "source": "chat",
    "session_id": "session-001",
    "metadata": {"turn": 1}
  }'
```

**Notes:**
- The chunk is written to SQLite *before* extraction, so it is never lost even if the LLM is unavailable.
- If the LLM is unreachable, extraction returns an empty result (`entities_extracted: 0`) — no error is returned to the caller.
- `processing_time_ms` includes LLM round-trip time.

**Errors:**
- `500` — database write failure.

---

## POST /retrieve

Retrieve ranked memory context for a query. Runs the full 6-stage retrieval pipeline: chunk search → entity search → graph expansion → relation filter → score+rank → context assembly.

**Request body:**
```jsonc
{
  "text": "string",              // required — query text
  "session_id": "string | null", // optional — session context bias
  "max_chunks": 10,              // default 10
  "max_entities": 20,            // default 20
  "min_confidence": 0.5,         // default 0.5 — filter below this score
  "include_graph": true,         // default true — expand via knowledge graph
  "graph_depth": 2               // default 2 — BFS hop depth (0–5)
}
```

**Response:**
```jsonc
{
  "chunks": [ /* MemoryChunk[] */ ],
  "entities": [ /* Entity[] */ ],
  "relations": [ /* Relation[] */ ],
  "context_prompt": "=== MEMORY CONTEXT ===\n\n[RELEVANT FACTS]\n...",
  "retrieved_at": "2024-01-15T10:30:00Z"
}
```

**Example:**
```bash
curl -X POST http://localhost:8080/retrieve \
  -H "Content-Type: application/json" \
  -d '{
    "text": "what does Alice work on?",
    "session_id": "session-001",
    "max_chunks": 5,
    "min_confidence": 0.3,
    "include_graph": true,
    "graph_depth": 2
  }'
```

**Notes:**
- Inject `context_prompt` into your LLM system prompt directly.
- `chunks`, `entities`, and `relations` are always arrays (never null) — empty arrays when no memory matches.
- Graph expansion adds neighbors of the top-scored entities; expanded entities receive a 0.5× score penalty.

**Errors:**
- `500` — database error.

---

## GET /entities

List all entities, paginated, sorted by `created_at` descending.

**Query parameters:**
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `limit` | integer | 50 | Max entities to return (max 200) |
| `offset` | integer | 0 | Number of entities to skip |

**Response:** `Entity[]`

```jsonc
[
  {
    "id": "uuid",
    "name": "Alice",
    "entity_type": "Person",        // Person|Organization|Place|Concept|Tool|Event|Document|Other
    "aliases": ["Al", "A. Smith"],
    "attributes": {"role": "engineer"},
    "confidence": 0.95,
    "source_count": 3,              // number of times seen across ingestions
    "created_at": "2024-01-15T10:00:00Z",
    "updated_at": "2024-01-15T10:05:00Z"
  }
]
```

**Example:**
```bash
curl "http://localhost:8080/entities?limit=10&offset=0"
```

---

## GET /entities/:id

Get a single entity by UUID.

**Response:** `Entity` (see above)

**Example:**
```bash
curl http://localhost:8080/entities/550e8400-e29b-41d4-a716-446655440000
```

**Errors:**
- `404` — entity not found.

---

## DELETE /entities/:id

Delete an entity. Cascades to relations and `memory_chunk_entities` join rows.

**Response:**
```json
{"deleted": true}
```

**Example:**
```bash
curl -X DELETE http://localhost:8080/entities/550e8400-e29b-41d4-a716-446655440000
```

---

## GET /entities/:id/neighbors

Return knowledge graph neighbors of an entity via BFS.

**Query parameters:**
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `depth` | integer | 2 | BFS depth (clamped to max 5) |

**Response:** `GraphNode[]`

```jsonc
[
  {
    "entity_id": "uuid",
    "name": "Stripe",
    "entity_type": "Organization",
    "confidence": 0.9
  }
]
```

**Example:**
```bash
curl "http://localhost:8080/entities/550e8400-e29b-41d4-a716-446655440000/neighbors?depth=2"
```

**Notes:**
- Returns all reachable neighbors within `depth` hops following outgoing edges.
- Cycles in the graph are handled safely — the BFS visited set prevents infinite loops.
- The start node itself is excluded from results.

---

## GET /chunks

List memory chunks, paginated, sorted by `created_at` descending.

**Query parameters:**
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `limit` | integer | 50 | Max chunks to return |
| `offset` | integer | 0 | Chunks to skip |
| `session_id` | string | — | Filter by session (overrides limit/offset) |

**Response:** `MemoryChunk[]`

```jsonc
[
  {
    "id": "uuid",
    "content": "Alice is a principal engineer at Stripe...",
    "source": "chat",
    "session_id": "session-001",
    "embedding": null,            // Vec<f32> if stored, otherwise null
    "metadata": {"turn": 1},
    "created_at": "2024-01-15T10:00:00Z"
  }
]
```

**Example:**
```bash
curl "http://localhost:8080/chunks?limit=20&offset=0"
curl "http://localhost:8080/chunks?session_id=session-001"
```

---

## GET /chunks/:id

Get a single memory chunk by UUID.

**Response:** `MemoryChunk` (see above)

**Example:**
```bash
curl http://localhost:8080/chunks/550e8400-e29b-41d4-a716-446655440000
```

**Errors:**
- `404` — chunk not found.

---

## DELETE /chunks/:id

Delete a memory chunk. Cascades to `memory_chunk_entities` join rows.

**Response:**
```json
{"deleted": true}
```

**Example:**
```bash
curl -X DELETE http://localhost:8080/chunks/550e8400-e29b-41d4-a716-446655440000
```

---

## POST /search

Full-text search across both entities (by name) and chunks (by content). Uses `LIKE %query%` against SQLite.

**Request body:**
```jsonc
{
  "query": "string",  // required — search term
  "limit": 10         // optional — max results per type, default 10
}
```

**Response:**
```jsonc
{
  "entities": [ /* Entity[] matching name */ ],
  "chunks":   [ /* MemoryChunk[] matching content */ ]
}
```

**Example:**
```bash
curl -X POST http://localhost:8080/search \
  -H "Content-Type: application/json" \
  -d '{"query": "Rust", "limit": 5}'
```

---

## DELETE /wipe

**Irreversible.** Delete all entities, relations, chunks, and join rows. Reloads the in-memory knowledge graph to an empty state.

**Required header:** `X-Confirm-Wipe: true`

**Response:**
```json
{"wiped": true}
```

**Example:**
```bash
curl -X DELETE http://localhost:8080/wipe \
  -H "X-Confirm-Wipe: true"
```

**Errors:**
- `400` — missing or incorrect `X-Confirm-Wipe` header.

---

## GET /stats

Returns aggregate counts for entities, chunks, graph nodes, graph edges, and server uptime.

**Response:**
```jsonc
{
  "entity_count": 42,
  "chunk_count": 17,
  "node_count": 42,        // in-memory graph nodes (should equal entity_count)
  "edge_count": 38,        // in-memory graph edges
  "uptime_seconds": 3600
}
```

**Example:**
```bash
curl http://localhost:8080/stats
```
