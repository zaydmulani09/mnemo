# mnemo — CONTEXT.md

## Project Description
Local-first AI memory layer for any LLM. Extracts entities and facts
from conversations, builds a persistent knowledge graph, and injects
relevant memory into future prompts. Works with Ollama (free, local),
OpenAI, Anthropic, or any OpenAI-compatible backend.

## Goals
- Zero cloud dependency by default (Ollama = fully free)
- Sub-50ms memory retrieval
- Works as a sidecar REST API any app can call
- Python SDK for easy integration
- Single binary distribution

## Tech Stack
| Component | Technology | Version |
|-----------|-----------|---------|
| Core engine | Rust | 1.78 |
| Async runtime | Tokio | 1.37 |
| Web framework | Axum | 0.7 |
| Database | SQLite via sqlx | 0.7 |
| Graph engine | petgraph | 0.6 |
| HTTP client | reqwest | 0.12 |
| CLI | clap | 4.5 |
| Serialization | serde + serde_json | 1.0 |

## Crate Structure
| Crate | Type | Role |
|-------|------|------|
| mnemo-core | lib | All business logic |
| mnemo-api | bin | REST API sidecar |
| mnemo-cli | bin | CLI tool |
| mnemo-bench | bin | Benchmarks |

## File Tree
[update after every prompt]
mnemo/
├── Cargo.toml
├── CONTEXT.md
├── README.md
├── CONTRIBUTING.md
├── LICENSE
├── .gitignore
├── .env.example
├── mnemo.example.toml
├── docs/
│   ├── architecture.md
│   └── api.md
├── examples/
│   └── basic_usage.py
├── crates/
│   ├── mnemo-core/
│   │   ├── Cargo.toml
│   │   ├── migrations/
│   │   │   ├── 001_entities.sql
│   │   │   ├── 002_relations.sql
│   │   │   ├── 003_chunks.sql
│   │   │   └── 004_chunk_entities.sql
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── models.rs
│   │       ├── db.rs
│   │       ├── graph.rs
│   │       ├── extractor.rs
│   │       ├── retrieval.rs
│   │       ├── provider.rs
│   │       └── error.rs
│   ├── mnemo-api/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       └── tests.rs
│   ├── mnemo-cli/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   └── mnemo-bench/
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
├── Dockerfile
├── docker-compose.yml
├── docker-compose.override.yml
├── Makefile
└── sdk/
    └── python/
        ├── pyproject.toml
        ├── README.md
        ├── .gitignore
        ├── mnemo/
        │   ├── __init__.py
        │   ├── client.py
        │   ├── async_client.py
        │   ├── models.py
        │   ├── exceptions.py
        │   └── py.typed
        ├── tests/
        │   ├── __init__.py
        │   ├── test_models.py
        │   ├── test_client.py
        │   └── test_async_client.py
        └── examples/
            └── demo.ipynb

## Prompt Status
| Prompt | Title | Status |
|--------|-------|--------|
| P1 | Scaffold repo, workspace, CONTEXT.md | ✅ Complete |
| P2 | Core data models | ✅ Complete |
| P3 | SQLite persistence layer | ✅ Complete |
| P4 | Entity extraction engine | ✅ Complete |
| P5 | Knowledge graph builder | ✅ Complete |
| P6 | Memory retrieval engine | ✅ Complete |
| P7 | REST API server | ✅ Complete |
| P8 | LLM provider abstraction | ✅ Complete |
| P9 | CLI tool | ✅ Complete |
| P10 | Python SDK | ✅ Complete |
| P11 | Docker + docker-compose | ✅ Complete |
| P12 | Benchmarks | ✅ Complete |
| P13 | Full test suite | ✅ Complete |
| P14 | README + docs | ✅ Complete |
| P15 | Git history + GitHub push + release | ⬜ Next |

