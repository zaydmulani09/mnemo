"""Tests for mnemo model dataclasses (no server required)."""

import pytest
from mnemo.models import (
    Entity,
    HealthStatus,
    IngestResponse,
    MemoryChunk,
    Relation,
    RetrievalResult,
    Stats,
)


ENTITY_DICT = {
    "id": "11111111-1111-1111-1111-111111111111",
    "name": "Rust programming language",
    "entity_type": "Tool",
    "aliases": ["Rust", "rustlang"],
    "attributes": {"paradigm": "systems"},
    "confidence": 0.95,
    "source_count": 3,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-02T00:00:00Z",
}

CHUNK_DICT = {
    "id": "22222222-2222-2222-2222-222222222222",
    "content": "I use Rust and Python daily.",
    "source": "conversation",
    "session_id": None,
    "metadata": {"turn": 1},
    "created_at": "2024-01-01T00:00:00Z",
    "embedding": None,
}

RELATION_DICT = {
    "id": "33333333-3333-3333-3333-333333333333",
    "from_entity_id": "11111111-1111-1111-1111-111111111111",
    "to_entity_id": "44444444-4444-4444-4444-444444444444",
    "relation_type": "uses",
    "weight": 0.8,
    "attributes": {},
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z",
}


def test_entity_from_dict():
    entity = Entity.from_dict(ENTITY_DICT)
    assert entity.id == "11111111-1111-1111-1111-111111111111"
    assert entity.name == "Rust programming language"
    assert entity.entity_type == "Tool"
    assert entity.aliases == ["Rust", "rustlang"]
    assert entity.attributes == {"paradigm": "systems"}
    assert entity.confidence == pytest.approx(0.95)
    assert entity.source_count == 3
    assert entity.created_at == "2024-01-01T00:00:00Z"
    assert entity.updated_at == "2024-01-02T00:00:00Z"


def test_memory_chunk_optional_session():
    chunk = MemoryChunk.from_dict(CHUNK_DICT)
    assert chunk.id == "22222222-2222-2222-2222-222222222222"
    assert chunk.content == "I use Rust and Python daily."
    assert chunk.source == "conversation"
    assert chunk.session_id is None
    assert chunk.metadata == {"turn": 1}
    assert chunk.embedding is None

    # Also test with session_id present
    with_session = dict(CHUNK_DICT, session_id="sess-42")
    chunk2 = MemoryChunk.from_dict(with_session)
    assert chunk2.session_id == "sess-42"


def test_retrieval_result_from_dict():
    data = {
        "chunks": [CHUNK_DICT],
        "entities": [ENTITY_DICT],
        "relations": [RELATION_DICT],
        "context_prompt": "RELEVANT FACTS:\n- Rust is a systems language.",
        "retrieved_at": "2024-01-01T00:00:00Z",
    }
    result = RetrievalResult.from_dict(data)
    assert len(result.chunks) == 1
    assert len(result.entities) == 1
    assert len(result.relations) == 1
    assert result.context_prompt == "RELEVANT FACTS:\n- Rust is a systems language."
    assert result.retrieved_at == "2024-01-01T00:00:00Z"
    assert result.entities[0].name == "Rust programming language"
    assert result.chunks[0].content == "I use Rust and Python daily."
    assert result.relations[0].relation_type == "uses"


def test_health_status_from_dict():
    data = {
        "status": "ok",
        "version": "0.1.0",
        "db_connected": True,
        "llm_reachable": False,
        "entity_count": 42,
        "chunk_count": 17,
        "uptime_seconds": 3600,
        "provider_type": "Ollama",
        "provider_model": "llama3",
        "config_source": "env",
    }
    health = HealthStatus.from_dict(data)
    assert health.status == "ok"
    assert health.version == "0.1.0"
    assert health.db_connected is True
    assert health.llm_reachable is False
    assert health.entity_count == 42
    assert health.chunk_count == 17
    assert health.uptime_seconds == 3600
    assert health.provider_type == "Ollama"
    assert health.provider_model == "llama3"
    assert health.config_source == "env"


def test_ingest_response_from_dict():
    data = {
        "chunk_id": "55555555-5555-5555-5555-555555555555",
        "entities_extracted": 3,
        "relations_extracted": 2,
        "processing_time_ms": 145,
    }
    resp = IngestResponse.from_dict(data)
    assert resp.chunk_id == "55555555-5555-5555-5555-555555555555"
    assert resp.entities_extracted == 3
    assert resp.relations_extracted == 2
    assert resp.processing_time_ms == 145


def test_stats_from_dict():
    data = {
        "entity_count": 100,
        "chunk_count": 50,
        "node_count": 100,
        "edge_count": 75,
        "uptime_seconds": 7200,
    }
    stats = Stats.from_dict(data)
    assert stats.entity_count == 100
    assert stats.chunk_count == 50
    assert stats.node_count == 100
    assert stats.edge_count == 75
    assert stats.uptime_seconds == 7200
