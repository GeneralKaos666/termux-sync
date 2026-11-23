"""Live view of a backup or restore child process."""

import time

from textual.app import ComposeResult
from textual.css.query import NoMatches
from textual.screen import Screen
from textual.widgets import Footer, Header, Static

from .dashboard import human_size
from .models import OperationState, apply_event, new_operation
from .modals import ConfirmCancelModal, FailureCard, ResultCard
from .runner import OperationRunner
from .widgets import GaugeRow, LogView, StageStepper

MARKS = {"complete": "✓", "failed": "✗", "running": "▶", "pending": "·"}


class OperationScreen(Screen):
    """Streams stages, progress, and logs from one engine run."""

    BINDINGS = [
        ("escape", "cancel", "Cancel"),
        ("r", "retry", "Retry"),
    ]

    def __init__(self, cli_prefix: list[str], command: str, backup_name: str = ""):
        super().__init__()
        self.cli_prefix = list(cli_prefix)
        self.command = command
        self.backup_name = backup_name
        self.state: OperationState = new_operation(command)
        self.runner: OperationRunner | None = None
        self._index = 0
        self._total = 0
        self._timer = None

    def compose(self) -> ComposeResult:
        yield Header()
        yield Static("", id="title")
        yield StageStepper(id="stepper")
        yield GaugeRow(id="gauges")
        yield LogView(id="log", max_lines=2000)
        yield FailureCard("", id="failure")
        yield ResultCard("", id="result")
        yield Static("", id="size-warning")
        yield Footer()

    def on_mount(self) -> None:
        self.query_one("#failure").display = False
        self.query_one("#result").display = False
        self.query_one("#size-warning").display = False
        self._start_runner()
        self._timer = self.set_interval(1, self._tick)
        self._refresh()

    def on_unmount(self) -> None:
        if self._timer is not None:
            self._timer.stop()
        if self.runner is not None:
            self.runner.close()

    def _start_runner(self) -> None:
        self.runner = OperationRunner(
            self.cli_prefix, on_event=self._thread_event, on_exit=self._thread_exit
        )
        self.runner.start(self.command, self.backup_name)

    def _dispatch(self, callback, *args) -> None:
        """Runner callbacks arrive on worker threads; Textual widgets need the app thread."""
        try:
            self.app.call_from_thread(callback, *args)
        except RuntimeError:
            callback(*args)

    def _thread_event(self, event: dict) -> None:
        self._dispatch(self.handle_event, event)

    def _thread_exit(self, code: int) -> None:
        self._dispatch(self.handle_exit, code)

    def handle_event(self, event: dict) -> None:
        if event.get("event") == "stage":
            if event.get("index"):
                self._index = int(event["index"])
            if event.get("total"):
                self._total = int(event["total"])
        apply_event(self.state, event)
        if event.get("event") == "log":
            try:
                self.query_one("#log", LogView).write(
                    f"[{event.get('level', 'INFO')}] {event.get('message', '')}"
                )
            except NoMatches:
                pass
        self._refresh()

    def handle_exit(self, code: int) -> None:
        if self.state.status in ("running", "cancelling"):
            if code == 130:
                self.state.status = "cancelled"
            elif code == 0:
                self.state.status = "succeeded"
            else:
                self.state.status = "failed"
                tail = ""
                if self.runner is not None:
                    tail = "\n".join(list(getattr(self.runner, "stderr_tail", []))[-5:]).strip()
                self.state.failure = {
                    "message": f"Engine exited with code {code}",
                    "hint": tail or "Check the log above.",
                    "exit_code": code,
                }
        self._refresh()

    def action_cancel(self) -> None:
        if self.state.status not in ("running", "cancelling"):
            return

        def _decided(confirmed: bool) -> None:
            if confirmed and self.runner is not None:
                self.runner.cancel()
                self.state.status = "cancelling"
                self._refresh()

        self.app.push_screen(ConfirmCancelModal(), _decided)

    def action_retry(self) -> None:
        self.app.switch_screen(
            OperationScreen(self.cli_prefix, self.command, self.backup_name)
        )

    def _tick(self) -> None:
        self._refresh()

    def _elapsed(self) -> int:
        return int(time.monotonic() - self.state.started_at)

    def _stepper_text(self) -> str:
        if self.size.width < 80:
            stage = self.state.current_stage()
            if stage is None:
                return "waiting…"
            return f"{stage.label} · {self._index}/{self._total}"
        lines = []
        for stage in self.state.stages:
            lines.append(f"{MARKS.get(stage.state, '·')} {stage.label}")
        return "\n".join(lines)

    def _refresh(self) -> None:
        try:
            self._refresh_impl()
        except NoMatches:
            pass

    def _refresh_impl(self) -> None:
        name = self.state.name or self.backup_name or "starting"
        self.query_one("#title", Static).update(
            f"{self.command} · {name} · {self.state.storage or '—'} · "
            f"{self._elapsed()}s · {self.state.status}"
        )
        self.query_one(StageStepper).update(self._stepper_text())

        self.query_one("#overall").update(progress=self.state.overall_percent())
        stage = self.state.current_stage()
        detail = self.query_one("#gauge-detail", Static)
        if stage is None:
            self.query_one("#stage").update(progress=0)
            detail.update("")
        elif stage.total:
            self.query_one("#stage").update(progress=(stage.current or 0) / stage.total * 100)
            detail.update(f"{stage.label}: {stage.current or 0} / {stage.total}")
        else:
            self.query_one("#stage").update(progress=0)
            detail.update(f"{stage.label}: measuring…")

        failure = self.query_one("#failure", FailureCard)
        if self.state.failure:
            text = self.state.failure.get("message", "Operation failed")
            hint = self.state.failure.get("hint", "")
            failure.update(f"{text}\n{hint}".strip())
            failure.display = True
        else:
            failure.display = False

        result = self.query_one("#result", ResultCard)
        if self.state.result:
            payload = self.state.result
            size = human_size(int(payload.get("size", 0) or 0))
            result.update(f"✓ {payload.get('name', name)} · {size}")
            result.display = True
        else:
            result.display = False

        warning = self.query_one("#size-warning", Static)
        too_small = self.size.width < 48 or self.size.height < 12
        warning.update("Terminal is very small; the operation keeps running.")
        warning.display = too_small
