"""mnemo Python SDK — local-first AI memory layer."""

from .async_client import AsyncMnemoClient
from .client import MnemoClient
from .exceptions import (
    MnemoConnectionError,
    MnemoError,
    MnemoNotFoundError,
    MnemoServerError,
    MnemoValidationError,
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

__version__ = "0.1.0"

__all__ = [
    # Clients
    "MnemoClient",
    "AsyncMnemoClient",
    # Models
    "Entity",
    "Relation",
    "MemoryChunk",
    "IngestResponse",
    "RetrievalResult",
    "HealthStatus",
    "Stats",
    # Exceptions
    "MnemoError",
    "MnemoConnectionError",
    "MnemoNotFoundError",
    "MnemoServerError",
    "MnemoValidationError",
    # Version
    "__version__",
]
