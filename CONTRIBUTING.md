# Contributing to mnemo

Thank you for your interest in contributing. This document covers development setup, code style, how to add a new LLM provider, and the PR process.

---

## Development setup

### Prerequisites

- Rust 1.78+ (`rustup update stable`)
- Python 3.10+ (for SDK development)
- Ollama (optional, needed for live extraction tests)
- Docker + Compose (optional, for integration runs)

### Clone and build

```bash
git clone https://github.com/zaydmulani09/mnemo
cd mnemo

# Build all Rust crates
cargo build --workspace

# Install the Python SDK with dev extras
pip install -e "sdk/python[dev]"
```

### Run the server locally

```bash
# With Ollama running at localhost:11434
cargo run -p mnemo-api

# With a custom config file
cargo run -p mnemo-api -- --config mnemo.example.toml

# With env vars
MNEMO_LLM_BASE_URL=https://api.openai.com/v1 \
MNEMO_LLM_API_KEY=sk-... \
MNEMO_LLM_MODEL=gpt-4o-mini \
cargo run -p mnemo-api
```

---

## Running tests

```bash
# All Rust tests (122 tests, no server needed)
make test
# or:
cargo test --workspace

# Python SDK tests (21 tests, no server needed)
make sdk-test
# or:
cd sdk/python && pytest tests/ -v

# Both together
make all-tests

# Coverage report (requires cargo-llvm-cov)
make coverage
```

All tests run against in-memory SQLite with a stub LLM (no real Ollama/OpenAI calls). You do not need any external services to run the test suite.

---

## Code style

### Rust

```bash
# Format
make fmt
# or: cargo fmt --all

# Lint (zero warnings required)
make lint
# or: cargo clippy --workspace -- -D warnings
```

- All public types must have `#[derive(Debug)]`.
- Use `tracing::info!` / `tracing::warn!` / `tracing::error!` — not `println!`.
- Error types go in `error.rs`. Use `MnemoError` variants; do not add `anyhow::Error` to public APIs.
- New async tests use `#[tokio::test]`. New pure-logic tests use `#[test]`.

### Python (SDK)

```bash
# Format + lint
cd sdk/python
ruff check mnemo/ tests/
```

- Use type annotations on all public functions.
- All new methods on `MnemoClient` must have a matching method on `AsyncMnemoClient`.
- Add tests in `sdk/python/tests/` using `requests_mock` (sync) or `respx` (async).

---

## Adding a new LLM provider

mnemo's provider abstraction lives in `crates/mnemo-core/src/provider.rs`. To add a new provider (e.g. `Gemini`):

### Step 1 — Add a `ProviderType` variant

```rust
pub enum ProviderType {
    Ollama,
    OpenAi,
    Anthropic,
    Custom,
    Gemini,   // ← add here
}
```

### Step 2 — Implement the trait methods

In the `impl ProviderType` block, extend each match arm:

```rust
pub fn default_base_url(&self) -> &'static str {
    match self {
        // ...existing arms...
        Self::Gemini => "https://generativelanguage.googleapis.com/v1beta",
    }
}

pub fn default_model(&self) -> &'static str {
    match self {
        // ...existing arms...
        Self::Gemini => "gemini-1.5-flash",
    }
}

pub fn requires_api_key(&self) -> bool {
    match self {
        // ...existing arms...
        Self::Gemini => true,
    }
}
```

### Step 3 — Handle the request format in `build_request()`

If Gemini uses a different HTTP format than OpenAI-compatible chat completions, add a match arm:

```rust
fn build_request(&self, system: &str, user: &str) -> RequestBuilder {
    match &self.config.provider {
        // ...existing arms...
        ProviderType::Gemini => {
            // build Gemini-specific request format
        }
    }
}
```

### Step 4 — Handle response parsing in `complete()`

Add a match arm for extracting the generated text from the Gemini response JSON.

### Step 5 — Parse the new variant from env / TOML

In `ProviderType`'s `FromStr` or deserialization impl, map `"gemini"` → `ProviderType::Gemini`.

### Step 6 — Update docs

- Add a row to the `MNEMO_LLM_PROVIDER` table in `README.md`.
- Add the provider to the `LLM provider abstraction` section in `docs/architecture.md`.

### Step 7 — Add tests

In `crates/mnemo-core/src/provider.rs`, add:
- A `test_provider_type_gemini_url` test checking `default_base_url()`.
- A `test_llm_config_gemini_constructor` test if you add a `LlmConfig::gemini()` constructor.

---

## Commit style

mnemo uses [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add Gemini provider support
fix: clamp relation weight correctly on concurrent upserts
docs: expand architecture.md with scoring algorithm detail
test: add property-based tests for score_chunk
refactor: extract build_request into ProviderType method
chore: update proptest to 1.5
```

Subject line: imperative mood, ≤72 characters, no trailing period.  
Body (optional): explain *why*, not *what*.

---

## PR process

1. **Fork** the repo and create a branch from `main`:
   ```bash
   git checkout -b feat/gemini-provider
   ```

2. **Make your changes.** Keep PRs focused — one feature or fix per PR.

3. **Run the full test suite before pushing:**
   ```bash
   make fmt && make lint && make all-tests
   ```
   Zero warnings required. All 122 Rust tests + 21 Python tests must pass.

4. **Open a PR** against `main`. The PR description should explain:
   - What the change does and why
   - Any deviations from existing patterns
   - How you tested it

5. **For large changes** (new crate, new endpoint, breaking model change), open an issue first to discuss the approach before writing code.

6. All CI checks must pass before merge. The CI runs `cargo build`, `cargo test --workspace`, `cargo clippy -- -D warnings`, and `pytest tests/ -v`.