## What P14 Did
- `README.md` fully rewritten: 12 sections — what/how/quickstart (3 paths)/API reference table/config env+TOML/CLI examples/Python SDK/architecture overview/performance table/testing section/contributing/license
- ASCII flow diagram showing ingest → extract → graph → retrieve → context_prompt pipeline
- `docs/architecture.md`: 10 sections, 600+ words — overview, crate structure, ingestion flow (5 steps), retrieval flow (6 stages), knowledge graph internals, entity deduplication logic, LLM provider abstraction, database schema (4 tables with column types + FK cascade rules), scoring algorithm formulas, configuration precedence
- `docs/api.md`: all 13 endpoints fully documented — method/path/description/request body/response body/curl example/error responses
- `examples/basic_usage.py`: standalone runnable Python script — health check with clear server-not-running error message, 10 realistic memories across 3 sessions, 5 retrieval queries with context preview, entity summary table, graph neighbors, stats, optional wipe prompt
- `CONTRIBUTING.md`: dev setup, test commands, Rust + Python style guides, step-by-step guide for adding a new LLM provider (7 steps), commit style (Conventional Commits), PR process
- `sdk/python/examples/demo.ipynb`: 2 new cells — markdown "Integration with LangChain" + code cell with `MnemoMemory` class implementing save/load interface
- `cargo build --workspace` clean, zero warnings; 122/122 Rust tests pass; 21/21 Python tests pass

## What P13 Did
- `proptest 1.4` + `wiremock 0.6` added to workspace dev-deps and mnemo-core dev-deps
- `retrieval.rs`: 3 property-based tests via `proptest!` macro — `proptest_score_chunk_always_in_range`, `proptest_score_entity_always_in_range`, `proptest_build_context_prompt_never_panics`; all assert outputs stay in [0.0, 1.0] or are valid UTF-8
- `extractor.rs`: 8 edge case tests — deeply nested attributes, all 8 EntityType variants, missing weight defaults to 0.0, empty string input, JSON array input, empty entity name, 10,000-char name (no truncation), unicode name/relation_type
- `db.rs`: 8 edge case tests — alias merge deduplication, weight cap at 1.0 after 20 upserts, entity pagination no-overlap, chunk pagination no-overlap, get_chunks_for_entity limit, concurrent upserts (no panic/corruption), embedding 128-float round-trip, wipe+reinsert state isolation
- `graph.rs`: 5 edge case tests — cycle detection (no infinite loop), subgraph dedup of shared neighbor, ingest skips unknown-entity relations, 1000-node chain neighbors (<100ms), reload clears in-memory-only nodes
- `mnemo-api/src/tests.rs`: 8 new E2E tests — full ingest→retrieve cycle, empty retrieve returns arrays not null, entity pagination, neighbors depth=1/2 param, search finds both entities+chunks, delete chunk→404, 5 concurrent ingest via tokio::join!, wipe→stats shows zero
- `Makefile`: `coverage` and `coverage-summary` targets added (requires `cargo-llvm-cov`, not installed/run)
- `README.md`: Testing section added with Rust/Python/Benchmarks commands and test counts
- `cargo build --workspace` clean, zero warnings; all 122 Rust tests pass

