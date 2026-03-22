"""Async client for the mnemo memory API (httpx-based)."""

from __future__ import annotations

from typing import Any, Optional

import httpx

from .exceptions import (
    MnemoConnectionError,
    MnemoError,
    MnemoNotFoundError,
    MnemoServerError,
)
from .models import (
    Entity,
    HealthStatus,
    IngestResponse,
    MemoryChunk,
    Relation,
    RetrievalResult,
    Stats,
)


class AsyncMnemoClient:
    """
    Async client for the mnemo memory API.

    Example::

        async with AsyncMnemoClient() as client:
            await client.ingest("Hello from async!")
            result = await client.retrieve("what did I say?")
            print(result.context_prompt)

    Or without context manager::

        client = AsyncMnemoClient()
        await client.ingest("some text")
        await client.close()
    """

    def __init__(
        self,
        base_url: str = "http://localhost:8080",
        timeout: int = 60,
    ) -> None:
        """
        Create a new AsyncMnemoClient.

        Args:
            base_url: Base URL of the running mnemo API server.
            timeout:  Request timeout in seconds.
        """
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self._client = httpx.AsyncClient(
            timeout=httpx.Timeout(timeout),
            headers={"Content-Type": "application/json"},
        )

    async def __aenter__(self) -> "AsyncMnemoClient":
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.close()

    async def close(self) -> None:
        """Close the underlying HTTP client."""
        await self._client.aclose()

    # ── Private helpers ──────────────────────────────────────────────────────

    async def _request(
        self,
        method: str,
        path: str,
        json: Optional[Any] = None,
        headers: Optional[dict] = None,
    ) -> dict:
        """
        Execute an async HTTP request and return the parsed JSON body.

        Raises:
            MnemoConnectionError: Server unreachable.
            MnemoNotFoundError:   404 response.
            MnemoServerError:     5xx response.
            MnemoError:           Any other non-2xx response.
        """
        url = f"{self.base_url}{path}"
        try:
            response = await self._client.request(
                method,
                url,
                json=json,
                headers=headers,
            )
        except httpx.ConnectError as exc:
            raise MnemoConnectionError(
                f"Cannot connect to mnemo server at {self.base_url}: {exc}"
            ) from exc
        except httpx.TimeoutException as exc:
            raise MnemoConnectionError(
                f"Request to mnemo server timed out: {exc}"
            ) from exc

        if response.status_code == 404:
            raise MnemoNotFoundError(f"Resource not found: {path}")

        if response.status_code >= 500:
            msg = response.text
            try:
                msg = response.json().get("error", msg)
            except Exception:
                pass
            raise MnemoServerError(response.status_code, msg)

        if not response.is_success:
            msg = response.text
            try:
                msg = response.json().get("error", msg)
            except Exception:
                pass
            raise MnemoError(f"Request failed {response.status_code}: {msg}")

        if response.content:
            return response.json()
        return {}

    # ── Public API ───────────────────────────────────────────────────────────

    async def ingest(
        self,
        content: str,
        source: str = "python-sdk",
        session_id: Optional[str] = None,
        metadata: Optional[dict] = None,
    ) -> IngestResponse:
        """
        Ingest text into memory.

        Args:
            content:    Text to store and extract entities from.
            source:     Source label (default: "python-sdk").
            session_id: Optional session identifier.
            metadata:   Optional arbitrary metadata dict.

        Returns:
            IngestResponse with chunk_id, entity/relation counts, and timing.
        """
        body: dict[str, Any] = {
            "content": content,
            "source": source,
            "session_id": session_id,
            "metadata": metadata,
        }
        data = await self._request("POST", "/ingest", json=body)
        return IngestResponse.from_dict(data)

    async def retrieve(
        self,
        text: str,
        session_id: Optional[str] = None,
        max_chunks: int = 10,
        max_entities: int = 20,
        min_confidence: float = 0.5,
        include_graph: bool = True,
        graph_depth: int = 2,
    ) -> RetrievalResult:
        """
        Retrieve relevant memory context for a query.

        Args:
            text:           Query text.
            session_id:     Restrict retrieval to a specific session.
            max_chunks:     Maximum memory chunks to return.
            max_entities:   Maximum entities to return.
            min_confidence: Minimum entity confidence threshold.
            include_graph:  Whether to expand via the knowledge graph.
            graph_depth:    Depth of graph expansion.

        Returns:
            RetrievalResult with chunks, entities, relations, and context_prompt.
        """
        body = {
            "text": text,
            "session_id": session_id,
            "max_chunks": max_chunks,
            "max_entities": max_entities,
            "min_confidence": min_confidence,
            "include_graph": include_graph,
            "graph_depth": graph_depth,
        }
        data = await self._request("POST", "/retrieve", json=body)
        return RetrievalResult.from_dict(data)

    async def get_context(
        self,
        text: str,
        session_id: Optional[str] = None,
    ) -> str:
        """
        Retrieve memory context as a plain string ready to inject into a prompt.

        Args:
            text:       Query text.
            session_id: Optional session filter.

        Returns:
            Formatted context string.
        """
        result = await self.retrieve(text, session_id=session_id)
        return result.context_prompt

    async def list_entities(self, limit: int = 50, offset: int = 0) -> list[Entity]:
        """List stored entities."""
        data = await self._request("GET", f"/entities?limit={limit}&offset={offset}")
        return [Entity.from_dict(e) for e in data]

    async def get_entity(self, entity_id: str) -> Entity:
        """
        Get a single entity by ID.

        Raises:
            MnemoNotFoundError: If the entity does not exist.
        """
        data = await self._request("GET", f"/entities/{entity_id}")
        return Entity.from_dict(data)

    async def delete_entity(self, entity_id: str) -> bool:
        """Delete an entity by ID. Returns True on success."""
        data = await self._request("DELETE", f"/entities/{entity_id}")
        return bool(data.get("deleted", False))

    async def get_neighbors(self, entity_id: str, depth: int = 2) -> list[dict]:
        """Get graph neighbors of an entity."""
        data = await self._request(
            "GET", f"/entities/{entity_id}/neighbors?depth={depth}"
        )
        return data  # type: ignore[return-value]

    async def list_chunks(
        self,
        limit: int = 50,
        offset: int = 0,
        session_id: Optional[str] = None,
    ) -> list[MemoryChunk]:
        """List stored memory chunks."""
        path = f"/chunks?limit={limit}&offset={offset}"
        if session_id:
            path += f"&session_id={session_id}"
        data = await self._request("GET", path)
        return [MemoryChunk.from_dict(c) for c in data]

    async def get_chunk(self, chunk_id: str) -> MemoryChunk:
        """
        Get a single memory chunk by ID.

        Raises:
            MnemoNotFoundError: If the chunk does not exist.
        """
        data = await self._request("GET", f"/chunks/{chunk_id}")
        return MemoryChunk.from_dict(data)

    async def delete_chunk(self, chunk_id: str) -> bool:
        """Delete a memory chunk by ID. Returns True on success."""
        data = await self._request("DELETE", f"/chunks/{chunk_id}")
        return bool(data.get("deleted", False))

    async def search(self, query: str, limit: int = 10) -> dict:
        """
        Search entities and chunks by text.

        Returns:
            Dict with "entities" and "chunks" lists.
        """
        body = {"query": query, "limit": limit}
        data = await self._request("POST", "/search", json=body)
        return {
            "entities": [Entity.from_dict(e) for e in data.get("entities", [])],
            "chunks": [MemoryChunk.from_dict(c) for c in data.get("chunks", [])],
        }

    async def wipe(self) -> bool:
        """Wipe all memory. **Warning:** irreversible."""
        data = await self._request(
            "DELETE", "/wipe", headers={"X-Confirm-Wipe": "true"}
        )
        return bool(data.get("wiped", False))

    async def stats(self) -> Stats:
        """Get memory statistics."""
        data = await self._request("GET", "/stats")
        return Stats.from_dict(data)

    async def health(self) -> HealthStatus:
        """Get server health status."""
        data = await self._request("GET", "/health")
        return HealthStatus.from_dict(data)
