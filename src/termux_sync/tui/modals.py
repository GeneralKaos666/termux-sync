"""Modal dialogs for the cockpit."""

from textual.app import ComposeResult
from textual.containers import Grid
from textual.screen import ModalScreen
from textual.widgets import Static


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
