"""Restore preview: what the backup contains and what will change."""

from textual.app import ComposeResult
from textual.css.query import NoMatches
from textual.screen import Screen
from textual.widgets import DataTable, Footer, Header, Input, Static

from .dashboard import human_size
from .runner import OperationRunner
from .widgets import RenderableStatic


class RestorePlanScreen(Screen):
    """Shows the plan, asks for the exact RESTORE token, then hands over to extraction."""

    BINDINGS = [("escape", "decline", "Decline")]

    def __init__(self, cli_prefix: list[str], backup_name: str):
        super().__init__()
        self.cli_prefix = list(cli_prefix)
        self.backup_name = backup_name
        self.plan: dict | None = None
        self.diff: dict[str, list[str]] = {"install": [], "present": [], "missing": []}
        self.declined = False
        self.runner: OperationRunner | None = None
        self._confirmed = False
        self._status = "reading backup…"

    def compose(self) -> ComposeResult:
        yield Header()
        yield Static("", id="title")
        yield RenderableStatic("", id="summary")
        yield DataTable(id="diff")
        yield Input(placeholder="Type RESTORE to continue", id="confirm")
        yield Static("", id="hint")
        yield Footer()

    def on_mount(self) -> None:
        self.runner = OperationRunner(
            self.cli_prefix, on_event=self.handle_event, on_exit=self.handle_exit
        )
        self.runner.start("restore", self.backup_name)
        self._refresh()

    def on_unmount(self) -> None:
        if self.runner is not None:
            self.runner.close()

    def can_confirm(self) -> bool:
        return self._confirmed

    def submit_confirmation(self, value: str) -> bool:
        self._confirmed = value == "RESTORE"
        if self._confirmed and self.runner is not None:
            self.runner.confirm("RESTORE")
            self._status = "restoring…"
        self._refresh()
        return self._confirmed

    def action_decline(self) -> None:
        self.declined = True
        if self.runner is not None:
            self.runner.confirm("")
        self._status = "declined"
        self._refresh()

    def handle_event(self, event: dict) -> None:
        kind = event.get("event")
        if kind == "plan":
            self.plan = event
            self._status = "ready — type RESTORE to continue"
        elif kind == "package_diff":
            for key in ("install", "present", "missing"):
                merged = set(self.diff[key]) | set(event.get(key, []))
                self.diff[key] = sorted(merged)
        elif kind == "result":
            self._status = "restore finished"
        elif kind == "error":
            self._status = str(event.get("message", "restore failed"))
        elif kind == "cancelled":
            self._status = "restore cancelled"
        self._refresh()

    def handle_exit(self, code: int) -> None:
        if code != 0 and not self.declined:
            self._status = f"engine exited with code {code}"
        self._refresh()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        self.submit_confirmation(event.value)

    def _refresh(self) -> None:
        try:
            self._refresh_impl()
        except NoMatches:
            pass

    def _refresh_impl(self) -> None:
        name = (self.plan or {}).get("name") or self.backup_name
        self.query_one("#title", Static).update(f"restore · {name} · {self._status}")

        summary = self.query_one("#summary", RenderableStatic)
        if self.plan:
            size = human_size(int(self.plan.get("size", 0) or 0))
            targets = ", ".join(self.plan.get("targets", [])) or "—"
            summary.update(
                f"{self.plan.get('created', '—')} · {size} · "
                f"{self.plan.get('packages', 0)} packages · targets {targets}"
            )
        else:
            summary.update("")

        table = self.query_one("#diff", DataTable)
        table.clear(columns=True)
        wide = self.size.width >= 80
        columns = ["Action", "Count"] + (["Packages"] if wide else [])
        table.add_columns(*columns)
        for action in ("install", "present", "missing"):
            packages = self.diff[action]
            row = [action, str(len(packages))]
            if wide:
                row.append(", ".join(packages[:6]) + ("…" if len(packages) > 6 else "") or "—")
            table.add_row(*row)

        hint = self.query_one("#hint", Static)
        if self.declined:
            hint.update("Restore declined; the engine is cleaning up.")
        elif self._confirmed:
            hint.update("Confirmed. Extracting now.")
        else:
            hint.update("This overwrites existing files. Type RESTORE, then Enter.")
