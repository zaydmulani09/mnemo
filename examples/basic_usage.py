"""
mnemo basic usage example
=========================

Demonstrates the full mnemo Python SDK workflow:
  - health check
  - ingesting memories across multiple sessions
  - retrieving context for various queries
  - listing and inspecting entities
  - graph neighbor traversal
  - stats
  - optional wipe

Prerequisites:
  1. Start the mnemo server:
       cargo run -p mnemo-api
     or:
       docker compose up -d

  2. Install the SDK:
       pip install mnemo-sdk
     or (from repo root):
       pip install -e sdk/python

Run:
  python examples/basic_usage.py
"""

import sys

try:
    from mnemo import MnemoClient
    from mnemo.exceptions import MnemoConnectionError, MnemoNotFoundError
except ImportError:
    print("ERROR: mnemo SDK not installed.")
    print("       Run: pip install mnemo-sdk")
    print("       Or:  pip install -e sdk/python  (from repo root)")
    sys.exit(1)


MEMORIES = [
    # session-work
    {
        "content": "I am a software engineer at Acme Corp working on backend infrastructure.",
        "source": "chat",
        "session_id": "session-work",
    },
    {
        "content": "I use Rust and Python daily — Rust for systems code, Python for ML pipelines.",
        "source": "chat",
        "session_id": "session-work",
    },
    {
        "content": (
            "My current project is a vector database called vecdb built in Rust "
            "using HNSW indexing for approximate nearest-neighbour search."
        ),
        "source": "chat",
        "session_id": "session-work",
    },
    {
        "content": (
            "My team at Acme uses GitHub for source control, Linear for project management, "
            "and Slack for communication."
        ),
        "source": "chat",
        "session_id": "session-work",
    },
    # session-prefs
    {
        "content": "I prefer dark mode everywhere. My terminal is Wezterm with fish shell.",
        "source": "settings",
        "session_id": "session-prefs",
    },
    {
        "content": "I edit code in Neovim with LazyVim. I never use VS Code.",
        "source": "settings",
        "session_id": "session-prefs",
    },
    {
        "content": "My keyboard is a ZSA Moonlander with Gateron Yellow switches.",
        "source": "settings",
        "session_id": "session-prefs",
    },
    # session-personal
    {
        "content": "I live in San Francisco and commute by bicycle.",
        "source": "profile",
        "session_id": "session-personal",
    },
    {
        "content": "I am learning Japanese. I have been studying for about six months.",
        "source": "profile",
        "session_id": "session-personal",
    },
    {
        "content": (
            "I maintain an open-source CLI tool called mnemo that gives LLMs persistent memory. "
            "It is written in Rust and has a Python SDK."
        ),
        "source": "profile",
        "session_id": "session-personal",
    },
]

QUERIES = [
    ("what programming languages do I use?", None),
    ("what editor do I use?", "session-prefs"),
    ("what am I building at work?", "session-work"),
    ("tell me about my open source projects", None),
    ("what city do I live in?", "session-personal"),
]


def separator(title: str) -> None:
    width = 60
    print(f"\n{'─' * width}")
    print(f"  {title}")
    print(f"{'─' * width}")


def step_1_health(client: MnemoClient) -> bool:
    separator("1. Health check")
    health = client.health()
    print(f"  status:        {health.status}")
    print(f"  version:       {health.version}")
    print(f"  db_connected:  {health.db_connected}")
    print(f"  llm_reachable: {health.llm_reachable}")
    print(f"  provider:      {health.provider_type} / {health.provider_model}")
    print(f"  uptime:        {health.uptime_seconds}s")

    if health.status != "ok" or not health.db_connected:
        print("\n  ERROR: server is not healthy. Check server logs.")
        return False
    if not health.llm_reachable:
        print("\n  WARNING: LLM is unreachable — entity extraction will be skipped.")
        print("           Memories will still be stored as text chunks.")
    return True


def step_2_ingest(client: MnemoClient) -> None:
    separator("2. Ingesting 10 memories across 3 sessions")
    for mem in MEMORIES:
        result = client.ingest(
            mem["content"],
            source=mem["source"],
            session_id=mem["session_id"],
        )
        session_label = mem["session_id"] or "—"
        print(
            f"  ✓ [{session_label:<18}] "
            f"entities={result.entities_extracted:2d}  "
            f"relations={result.relations_extracted:2d}  "
            f"{result.processing_time_ms}ms"
        )


