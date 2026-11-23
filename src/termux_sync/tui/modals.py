"""Modal dialogs for the cockpit."""

import os

from textual.app import ComposeResult
from textual.containers import Grid
from textual.screen import ModalScreen
from textual.widgets import Static

from .widgets import RenderableStatic


class ConfirmCancelModal(ModalScreen[bool]):
    """Enter cancels the operation, Esc keeps it running."""

    BINDINGS = [
        ("escape", "decline", "Keep running"),
        ("enter", "confirm", "Cancel operation"),
    ]

    def compose(self) -> ComposeResult:
        yield Grid(
            Static("Cancel this operation?", id="question"),
            Static("Enter cancels · Esc keeps running", id="hint"),
            id="dialog",
        )

    def action_decline(self) -> None:
        self.dismiss(False)

    def action_confirm(self) -> None:
        self.dismiss(True)


HELP_TEXT = """b   Backup
r   Restore the newest backup
l   Reload the dashboard
d   Delete a backup (hands over to the CLI)
s   Run setup (hands over to the CLI)
?   This help
q   Quit"""


class HelpScreen(ModalScreen[None]):
    """Lists the cockpit bindings."""

    BINDINGS = [
        ("escape", "close_help", "Close"),
        ("question_mark", "close_help", "Close"),
    ]

    def compose(self) -> ComposeResult:
        yield Grid(Static(HELP_TEXT, id="help"), id="dialog")

    def action_close_help(self) -> None:
        self.dismiss(None)


class ResultCard(RenderableStatic):
    """Success summary for a finished operation."""


class FailureCard(RenderableStatic):
    """Failure summary with the engine's hint."""


def reduced_motion() -> bool:
    value = os.environ.get("TERMUX_SYNC_TUI_REDUCED_MOTION", "").strip().lower()
    return value in {"1", "true", "yes", "on"}
