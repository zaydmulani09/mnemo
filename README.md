# mnemo

> Local-first AI memory layer for any LLM. Persistent knowledge graph,
> entity extraction, semantic retrieval — no cloud required.

![Build Status](https://img.shields.io/github/actions/workflow/status/zaydmulani09/mnemo/ci.yml?branch=main)
![License](https://img.shields.io/badge/license-MIT-blue)
![Crates.io](https://img.shields.io/crates/v/mnemo-core)
![PyPI](https://img.shields.io/pypi/v/mnemo-sdk)
![Docker](https://img.shields.io/docker/pulls/zaydmulani09/mnemo)

---

## What is mnemo?

Most LLMs forget everything the moment a conversation ends. mnemo fixes that.

mnemo is a sidecar service that watches every conversation you feed it, extracts named entities and relationships using an LLM, builds a persistent knowledge graph in SQLite, and injects relevant context back into future prompts — automatically, in under 50ms. It works with **Ollama** (fully local, free), OpenAI, Anthropic, or any OpenAI-compatible API. It ships as a single static binary with zero cloud dependency.

---

## How it works

```
  your app
     │
     ▼
  POST /ingest ──► entity extraction (LLM) ──► knowledge graph (SQLite + petgraph)
                                                        │
  POST /retrieve ◄── scoring + ranking ◄── graph traversal + full-text search
     │
     ▼
  context_prompt  ──► inject into your LLM prompt
```

1. You POST raw text to `/ingest` (a conversation turn, a document, a note).
2. mnemo sends it to your configured LLM and extracts entities (people, tools, places, concepts) and the relationships between them.
3. Entities are deduplicated by name+type, aliases are merged, and everything is written to SQLite. The in-memory petgraph is updated atomically.
4. On POST `/retrieve`, mnemo runs a 6-stage pipeline: full-text chunk search → entity name search → graph expansion (BFS over the knowledge graph) → relation filter → score+rank → assemble a `context_prompt` string.
5. You inject `context_prompt` into your LLM's system prompt. Done.

---

## Quickstart

### Path A — Docker + Ollama (fully free, recommended)

```bash
git clone https://github.com/zaydmulani09/mnemo
cd mnemo
docker compose up -d

# Pull the llama3 model the first time (~4 GB)
docker exec mnemo-ollama ollama pull llama3

# Verify everything is healthy
curl http://localhost:8080/health
```

### Path B — Binary (Ollama or OpenAI running separately)

```bash
cargo install --path crates/mnemo-api

# With Ollama
export MNEMO_LLM_BASE_URL=http://localhost:11434/v1
mnemo-api

# With OpenAI
export MNEMO_LLM_BASE_URL=https://api.openai.com/v1
export MNEMO_LLM_API_KEY=sk-...
export MNEMO_LLM_MODEL=gpt-4o-mini
export MNEMO_LLM_PROVIDER=openai
mnemo-api
```

### Path C — Python SDK

```bash
pip install mnemo-sdk
```

```python
from mnemo import MnemoClient

client = MnemoClient()  # server at http://localhost:8080

# Store a memory
client.ingest("I'm building a Rust vector database called vecdb")

# Get context for injection into your next LLM prompt
print(client.get_context("what am I working on?"))
```

---

## API Reference

All endpoints accept and return `application/json`. Base URL: `http://localhost:8080`.

| Method | Path | Description | Request body | Response |
|--------|------|-------------|--------------|----------|
| `GET` | `/health` | Server + DB + LLM status | — | `HealthResponse` |
| `POST` | `/ingest` | Store text, extract entities | `IngestRequest` | `IngestResponse` |
| `POST` | `/retrieve` | Retrieve ranked memory context | `RetrievalQuery` | `RetrievalResult` |
| `GET` | `/entities` | List entities (paginated) | `?limit&offset` | `Entity[]` |
| `GET` | `/entities/:id` | Get entity by UUID | — | `Entity` |
| `DELETE` | `/entities/:id` | Delete entity (cascades) | — | `{"deleted":true}` |
| `GET` | `/entities/:id/neighbors` | Knowledge graph neighbors | `?depth` (max 5) | `GraphNode[]` |
| `GET` | `/chunks` | List memory chunks (paginated) | `?limit&offset&session_id` | `MemoryChunk[]` |
| `GET` | `/chunks/:id` | Get chunk by UUID | — | `MemoryChunk` |
| `DELETE` | `/chunks/:id` | Delete chunk | — | `{"deleted":true}` |
| `POST` | `/search` | Full-text search entities + chunks | `{"query","limit"}` | `{"entities","chunks"}` |
| `DELETE` | `/wipe` | Delete all memory (irreversible) | header: `X-Confirm-Wipe: true` | `{"wiped":true}` |
| `GET` | `/stats` | Entity/chunk/graph counts + uptime | — | `StatsResponse` |

**Key request/response types:**

```jsonc
// IngestRequest
{
  "content": "string",         // required — text to store
  "source":  "string",         // required — e.g. "chat", "email", "cli"
  "session_id": "string|null", // optional — group related chunks
  "metadata": {}               // optional — arbitrary JSON
}

// RetrievalQuery
{
  "text": "string",            // required — query text
  "session_id": "string|null", // optional — filter by session
  "max_chunks": 10,            // default 10
  "max_entities": 20,          // default 20
  "min_confidence": 0.5,       // default 0.5
  "include_graph": true,       // default true — expand via knowledge graph
  "graph_depth": 2             // default 2 — BFS depth for graph expansion
}
```

Full endpoint documentation with curl examples: [`docs/api.md`](docs/api.md)

---

## Configuration

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MNEMO_DB_PATH` | `mnemo.db` | SQLite database file path |
| `MNEMO_PORT` | `8080` | API server port |
| `MNEMO_LLM_BASE_URL` | `http://localhost:11434/v1` | OpenAI-compatible LLM base URL |
| `MNEMO_LLM_MODEL` | `llama3` | Model name for entity extraction |
| `MNEMO_LLM_API_KEY` | `ollama` | API key (any value works for Ollama) |
| `MNEMO_LLM_PROVIDER` | `ollama` | Provider type: `ollama`, `openai`, `anthropic`, `custom` |

### TOML config file

Pass `--config path/to/config.toml` to `mnemo-api`. See `mnemo.example.toml`:

```toml
db_path = "mnemo.db"
port = 8080

[llm]
provider = "ollama"
base_url = "http://localhost:11434/v1"
model = "llama3"
api_key = "ollama"
timeout_secs = 30
max_retries = 3
max_tokens = 2048
temperature = 0.1
```

Environment variables take precedence over TOML values. The active config source is reported in `GET /health` → `config_source`.

---

## CLI

Install:

```bash
cargo install --path crates/mnemo-cli
```

Usage:

```bash
# Store a memory
mnemo ingest "I use Neovim and prefer dark mode"

# Retrieve relevant context
mnemo search "what editor do I use?"

# List all extracted entities
mnemo entities

# Show entity detail + graph neighbors
mnemo entity <uuid> --neighbors

# List memory chunks
mnemo chunks

# Server health
mnemo health

# Memory statistics
mnemo stats

# Delete everything (prompts for confirmation)
mnemo wipe

# Skip confirmation prompt
mnemo wipe --yes

# Point at a non-default server
mnemo --server http://192.168.1.10:8080 stats
```

---

## Python SDK

Install:

```bash
pip install mnemo-sdk
```

See [`sdk/python/README.md`](sdk/python/README.md) for the full API reference.

**Async example:**

```python
import asyncio
from mnemo import AsyncMnemoClient

async def main():
    async with AsyncMnemoClient() as client:
        await client.ingest(
            "Alice is a principal engineer at Stripe working on payment infrastructure.",
            session_id="session-001",
        )
        context = await client.get_context(
            "what does Alice work on?",
            session_id="session-001",
        )
        print(context)

asyncio.run(main())
```

A working standalone example: [`examples/basic_usage.py`](examples/basic_usage.py)

---

## Architecture

Four Rust crates wired together:

| Crate | Type | Role |
|-------|------|------|
| `mnemo-core` | lib | Entity extraction, graph ops, retrieval engine, DB layer |
| `mnemo-api` | bin | Axum REST API — thin handler layer over mnemo-core |
| `mnemo-cli` | bin | CLI tool using blocking reqwest against the API |
| `mnemo-bench` | bin | Performance benchmarks (12 suites) |

Full architecture documentation: [`docs/architecture.md`](docs/architecture.md)

---

## Performance

Benchmarked on Apple M2, SQLite WAL mode, in-memory petgraph. Debug build numbers — release build (`--release`) is 3–5× faster.

| Operation | Avg latency | Throughput |
|-----------|-------------|------------|
| Entity insert (SQLite) | ~0.12 ms | ~8,300 ops/s |
| Entity lookup by ID | ~0.08 ms | ~12,500 ops/s |
| Chunk insert | ~0.14 ms | ~7,100 ops/s |
| Full-text chunk search | ~0.28 ms | ~3,500 ops/s |
| Graph neighbor (depth=1) | ~0.21 ms | ~4,700 ops/s |
| Graph neighbor (depth=2) | ~0.89 ms | ~1,100 ops/s |
| Full retrieval pipeline | ~4.2 ms | ~238 ops/s |

Run `cargo run -p mnemo-bench` to benchmark on your hardware.

---

## Testing

### Rust
```bash
cargo test --workspace          # run all 122 tests
make coverage                  # HTML coverage report (requires cargo-llvm-cov)
make coverage-summary          # summary to stdout
```

### Python SDK
```bash
cd sdk/python && pytest tests/ -v
```

### Benchmarks
```bash
cargo run -p mnemo-bench                    # all 12 benchmarks
cargo run -p mnemo-bench -- --filter graph  # graph benchmarks only
cargo run -p mnemo-bench -- --json out.json # save results to JSON
```

Current test counts: **122 Rust tests** · **21 Python tests** · **12 benchmarks**

---

## Contributing

PRs welcome. Please run `make fmt && make lint` before submitting.
Open an issue first for large changes.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for full setup instructions, code style guide, and how to add a new LLM provider.

---

## License

MIT — see [LICENSE](LICENSE)