def step_3_retrieve(client: MnemoClient) -> None:
    separator("3. Retrieving context for 5 queries")
    for query_text, session_id in QUERIES:
        result = client.retrieve(
            query_text,
            session_id=session_id,
            max_chunks=3,
            max_entities=5,
            min_confidence=0.0,
            include_graph=True,
            graph_depth=2,
        )
        print(f"\n  Query: \"{query_text}\"")
        if session_id:
            print(f"  Session filter: {session_id}")
        print(
            f"  → {len(result.chunks)} chunks, "
            f"{len(result.entities)} entities, "
            f"{len(result.relations)} relations"
        )
        if result.context_prompt:
            # Show first 300 chars of context_prompt
            preview = result.context_prompt[:300].replace("\n", "\n    ")
            print(f"  Context preview:\n    {preview}{'...' if len(result.context_prompt) > 300 else ''}")
        else:
            print("  (no context returned)")


def step_4_entities(client: MnemoClient) -> str | None:
    """List entities, print summary table, return ID of most-referenced entity."""
    separator("4. Entities extracted from memory")
    entities = client.list_entities(limit=50)
    if not entities:
        print("  (no entities yet — LLM may be offline)")
        return None

    # Sort by source_count descending
    entities.sort(key=lambda e: e.source_count, reverse=True)

    col_name = 30
    col_type = 14
    print(
        f"  {'Name':<{col_name}} {'Type':<{col_type}} {'Conf':>5} {'Refs':>4}"
    )
    print(f"  {'─' * col_name} {'─' * col_type} {'─' * 5} {'─' * 4}")
    for e in entities[:20]:  # show top 20
        name = e.name[:col_name - 1] if len(e.name) >= col_name else e.name
        etype = e.entity_type[:col_type - 1] if len(e.entity_type) >= col_type else e.entity_type
        print(
            f"  {name:<{col_name}} {etype:<{col_type}} "
            f"{e.confidence:>5.2f} {e.source_count:>4}"
        )
    if len(entities) > 20:
        print(f"  ... and {len(entities) - 20} more")

    top_entity = entities[0]
    print(f"\n  Most-referenced entity: \"{top_entity.name}\" (refs={top_entity.source_count})")
    return str(top_entity.id)


def step_5_neighbors(client: MnemoClient, entity_id: str) -> None:
    separator("5. Knowledge graph neighbors")
    try:
        entity = client.get_entity(entity_id)
        print(f"  Entity: \"{entity.name}\" ({entity.entity_type})")
        neighbors = client.get_neighbors(entity_id, depth=2)
        if neighbors:
            print(f"  Neighbors (depth=2):")
            for n in neighbors:
                print(f"    → {n['name']} ({n['entity_type']})")
        else:
            print("  No neighbors found (graph may be sparse if LLM was offline).")
    except MnemoNotFoundError:
        print(f"  Entity {entity_id} not found.")


def step_6_stats(client: MnemoClient) -> None:
    separator("6. Memory statistics")
    stats = client.stats()
    print(f"  entities:    {stats.entity_count}")
    print(f"  chunks:      {stats.chunk_count}")
    print(f"  graph nodes: {stats.node_count}")
    print(f"  graph edges: {stats.edge_count}")
    print(f"  uptime:      {stats.uptime_seconds}s")


def step_7_wipe(client: MnemoClient) -> None:
    separator("7. Optional wipe")
    print("  This will delete ALL memories from the server.")
    try:
        answer = input("  Wipe all memories? [y/N] ").strip().lower()
    except (EOFError, KeyboardInterrupt):
        answer = "n"

    if answer == "y":
        client.wipe()
        print("  ✓ All memories wiped.")
        stats = client.stats()
        print(f"  entity_count={stats.entity_count} chunk_count={stats.chunk_count}")
    else:
        print("  Skipped.")


def main() -> None:
    print("\nmnemo basic_usage.py")
    print("====================")
    print("Connecting to http://localhost:8080 ...")

    client = MnemoClient(base_url="http://localhost:8080", timeout=60)

    # 1. Health
    try:
        if not step_1_health(client):
            sys.exit(1)
    except MnemoConnectionError:
        print("\nERROR: Cannot connect to mnemo server at http://localhost:8080")
        print("\nStart the server with one of:")
        print("  cargo run -p mnemo-api")
        print("  docker compose up -d")
        sys.exit(1)

    # 2. Ingest
    step_2_ingest(client)

    # 3. Retrieve
    step_3_retrieve(client)

    # 4. Entities
    top_entity_id = step_4_entities(client)

    # 5. Neighbors
    if top_entity_id:
        step_5_neighbors(client, top_entity_id)

    # 6. Stats
    step_6_stats(client)

    # 7. Optional wipe
    step_7_wipe(client)

    print("\nDone.\n")


if __name__ == "__main__":
    main()