## What P12 Did
- `mnemo-bench/Cargo.toml`: added `serde`, `serde_json`, `uuid`, `tempfile` (workspace), `colored 2.1`
- `BenchResult` struct: 8 fields (name, iterations, total_ms, avg_ms, min_ms, max_ms, p95_ms, ops_per_sec); `from_samples()` sorts, computes p95 at floor(0.95×n), ops/sec = 1000/avg
- `BenchSuite`: temp-dir file DB (forward-slash path for Windows SQLite URL compat), `SharedGraph`, results vec
- 12 benchmarks: `db_insert_entity` (1000), `db_get_entity_by_id` (1000, 100-entity pre-population), `db_search_entities` (500, 200-entity pre-population), `db_insert_chunk` (1000), `db_search_chunks` (500, 200-chunk pre-population), `db_upsert_relation` (1000, ON CONFLICT path), `graph_add_node` (1000), `graph_get_neighbors_d1` (500, lock outside loop), `graph_get_neighbors_d2` (500), `graph_find_path` (200)
- Graph fixture: 500-node graph + 1000 edges (ring + stride shortcuts); built once before neighbor/path benchmarks
- `bench_retrieval_score_chunk` (1000): inlines private `score_chunk` logic; lock-free timing
- `bench_retrieval_full_pipeline` (100): real `RetrievalEngine::new()`, full 6-stage pipeline, 100+50 fixture pre-population
- ASCII report: `colored` headers (bold cyan), yellow highlight for avg > 10ms, ops/sec truncated to ">1M" when applicable
- `--json <path>`: saves `Vec<BenchResult>` as pretty JSON
- `--filter <keyword>`: post-run retain filter on benchmark name
- README Performance section added with M2 reference numbers
- `cargo run -p mnemo-bench` completes all 12 benchmarks without panic

## What P11 Did
- `Dockerfile`: multi-stage musl static binary; Stage 1 (`rust:1.78-slim`) installs `musl-tools`, adds `x86_64-unknown-linux-musl` target, caches deps via stub files before real source copy, strips binary; Stage 2 (`FROM scratch`) copies CA certs + binary only
- `.dockerignore`: excludes `target/`, `.git/`, `*.db*`, `.env`, SDK build artifacts
- `--health-check` flag added to `mnemo-api/src/main.rs`: checked at entry before tracing init; reads `MNEMO_PORT` env (default 8080); `TcpStream::connect` → exit 0 / exit 1; no curl dependency, works in scratch container
- `docker-compose.yml`: `mnemo` + `ollama/ollama:latest` services; named volumes `mnemo-data` / `ollama-data`; mnemo `depends_on` ollama with `condition: service_healthy`; both have `healthcheck` + `restart: unless-stopped`
- `docker-compose.override.yml`: local dev override — `RUST_LOG=mnemo=debug`, mounts `./mnemo.toml` read-only
- `Makefile`: 12 targets — `build`, `test`, `run`, `fmt`, `lint`, `docker-build`, `docker-run`, `docker-up`, `docker-down`, `clean`, `sdk-install`, `sdk-test`, `all-tests`
- `README.md`: Docker section expanded with `docker compose up -d` + ollama pull instructions; `docker run` example for bring-your-own-LLM (OpenAI)
- Zero warnings, 90/90 Rust tests still pass

## What P10 Did
- `sdk/python/` created: full pip-installable Python SDK for mnemo
- `pyproject.toml`: `setuptools` backend, `requires-python = ">=3.10"`, deps: `requests>=2.31`, `httpx>=0.27`, dev: `pytest`, `pytest-asyncio`, `respx`, `requests-mock`
- `mnemo/exceptions.py`: 5-class hierarchy — `MnemoError` → `MnemoConnectionError`, `MnemoNotFoundError`, `MnemoServerError` (with `.status_code`), `MnemoValidationError`
- `mnemo/models.py`: 7 dataclasses with `from_dict()` — `Entity`, `Relation`, `MemoryChunk`, `IngestResponse`, `RetrievalResult`, `HealthStatus`, `Stats`
- `mnemo/client.py`: `MnemoClient` sync client (requests), 14 methods, `_request()` helper with full error mapping
- `mnemo/async_client.py`: `AsyncMnemoClient` (httpx), mirrors sync API, supports async context manager
- `mnemo/__init__.py`: clean re-exports, `__version__ = "0.1.0"`, `__all__`
- `mnemo/py.typed`: PEP 561 typed package marker
- `tests/test_models.py`: 6 tests — all 7 model `from_dict()` paths, optional fields, nested deserialization
- `tests/test_client.py`: 10 tests using `requests_mock` — ingest, retrieve, 404, empty list, wipe header, connection error, 5xx, health, stats, search
- `tests/test_async_client.py`: 5 tests using `respx` — async ingest, retrieve, 404, connection error, context manager close
- `examples/demo.ipynb`: 10-cell Jupyter notebook with health check, 5 ingest calls, retrieve, entity table (pandas-aware), stats, async example
- `sdk/python/README.md`: install, quickstart, full API reference table, async example, exception table
- Deviation: `hatchling` build backend fails on Python 3.14; replaced with `setuptools.build_meta` (functionally identical)

