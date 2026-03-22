"""Synchronous client for the mnemo memory API."""

from __future__ import annotations

from typing import Any, Optional

import requests
import requests.exceptions

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


class MnemoClient:
    """
    Synchronous client for the mnemo memory API.

    Example::

        client = MnemoClient()
        client.ingest("I am building a Rust vector database called vecdb")
        result = client.retrieve("what projects am I working on?")
        print(result.context_prompt)
    """

    def __init__(
        self,
        base_url: str = "http://localhost:8080",
        timeout: int = 60,
    ) -> None:
        """
        Create a new MnemoClient.

        Args:
            base_url: Base URL of the running mnemo API server.
            timeout:  Request timeout in seconds.
        """
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.session = requests.Session()
        self.session.headers.update({"Content-Type": "application/json"})

    # ── Private helpers ──────────────────────────────────────────────────────

    def _request(
        self,
        method: str,
        path: str,
        json: Optional[Any] = None,
        headers: Optional[dict] = None,
    ) -> dict:
        """
        Execute an HTTP request and return the parsed JSON body.

        Raises:
            MnemoConnectionError: Server unreachable.
            MnemoNotFoundError:   404 response.
            MnemoServerError:     5xx response.
            MnemoError:           Any other non-2xx response.
        """
        url = f"{self.base_url}{path}"
        try:
            response = self.session.request(
                method,
                url,
                json=json,
                headers=headers,
                timeout=self.timeout,
            )
        except requests.exceptions.ConnectionError as exc:
            raise MnemoConnectionError(
                f"Cannot connect to mnemo server at {self.base_url}: {exc}"
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

        if not response.ok:
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

    def ingest(
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
            session_id: Optional session identifier for grouping memories.
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
        data = self._request("POST", "/ingest", json=body)
        return IngestResponse.from_dict(data)

    def retrieve(
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
        data = self._request("POST", "/retrieve", json=body)
        return RetrievalResult.from_dict(data)

    def get_context(
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
            Formatted context string (context_prompt field from RetrievalResult).
        """
        return self.retrieve(text, session_id=session_id).context_prompt

    def list_entities(self, limit: int = 50, offset: int = 0) -> list[Entity]:
        """
        List stored entities.

        Args:
            limit:  Maximum number of entities to return.
            offset: Pagination offset.

        Returns:
            List of Entity objects.
        """
        data = self._request("GET", f"/entities?limit={limit}&offset={offset}")
        return [Entity.from_dict(e) for e in data]

    def get_entity(self, entity_id: str) -> Entity:
        """
        Get a single entity by ID.

        Args:
            entity_id: UUID string of the entity.

        Returns:
            Entity object.

        Raises:
            MnemoNotFoundError: If the entity does not exist.
        """
        data = self._request("GET", f"/entities/{entity_id}")
        return Entity.from_dict(data)

    def delete_entity(self, entity_id: str) -> bool:
        """
        Delete an entity by ID.

        Args:
            entity_id: UUID string of the entity.

        Returns:
            True on success.
        """
        data = self._request("DELETE", f"/entities/{entity_id}")
        return bool(data.get("deleted", False))

    def get_neighbors(self, entity_id: str, depth: int = 2) -> list[dict]:
        """
        Get graph neighbors of an entity.

        Args:
            entity_id: UUID string of the starting entity.
            depth:     Traversal depth (max 5).

        Returns:
            List of neighbor dicts with entity_id, name, entity_type, confidence.
        """
        data = self._request("GET", f"/entities/{entity_id}/neighbors?depth={depth}")
        return data  # type: ignore[return-value]

    def list_chunks(
        self,
        limit: int = 50,
        offset: int = 0,
        session_id: Optional[str] = None,
    ) -> list[MemoryChunk]:
        """
        List stored memory chunks.

        Args:
            limit:      Maximum number of chunks to return.
            offset:     Pagination offset.
            session_id: Optional session filter.

        Returns:
            List of MemoryChunk objects.
        """
        path = f"/chunks?limit={limit}&offset={offset}"
        if session_id:
            path += f"&session_id={session_id}"
        data = self._request("GET", path)
        return [MemoryChunk.from_dict(c) for c in data]

    def get_chunk(self, chunk_id: str) -> MemoryChunk:
        """
        Get a single memory chunk by ID.

        Args:
            chunk_id: UUID string of the chunk.

        Returns:
            MemoryChunk object.

        Raises:
            MnemoNotFoundError: If the chunk does not exist.
        """
        data = self._request("GET", f"/chunks/{chunk_id}")
        return MemoryChunk.from_dict(data)

    def delete_chunk(self, chunk_id: str) -> bool:
        """
        Delete a memory chunk by ID.

        Args:
            chunk_id: UUID string of the chunk.

        Returns:
            True on success.
        """
        data = self._request("DELETE", f"/chunks/{chunk_id}")
        return bool(data.get("deleted", False))

    def search(self, query: str, limit: int = 10) -> dict:
        """
        Search entities and chunks by text.

        Args:
            query: Search query string.
            limit: Maximum results per category.

        Returns:
            Dict with "entities" (list of Entity) and "chunks" (list of MemoryChunk).
        """
        body = {"query": query, "limit": limit}
        data = self._request("POST", "/search", json=body)
        return {
            "entities": [Entity.from_dict(e) for e in data.get("entities", [])],
            "chunks": [MemoryChunk.from_dict(c) for c in data.get("chunks", [])],
        }

    def wipe(self) -> bool:
        """
        Wipe all memory (entities, chunks, relations, graph).

        **Warning:** This is irreversible.

        Returns:
            True on success.
        """
        data = self._request(
            "DELETE", "/wipe", headers={"X-Confirm-Wipe": "true"}
        )
        return bool(data.get("wiped", False))

    def stats(self) -> Stats:
        """
        Get memory statistics.

        Returns:
            Stats with entity/chunk/graph counts and uptime.
        """
        data = self._request("GET", "/stats")
        return Stats.from_dict(data)

    def health(self) -> HealthStatus:
        """
        Get server health status.

        Returns:
            HealthStatus with server, DB, and LLM reachability info.
        """
        data = self._request("GET", "/health")
        return HealthStatus.from_dict(data)
