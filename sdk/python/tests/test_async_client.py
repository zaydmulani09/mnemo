"""Tests for the async AsyncMnemoClient using respx."""

import httpx
import pytest
import respx

from mnemo.async_client import AsyncMnemoClient
from mnemo.exceptions import MnemoConnectionError, MnemoNotFoundError
from mnemo.models import IngestResponse, RetrievalResult

BASE = "http://localhost:8080"

INGEST_PAYLOAD = {
    "chunk_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    "entities_extracted": 2,
    "relations_extracted": 1,
    "processing_time_ms": 55,
}

RETRIEVE_PAYLOAD = {
    "chunks": [
        {
            "id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "content": "Hello from async!",
            "source": "python-sdk",
            "session_id": None,
            "metadata": {},
            "created_at": "2024-01-01T00:00:00Z",
        }
    ],
    "entities": [],
    "relations": [],
    "context_prompt": "RELEVANT MEMORIES:\n- Hello from async!",
    "retrieved_at": "2024-01-01T00:00:01Z",
}


@respx.mock
async def test_async_ingest_success():
    respx.post(f"{BASE}/ingest").mock(
        return_value=httpx.Response(200, json=INGEST_PAYLOAD)
    )
    async with AsyncMnemoClient(base_url=BASE) as client:
        result = await client.ingest("Hello from async!")
    assert isinstance(result, IngestResponse)
    assert result.chunk_id == "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
    assert result.entities_extracted == 2


@respx.mock
async def test_async_retrieve_success():
    respx.post(f"{BASE}/retrieve").mock(
        return_value=httpx.Response(200, json=RETRIEVE_PAYLOAD)
    )
    async with AsyncMnemoClient(base_url=BASE) as client:
        result = await client.retrieve("what did I say?")
    assert isinstance(result, RetrievalResult)
    assert "Hello from async!" in result.context_prompt
    assert len(result.chunks) == 1


@respx.mock
async def test_async_not_found_raises():
    respx.get(f"{BASE}/entities/deadbeef-dead-dead-dead-deaddeadbeef").mock(
        return_value=httpx.Response(404, json={"error": "not found"})
    )
    async with AsyncMnemoClient(base_url=BASE) as client:
        with pytest.raises(MnemoNotFoundError):
            await client.get_entity("deadbeef-dead-dead-dead-deaddeadbeef")


@respx.mock
async def test_async_connection_error():
    respx.post(f"{BASE}/ingest").mock(side_effect=httpx.ConnectError("refused"))
    client = AsyncMnemoClient(base_url=BASE)
    try:
        with pytest.raises(MnemoConnectionError):
            await client.ingest("test")
    finally:
        await client.close()


@respx.mock
async def test_async_context_manager():
    """Context manager enters, executes, and closes cleanly."""
    respx.get(f"{BASE}/health").mock(
        return_value=httpx.Response(
            200,
            json={
                "status": "ok",
                "version": "0.1.0",
                "db_connected": True,
                "llm_reachable": True,
                "entity_count": 0,
                "chunk_count": 0,
                "uptime_seconds": 10,
                "provider_type": "Ollama",
                "provider_model": "llama3",
                "config_source": "env",
            },
        )
    )
    async with AsyncMnemoClient(base_url=BASE) as client:
        health = await client.health()
    assert health.status == "ok"
    # After __aexit__, the underlying client should be closed
    assert client._client.is_closed
