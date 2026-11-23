"""Textual cockpit for termux-sync.

Importing this package needs the optional ``tui`` extra. The command layer
turns the resulting ``ImportError`` into an install hint.
"""


def run(cli_prefix: list[str]) -> None:
    """Start the full-screen app. Imports Textual only when actually used."""
    from .app import TermuxSyncApp

    TermuxSyncApp(cli_prefix=cli_prefix).run()
