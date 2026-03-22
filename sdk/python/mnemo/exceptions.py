"""Exception hierarchy for the mnemo Python SDK."""


class MnemoError(Exception):
    """Base exception for all mnemo SDK errors."""
    pass


class MnemoConnectionError(MnemoError):
    """Cannot connect to mnemo server."""
    pass


class MnemoNotFoundError(MnemoError):
    """Requested resource not found (404)."""
    pass


class MnemoServerError(MnemoError):
    """Server returned 5xx error."""

    def __init__(self, status_code: int, message: str) -> None:
        self.status_code = status_code
        super().__init__(f"Server error {status_code}: {message}")


class MnemoValidationError(MnemoError):
    """Invalid request parameters."""
    pass
