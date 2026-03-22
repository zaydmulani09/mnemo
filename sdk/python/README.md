# mnemo Python SDK

Python client for **mnemo** — a local-first AI memory layer that extracts entities from conversations, builds a knowledge graph, and injects relevant memory into LLM prompts.

> **mnemo server must be running.** See the [main README](../../README.md) for server setup.

---

## Install

```bash
pip install mnemo-sdk
```

## Quickstart

```python
from mnemo import MnemoClient

client = MnemoClient()  # default: http://localhost:8080

# Store a memory
client.ingest("I am a software engineer at Acme Corp building a vector database in Rust.")

# Retrieve relevant context as a string
context = client.get_context("what is the user working on?")
print(context)
# → RELEVANT MEMORIES:
# → - I am a software engineer at Acme Corp building a vector database in Rust.
```

---

## Full API Reference

### `MnemoClient(base_url, timeout)`

| Argument   | Type  | Default                    | Description            |
|------------|-------|----------------------------|------------------------|
| `base_url` | `str` | `"http://localhost:8080"`  | mnemo server URL       |
| `timeout`  | `int` | `60`                       | Request timeout (secs) |

### Methods

| Method | Arguments | Returns | Description |
|--------|-----------|---------|-------------|
| `ingest(content, source, session_id, metadata)` | `str, str="python-sdk", Optional[str], Optional[dict]` | `IngestResponse` | Store text; extract entities |
| `retrieve(text, session_id, max_chunks, max_entities, min_confidence, include_graph, graph_depth)` | `str, …` | `RetrievalResult` | Full retrieval with entities + relations |
| `get_context(text, session_id)` | `str, Optional[str]` | `str` | Context string ready for prompt injection |
| `list_entities(limit, offset)` | `int=50, int=0` | `list[Entity]` | Paginated entity list |
| `get_entity(entity_id)` | `str` | `Entity` | Single entity by UUID |
| `delete_entity(entity_id)` | `str` | `bool` | Delete entity |
| `get_neighbors(entity_id, depth)` | `str, int=2` | `list[dict]` | Knowledge graph neighbors |
| `list_chunks(limit, offset, session_id)` | `int=50, int=0, Optional[str]` | `list[MemoryChunk]` | Paginated chunk list |
| `get_chunk(chunk_id)` | `str` | `MemoryChunk` | Single chunk by UUID |
| `delete_chunk(chunk_id)` | `str` | `bool` | Delete chunk |
| `search(query, limit)` | `str, int=10` | `dict` | Search entities + chunks by text |
| `wipe()` | — | `bool` | **Irreversible.** Delete all memory |
| `stats()` | — | `Stats` | Entity/chunk/graph counts + uptime |
| `health()` | — | `HealthStatus` | Server + DB + LLM status |

---

## Async Usage

Use `AsyncMnemoClient` for async frameworks (FastAPI, LangChain async, etc.):

```python
import asyncio
from mnemo import AsyncMnemoClient

async def main():
    async with AsyncMnemoClient() as client:
        await client.ingest("I prefer Neovim and dark mode.")
        context = await client.get_context("what are the user's preferences?")
        print(context)

asyncio.run(main())
```

`AsyncMnemoClient` has identical method signatures — all are `async def`.

---

## Models

| Class | Key Fields |
|-------|-----------|
| `Entity` | `id, name, entity_type, aliases, confidence, source_count` |
| `Relation` | `id, from_entity_id, to_entity_id, relation_type, weight` |
| `MemoryChunk` | `id, content, source, session_id, metadata, created_at` |
| `IngestResponse` | `chunk_id, entities_extracted, relations_extracted, processing_time_ms` |
| `RetrievalResult` | `chunks, entities, relations, context_prompt, retrieved_at` |
| `HealthStatus` | `status, db_connected, llm_reachable, provider_type, provider_model` |
| `Stats` | `entity_count, chunk_count, node_count, edge_count, uptime_seconds` |

All models have a `from_dict(data: dict)` classmethod for constructing from raw API responses.

---

## Exceptions

| Exception | When raised |
|-----------|-------------|
| `MnemoError` | Base class for all SDK errors |
| `MnemoConnectionError` | Server unreachable |
| `MnemoNotFoundError` | 404 — resource does not exist |
| `MnemoServerError` | 5xx — server-side error (has `.status_code`) |
| `MnemoValidationError` | Invalid request parameters |

```python
from mnemo import MnemoClient
from mnemo.exceptions import MnemoConnectionError, MnemoNotFoundError

client = MnemoClient()
try:
    entity = client.get_entity("some-uuid")
except MnemoNotFoundError:
    print("Entity not found")
except MnemoConnectionError:
    print("Is the mnemo server running?")
```

---

## Development

```bash
# Clone the repo and install with dev extras
pip install -e ".[dev]"

# Run tests (no server needed)
pytest tests/ -v
```
