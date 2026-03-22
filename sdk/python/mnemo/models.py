"""Typed dataclasses mirroring the mnemo API response shapes."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional


@dataclass
class Entity:
    """A named entity extracted from memory."""

    id: str
    name: str
    entity_type: str
    aliases: list[str]
    attributes: dict[str, Any]
    confidence: float
    source_count: int
    created_at: str
    updated_at: str

    @classmethod
    def from_dict(cls, data: dict) -> "Entity":
        """Construct from a raw API response dict."""
        return cls(
            id=data["id"],
            name=data["name"],
            entity_type=data["entity_type"],
            aliases=data.get("aliases") or [],
            attributes=data.get("attributes") or {},
            confidence=float(data.get("confidence", 0.0)),
            source_count=int(data.get("source_count", 0)),
            created_at=data.get("created_at", ""),
            updated_at=data.get("updated_at", ""),
        )


@dataclass
class Relation:
    """A directed relationship between two entities."""

    id: str
    from_entity_id: str
    to_entity_id: str
    relation_type: str
    weight: float
    attributes: dict[str, Any]
    created_at: str
    updated_at: str

    @classmethod
    def from_dict(cls, data: dict) -> "Relation":
        """Construct from a raw API response dict."""
        return cls(
            id=data["id"],
            from_entity_id=data["from_entity_id"],
            to_entity_id=data["to_entity_id"],
            relation_type=data["relation_type"],
            weight=float(data.get("weight", 0.0)),
            attributes=data.get("attributes") or {},
            created_at=data.get("created_at", ""),
            updated_at=data.get("updated_at", ""),
        )


@dataclass
class MemoryChunk:
    """A chunk of text stored in memory."""

    id: str
    content: str
    source: str
    session_id: Optional[str]
    metadata: dict[str, Any]
    created_at: str
    embedding: Optional[list[float]] = None

    @classmethod
    def from_dict(cls, data: dict) -> "MemoryChunk":
        """Construct from a raw API response dict."""
        return cls(
            id=data["id"],
            content=data["content"],
            source=data["source"],
            session_id=data.get("session_id"),
            metadata=data.get("metadata") or {},
            created_at=data.get("created_at", ""),
            embedding=data.get("embedding"),
        )


@dataclass
class IngestResponse:
    """Response from POST /ingest."""

    chunk_id: str
    entities_extracted: int
    relations_extracted: int
    processing_time_ms: int

    @classmethod
    def from_dict(cls, data: dict) -> "IngestResponse":
        """Construct from a raw API response dict."""
        return cls(
            chunk_id=data["chunk_id"],
            entities_extracted=int(data.get("entities_extracted", 0)),
            relations_extracted=int(data.get("relations_extracted", 0)),
            processing_time_ms=int(data.get("processing_time_ms", 0)),
        )


@dataclass
class RetrievalResult:
    """Response from POST /retrieve."""

    chunks: list[MemoryChunk]
    entities: list[Entity]
    relations: list[Relation]
    context_prompt: str
    retrieved_at: str

    @classmethod
    def from_dict(cls, data: dict) -> "RetrievalResult":
        """Construct from a raw API response dict."""
        return cls(
            chunks=[MemoryChunk.from_dict(c) for c in data.get("chunks", [])],
            entities=[Entity.from_dict(e) for e in data.get("entities", [])],
            relations=[Relation.from_dict(r) for r in data.get("relations", [])],
            context_prompt=data.get("context_prompt", ""),
            retrieved_at=data.get("retrieved_at", ""),
        )


@dataclass
class HealthStatus:
    """Response from GET /health."""

    status: str
    version: str
    db_connected: bool
    llm_reachable: bool
    entity_count: int
    chunk_count: int
    uptime_seconds: int
    provider_type: str
    provider_model: str
    config_source: str

    @classmethod
    def from_dict(cls, data: dict) -> "HealthStatus":
        """Construct from a raw API response dict."""
        return cls(
            status=data.get("status", ""),
            version=data.get("version", ""),
            db_connected=bool(data.get("db_connected", False)),
            llm_reachable=bool(data.get("llm_reachable", False)),
            entity_count=int(data.get("entity_count", 0)),
            chunk_count=int(data.get("chunk_count", 0)),
            uptime_seconds=int(data.get("uptime_seconds", 0)),
            provider_type=data.get("provider_type", ""),
            provider_model=data.get("provider_model", ""),
            config_source=data.get("config_source", ""),
        )


@dataclass
class Stats:
    """Response from GET /stats."""

    entity_count: int
    chunk_count: int
    node_count: int
    edge_count: int
    uptime_seconds: int

    @classmethod
    def from_dict(cls, data: dict) -> "Stats":
        """Construct from a raw API response dict."""
        return cls(
            entity_count=int(data.get("entity_count", 0)),
            chunk_count=int(data.get("chunk_count", 0)),
            node_count=int(data.get("node_count", 0)),
            edge_count=int(data.get("edge_count", 0)),
            uptime_seconds=int(data.get("uptime_seconds", 0)),
        )
