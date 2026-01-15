"""
eitype - Type text on Wayland using the Emulated Input (EI) protocol.

This module provides Python bindings for the eitype Rust library, allowing you
to programmatically type text and press keys on Wayland compositors.

Example:
    >>> from eitype import EiType
    >>> typer = EiType.connect_portal()
    >>> typer.type_text("Hello, world!")
    >>> typer.press_key("Return")

For long-running applications (like voiceType), use token-based connections:
    >>> typer, token = EiType.connect_portal_with_token(saved_token)
    >>> if token:
    ...     save_token_to_config(token)  # Persist for next run
"""

from __future__ import annotations
from typing import Optional, Tuple

# Import from the Rust extension module
from eitype.eitype import EiType as _RustEiType, EiTypeConfig


def connect_portal(config: Optional[EiTypeConfig] = None) -> _RustEiType:
    """Connect via the XDG RemoteDesktop portal.

    This will prompt for user authorization on first use. The authorization
    may be cached by the compositor for future connections.

    Args:
        config: Optional keyboard configuration. If None, uses defaults.

    Returns:
        An EiType instance ready for typing.

    Raises:
        RuntimeError: If connection fails.
    """
    return _RustEiType.py_connect_portal(config)


def connect_portal_with_token(
    restore_token: Optional[str] = None,
    config: Optional[EiTypeConfig] = None,
) -> Tuple[_RustEiType, Optional[str]]:
    """Connect via the XDG RemoteDesktop portal with token support.

    If a valid restore_token is provided, the portal will skip the
    authorization dialog. Returns a new token that can be saved for
    future connections.

    Args:
        restore_token: Optional token from a previous connection.
        config: Optional keyboard configuration.

    Returns:
        A tuple of (EiType instance, new restore token or None).

    Raises:
        RuntimeError: If connection fails.

    Example:
        >>> # First run (no saved token)
        >>> typer, token = connect_portal_with_token()
        >>> save_to_config(token)  # Save for next time

        >>> # Subsequent runs
        >>> saved = load_from_config()
        >>> typer, _ = connect_portal_with_token(saved)
    """
    return _RustEiType.py_connect_portal_with_token(restore_token, config)


def connect_socket(path: str, config: Optional[EiTypeConfig] = None) -> _RustEiType:
    """Connect via a Unix socket.

    This is for direct connections to an EIS server, typically used for
    testing or advanced use cases.

    Args:
        path: Path to the Unix socket.
        config: Optional keyboard configuration.

    Returns:
        An EiType instance ready for typing.

    Raises:
        RuntimeError: If connection fails.
    """
    return _RustEiType.py_connect_socket(path, config)


# Create a wrapper class that has nice static methods
class EiType:
    """Main interface for typing text via the EI protocol.

    This class provides methods to connect to an EI server (via portal or socket)
    and emulate keyboard input.

    Example:
        >>> typer = EiType.connect_portal()
        >>> typer.type_text("Hello!")
        >>> typer.press_key("Return")
    """

    @staticmethod
    def connect_portal(config: Optional[EiTypeConfig] = None) -> "_RustEiType":
        """Connect via the XDG RemoteDesktop portal.

        Args:
            config: Optional keyboard configuration.

        Returns:
            An EiType instance ready for typing.
        """
        return connect_portal(config)

    @staticmethod
    def connect_portal_with_token(
        restore_token: Optional[str] = None,
        config: Optional[EiTypeConfig] = None,
    ) -> Tuple["_RustEiType", Optional[str]]:
        """Connect via the portal with token support.

        Args:
            restore_token: Optional token from a previous connection.
            config: Optional keyboard configuration.

        Returns:
            A tuple of (EiType instance, new restore token or None).
        """
        return connect_portal_with_token(restore_token, config)

    @staticmethod
    def connect_socket(path: str, config: Optional[EiTypeConfig] = None) -> "_RustEiType":
        """Connect via a Unix socket.

        Args:
            path: Path to the Unix socket.
            config: Optional keyboard configuration.

        Returns:
            An EiType instance ready for typing.
        """
        return connect_socket(path, config)


__all__ = [
    "EiType",
    "EiTypeConfig",
    "connect_portal",
    "connect_portal_with_token",
    "connect_socket",
]