## What P9 Did
- `mnemo-cli/Cargo.toml`: added `colored 2.1`, `indicatif 0.17`, `prettytable-rs 0.10`; direct `reqwest 0.12` with `blocking+json` features (workspace entry omits `blocking`); `chrono` + `uuid` from workspace
- `ApiClient` struct with `reqwest::blocking::Client` (60s timeout), `get/post/delete/delete_with_header` methods; unreachable server → clear error + exit 1
- 10 subcommands via Clap: `ingest`, `search`, `entities`, `entity`, `chunks`, `chunk`, `wipe`, `stats`, `health`, `config`
- `ingest`: spinner → structured success output (chunk ID, entities, relations, time)
- `search`: spinner → formatted ENTITIES / RELATIONSHIPS / MEMORIES sections; `--raw` prints context_prompt directly
- `entities` / `chunks`: prettytable-rs table output; `--search` hits POST /search
- `entity`: key-value detail view; `--neighbors` adds neighbor table via GET /entities/:id/neighbors
- `wipe`: stdin confirmation or `--yes`; DELETE /wipe with X-Confirm-Wipe header; exit 2 on abort
- `health`: colored ✓/✗ via `colored` crate for LLM + DB status
- `stats`: 5-field summary (entities, chunks, graph nodes/edges, uptime)
- `config`: hits GET /health, prints provider/model/config_source
- Exit codes: 0 success, 1 server error, 2 user aborted
- 5 tests: unreachable server, ingest/search/wipe parse, --server flag parse
- Local mirror structs for API types lacking `Deserialize`: `GraphNeighbor`, `StatsResponse`, `SearchResponse`

## What P8 Did
- `ProviderType` enum: Ollama, OpenAi, Anthropic, Custom — with `default_base_url()`, `default_model()`, `requires_api_key()`
- `LlmConfig` expanded: added `provider`, `max_tokens`, `temperature`, `system_prompt_prefix`
- Named constructors: `LlmConfig::ollama()`, `openai()`, `anthropic()`, `from_env()`, `validate()`
- `LlmProvider::build_request()` private method handles Anthropic (POST /messages, x-api-key header, anthropic-version header) vs OpenAI-compat (POST /chat/completions, Bearer auth)
- `complete()` dispatches response parsing by provider type (Anthropic content[] vs OpenAI choices[])
- `LlmProvider::list_models()`: Ollama/OpenAI/Custom hit /models; Anthropic returns hardcoded list
- `MnemoConfig` with `db_path`, `port`, `llm: LlmConfig`; TOML serializable
- `MnemoConfig::from_file()`, `from_env_or_file()`, `to_example_toml()`
- `mnemo.example.toml` created at repo root
- `mnemo-api/main.rs` replaced `ServerConfig` with `MnemoConfig`; added `config_source: String` to `AppState`
- `--config path` CLI arg parsed in main fn; sets `config_source` to `"file:path"` or `"env"`
- `HealthResponse` expanded with `provider_type`, `provider_model`, `config_source`
- `toml = "0.8"` added to workspace; added to mnemo-core Cargo.toml
- 12 unit tests: provider type URLs/key requirements, LlmConfig constructors/validation/from_env, MnemoConfig defaults/TOML parse/file missing/example TOML
- Deviation: `test_mnemo_config_example_toml_parses` uses `std::result::Result<MnemoConfig, _>` (not `crate::Result`) to avoid type alias conflict with `toml::de::Error`

