"""Tests for the synchronous MnemoClient using requests_mock."""

import pytest
import requests_mock as req_mock_module

from mnemo.client import MnemoClient
from mnemo.exceptions import (
    MnemoConnectionError,
    MnemoNotFoundError,
    MnemoServerError,
)
from mnemo.models import HealthStatus, IngestResponse, RetrievalResult, Stats

BASE = "http://localhost:8080"

HEALTH_PAYLOAD = {
    "status": "ok",
    "version": "0.1.0",
    "db_connected": True,
    "llm_reachable": True,
    "entity_count": 5,
    "chunk_count": 3,
    "uptime_seconds": 120,
    "provider_type": "Ollama",
    "provider_model": "llama3",
    "config_source": "env",
}

INGEST_PAYLOAD = {
    "chunk_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    "entities_extracted": 2,
    "relations_extracted": 1,
    "processing_time_ms": 87,
}

RETRIEVE_PAYLOAD = {
    "chunks": [
        {
            "id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "content": "I am building a vector database.",
            "source": "python-sdk",
            "session_id": None,
            "metadata": {},
            "created_at": "2024-01-01T00:00:00Z",
        }
    ],
    "entities": [],
    "relations": [],
    "context_prompt": "RELEVANT MEMORIES:\n- I am building a vector database.",
    "retrieved_at": "2024-01-01T00:00:01Z",
}

STATS_PAYLOAD = {
    "entity_count": 10,
    "chunk_count": 5,
    "node_count": 10,
    "edge_count": 8,
    "uptime_seconds": 300,
}

ENTITY_PAYLOAD = {
    "id": "cccccccc-cccc-cccc-cccc-cccccccccccc",
    "name": "vecdb",
    "entity_type": "Tool",
    "aliases": [],
    "attributes": {},
    "confidence": 0.9,
    "source_count": 1,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z",
}


def test_ingest_success(requests_mock):
    requests_mock.post(f"{BASE}/ingest", json=INGEST_PAYLOAD)
    client = MnemoClient(base_url=BASE)
    result = client.ingest("I am building a vector database called vecdb")
    assert isinstance(result, IngestResponse)
    assert result.chunk_id == "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
    assert result.entities_extracted == 2
    assert result.relations_extracted == 1
    assert result.processing_time_ms == 87


def test_retrieve_success(requests_mock):
    requests_mock.post(f"{BASE}/retrieve", json=RETRIEVE_PAYLOAD)
    client = MnemoClient(base_url=BASE)
    result = client.retrieve("what am I building?")
    assert isinstance(result, RetrievalResult)
    assert "vector database" in result.context_prompt
    assert len(result.chunks) == 1
    assert result.chunks[0].content == "I am building a vector database."


def test_get_entity_not_found(requests_mock):
    requests_mock.get(
        f"{BASE}/entities/deadbeef-dead-dead-dead-deaddeadbeef",
        status_code=404,
        json={"error": "not found"},
    )
    client = MnemoClient(base_url=BASE)
    with pytest.raises(MnemoNotFoundError):
        client.get_entity("deadbeef-dead-dead-dead-deaddeadbeef")


def test_list_entities_empty(requests_mock):
    requests_mock.get(f"{BASE}/entities", json=[])
    client = MnemoClient(base_url=BASE)
    result = client.list_entities()
    assert result == []


def test_wipe_success(requests_mock):
    requests_mock.delete(f"{BASE}/wipe", json={"wiped": True})
    client = MnemoClient(base_url=BASE)
    result = client.wipe()
    assert result is True
    # Verify header was sent
    assert requests_mock.last_request.headers.get("X-Confirm-Wipe") == "true"


def test_connection_error():
    client = MnemoClient(base_url="http://localhost:1")
    with pytest.raises(MnemoConnectionError):
        client.health()


def test_server_error(requests_mock):
    requests_mock.get(
        f"{BASE}/health",
        status_code=500,
        json={"error": "internal server error"},
    )
    client = MnemoClient(base_url=BASE)
    with pytest.raises(MnemoServerError) as exc_info:
        client.health()
    assert exc_info.value.status_code == 500


def test_health_success(requests_mock):
    requests_mock.get(f"{BASE}/health", json=HEALTH_PAYLOAD)
    client = MnemoClient(base_url=BASE)
    result = client.health()
    assert isinstance(result, HealthStatus)
    assert result.status == "ok"
    assert result.db_connected is True
    assert result.provider_model == "llama3"


def test_stats_success(requests_mock):
    requests_mock.get(f"{BASE}/stats", json=STATS_PAYLOAD)
    client = MnemoClient(base_url=BASE)
    result = client.stats()
    assert isinstance(result, Stats)
    assert result.entity_count == 10
    assert result.node_count == 10
    assert result.edge_count == 8


def test_search_success(requests_mock):
    requests_mock.post(
        f"{BASE}/search",
        json={"entities": [ENTITY_PAYLOAD], "chunks": []},
    )
    client = MnemoClient(base_url=BASE)
    result = client.search("vecdb")
    assert "entities" in result
    assert "chunks" in result
    assert len(result["entities"]) == 1
    assert result["entities"][0].name == "vecdb"
    assert result["chunks"] == []
