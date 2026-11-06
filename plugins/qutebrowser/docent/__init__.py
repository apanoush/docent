"""Docent adapter for qutebrowser.

The adapter is loaded from qutebrowser's ``config.py``::

    import sys
    sys.path.insert(0, "/path/to/reading-assistant/qutebrowser")
    import docent

    docent.setup(config, keymaps={"explain": ",de"})

The package itself has no qutebrowser imports. ``setup`` lazily imports
``docent.plugin`` so the pure request, stream, and profile modules can be tested
outside a running browser.
"""

from __future__ import annotations

from typing import Any

__version__ = "0.1.0"
__all__ = ["profile_list", "refresh", "setup"]


def setup(config: Any = None, **options: Any) -> Any:
    """Load the qutebrowser integration and apply plugin settings."""
    from . import plugin

    return plugin.setup(config, **options)


def refresh() -> None:
    """Clear the cached profile list."""
    from . import plugin

    plugin.refresh()


def profile_list(force: bool = False) -> Any:
    """Return profile metadata, loading it from the CLI when needed."""
    from . import plugin

    return plugin.profiles(force)