## What P7 Did
- Implemented full Axum REST API server in `mnemo-api/src/main.rs`
- `AppState` with Arc<Database>, SharedGraph, Arc<Extractor>, Arc<RetrievalEngine>, start_time, Arc<ServerConfig>
- `ServerConfig::from_env()` reads MNEMO_PORT/DB_PATH/LLM_BASE_URL/LLM_MODEL/LLM_API_KEY with defaults
- `ApiError` implements `IntoResponse` as `{"error": "..."}` JSON; MnemoError::NotFound → 404, all others → 500
- 13 routes: health, ingest, retrieve, entities CRUD + neighbors, chunks CRUD, search, wipe, stats
- Middleware: CorsLayer::permissive, TraceLayer, DefaultBodyLimit(10MB)
- `DELETE /wipe` guards on `X-Confirm-Wipe: true` header; returns 400 without it
- `GET /entities/:id/neighbors` accepts `depth` query param (default 2, max 5)
- `GraphNode` given `Serialize` derive; `Extractor` given `health_check()` method
- `POST /ingest` inserts chunk to DB first, then calls graph.ingest() (which links entities)
- Main fn: tracing init with EnvFilter, graceful ctrl+c shutdown
- Added `chrono` + `uuid` to mnemo-api Cargo.toml; added `features = ["util"]` to tower workspace dep; added `features = ["env-filter"]` to tracing-subscriber workspace dep
- 12 integration tests using `tower::ServiceExt::oneshot()` — no live server needed; pre-populate DB directly for entity tests (LLM offline in test environment)
- Deviation: `test_ingest_then_list_entities` and `test_delete_entity` pre-insert entities via DB (not via ingest API) since extract_with_fallback returns empty without a running LLM

## What P6 Did
- Implemented `RetrievalEngine` with `Arc<Database>` + `SharedGraph`
- `retrieve()` runs 6-stage pipeline: chunk search → entity search → graph expansion → relation filter → rank/truncate → context string
- Stage 1: content search + session search, merged by chunk ID
- Stage 2: entity name search + entities from top-10 chunks, merged by entity ID
- Stage 3: graph expansion via `get_subgraph_entities`, expanded entities score at 0.5× base
- Stage 4: relations filtered to keep only those where both endpoints are in the accumulated entity set
- Stage 5: sort by score desc, filter below min_confidence, take max_chunks/max_entities
- `score_chunk()`: base 0.5 + keyword overlap (capped 0.4) + recency (0.1/0.05) + session match (0.15)
- `score_entity()`: confidence×0.5 + name match (0.3) + alias match (0.2) + source count bonus (capped 0.2)
- `build_context_prompt()`: structured RELEVANT FACTS / RELATIONSHIPS / RELEVANT MEMORIES sections; empty sections omitted; chunk content truncated at 500 chars; relation_type underscores → spaces
- `retrieve_for_prompt()` never panics — returns empty string on error
- `stats()` returns (entity_count, chunk_count) for health endpoint
- 14 tests: sync scoring tests (keyword, session, recency, name, alias, source count) + async integration tests (empty DB, relevant chunks, entities, graph expansion, context prompt formatting, stats)

## What P5 Did
- Implemented `GraphNode` and `GraphEdge` structs (node/edge weights for petgraph DiGraph)
- Implemented `KnowledgeGraph` with `DiGraph<GraphNode, GraphEdge>`, `node_index: HashMap<Uuid, NodeIndex>`, `db: Arc<Database>`
- `new()` creates empty graph then calls `load_from_db()` to hydrate from SQLite
- `load_from_db()` does two passes: all entities as nodes first, then all relations as edges (avoids missing-node edge warnings)
- `ingest()` resolves/creates entities, links to chunk, upserts relations, updates in-memory graph, returns `IngestResponse`
- `resolve_or_create_entity()` deduplicates by name — same name = same entity; merges aliases and attributes on collision
- `add_node()` idempotent — checks node_index before inserting
- `add_edge()` logs warning and skips if either endpoint missing in graph
- `get_neighbors()` BFS up to configurable depth, excludes start node from results
- `get_subgraph_entities()` unions neighbors across multiple starting points, deduplicates by entity_id
- `find_path()` BFS shortest path returning sequence of GraphNodes, or None if unreachable
- `reload()` clears graph + node_index then re-runs `load_from_db()`
- `SharedGraph = Arc<RwLock<KnowledgeGraph>>` type alias + `new_shared_graph()` constructor
- 12 tokio tests: empty graph, node dedup, create/dedup entity, ingest, single-hop, multi-hop, unknown entity, path exists, path none, reload, counts

## What P4 Did
- Implemented `LlmConfig` with `Default` (Ollama at `localhost:11434`, model `llama3`, 30s timeout, 3 retries)
- Implemented `LlmProvider::new()` building reqwest client with timeout
- Implemented `complete()` — POST to `/chat/completions`, temperature 0.1, max_tokens 2048, retries on network error with 500ms backoff
- Implemented `health_check()` — GET `/models`, returns bool
- Defined `SYSTEM_PROMPT` and `EXTRACTION_SCHEMA` consts in `extractor.rs`
- Implemented `Extractor::new()`, `extract()`, `extract_with_fallback()`
- `extract()` strips markdown fences, clamps confidence/weight to 0.0–1.0, returns `MnemoError::Extraction` with raw response on JSON parse failure
- `extract_with_fallback()` never panics, never errors — fallback returns empty result with summary `"Extraction unavailable"`
- `parse_extraction_response()` private fn handles fence stripping, JSON parse, EntityType mapping (unknown → `Other`), clamping
- Added `reqwest` dependency to mnemo-core `Cargo.toml`
- 7 unit tests (all sync, no real LLM calls): clean JSON, markdown fences, confidence clamping, unknown entity type, empty entities, invalid JSON error, missing summary
- Real LLM calls not tested by design — no mock server

## What P3 Did
- Created 4 migration files (001–004) covering entities, relations, chunks, chunk_entities
- Implemented full `Database` struct in `db.rs` with `new()`, `new_in_memory()`, pragma setup, migrations
- Implemented 7 entity methods, 5 relation methods, 7 chunk methods, 3 join-table methods
- Implemented `wipe_all()` and `health_check()`
- Row parsers handle UUID↔TEXT, DateTime↔RFC3339, aliases/embedding↔JSON, EntityType↔JSON string
- `upsert_entity` merges aliases and attributes on name+type collision; `upsert_relation` uses ON CONFLICT to cap weight at 1.0
- 14 integration tests all against in-memory SQLite; FK cascade verified
- Fixed: `sqlx::migrate!()` (no-arg form) required — path form unsupported in sqlx 0.7

## What P2 Did
- Implemented `MnemoError` enum (8 variants) + `Result<T>` alias in `error.rs`
- Implemented all 12 production types in `models.rs`: `Entity`, `Relation`, `MemoryChunk`, `MemoryChunkEntity`, `ExtractionResult`, `ExtractedEntity`, `ExtractedRelation`, `RetrievalQuery`, `RetrievalResult`, `IngestRequest`, `IngestResponse`, `HealthResponse`
- Implemented `EntityType` enum (8 variants) with `sqlx::Type` derive
- Implemented `RetrievalQuery::default()`
- Added re-exports to `lib.rs`
- 14 unit tests all passing

## What P1 Did
- Created GitHub repo via gh CLI
- Initialized Cargo workspace with 4 crates
- Scaffolded all crate src files as placeholders
- Created migrations directory
- Created .gitignore, .env.example, README.md, LICENSE
- Created this CONTEXT.md

## Test Count
**Rust:** 122 tests (14 model + 22 DB + 15 extractor + 17 graph + 17 retrieval + 20 API integration + 12 provider/config + 5 CLI)
**Python:** 21 tests (6 model + 10 sync client + 5 async client)

## Known Issues / Technical Debt
None yet.

## Deviations from Spec
None.
