"""Full-screen cockpit for termux-sync, laid out like the Rust reference."""

from rich.style import Style
from rich.text import Text
from textual import work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.css.query import NoMatches
from textual.widgets import DataTable, Label, ListView, ListItem, Static

from .dashboard import (
    ACCENT,
    MAGENTA,
    MUTED,
    SUCCESS,
    WARN,
    Snapshot,
    backup_age,
    gradient_text,
    human_size,
    load_snapshot,
    render_log_line,
    short_name,
    size_bar,
    status_text,
)
from .modals import HelpScreen, reduced_motion

SPLIT_WIDTH = 80
CARDS_MIN_HEIGHT = 20
MIN_WIDTH = 48
MIN_HEIGHT = 12

ACTIONS = [
    ("↑", "Backup now", "backup"),
    ("↺", "Restore selected", "restore"),
    ("✕", "Delete selected", "delete"),
    ("☰", "List backups", "list"),
    ("▤", "Show logs", "logs"),
    ("◔", "Disk check (~)", "disk"),
    ("⚙", "Setup", "setup"),
    ("◷", "Schedule", "schedule"),
    ("⌫", "Clear cache", "clear-cache"),
    ("⟳", "Refresh", "refresh"),
    ("?", "Help", "help"),
    ("↩", "Quit", "quit"),
]

PANES = ("actions", "backups", "output")


class FocusableStatic(Static):
    """Static that can take keyboard focus, so Tab can reach the output pane."""

    can_focus = True


class TermuxSyncApp(App):
    """Owns the screen stack and the engine child process."""

    TITLE = "termux-sync"
    CSS_PATH = "app.tcss"
    BINDINGS = [
        ("b", "backup", "Backup"),
        ("r", "restore", "Restore"),
        ("l", "reload", "Refresh"),
        ("d", "delete", "Delete"),
        ("s", "setup", "Setup"),
        Binding("tab", "focus_next_pane", "Focus", priority=True),
        ("q", "quit", "Quit"),
        ("question_mark", "help", "Help"),
    ]

    def __init__(self, cli_prefix: list[str]):
        super().__init__()
        self.cli_prefix = list(cli_prefix)
        self.snapshot: Snapshot = load_snapshot()
        self.focus_pane = "actions"
        self.output_text: str | None = None
        self.output_title = ""
        self.busy: str | None = None

    def compose(self) -> ComposeResult:
        yield Static("", id="too-small")

        yield Horizontal(
            Static(id="hero-left"),
            Static(id="hero-right"),
            id="hero",
        )

        yield Horizontal(
            Static(id="stat-backups"),
            Static(id="stat-stored"),
            Static(id="stat-packages"),
            Static(id="stat-auto"),
            id="stats",
        )

        yield Horizontal(
            Vertical(
                Static("▸ Actions", id="actions-title", classes="pane-title"),
                ListView(id="actions"),
                id="actions-pane",
            ),
            Vertical(
                Vertical(
                    Static("▸ Backups", id="backups-title", classes="pane-title"),
                    DataTable(id="backups"),
                    Static("", id="empty"),
                    id="backups-pane",
                ),
                Vertical(
                    Static("▸ Output", id="output-title", classes="pane-title"),
                    FocusableStatic("", id="output"),
                    id="output-pane",
                ),
                id="right",
            ),
            id="body",
        )

        yield Static("", id="footer-status")
        yield Static("", id="footer-hints")

    def on_mount(self) -> None:
        if reduced_motion():
            self.add_class("reduced-motion")

        table = self.query_one("#backups", DataTable)
        table.cursor_type = "row"
        table.zebra_stripes = True

        actions = self.query_one("#actions", ListView)
        for glyph, label, _action in ACTIONS:
            actions.append(ListItem(Label(f"{glyph} {label}")))

        self.query_one("#stat-backups", Static).border_title = "BACKUPS"
        self.query_one("#stat-stored", Static).border_title = "STORED"
        self.query_one("#stat-packages", Static).border_title = "PACKAGES"
        self.query_one("#stat-auto", Static).border_title = "AUTO"

        self.refresh_view()
        self._focus_pane("actions")

    def on_resize(self) -> None:
        try:
            self.refresh_view()
        except NoMatches:
            pass

    # -- layout ---------------------------------------------------------

    @property
    def split(self) -> bool:
        return self.size.width >= SPLIT_WIDTH

    def _focus_pane(self, pane: str) -> None:
        self.focus_pane = pane
        widget = {
            "actions": "#actions",
            "backups": "#backups",
            "output": "#output",
        }[pane]
        try:
            self.query_one(widget).focus()
        except NoMatches:
            pass
        self._layout_panes()

    def _layout_panes(self) -> None:
        try:
            actions = self.query_one("#actions-pane", Vertical)
            backups = self.query_one("#backups-pane", Vertical)
            output = self.query_one("#output-pane", Vertical)
        except NoMatches:
            return

        if self.split:
            actions.display = backups.display = output.display = True
        else:
            actions.display = self.focus_pane == "actions"
            backups.display = self.focus_pane == "backups"
            output.display = self.focus_pane == "output"

        for pane, widget in (
            ("actions", actions),
            ("backups", backups),
            ("output", output),
        ):
            widget.set_class(pane == self.focus_pane, "focused")

    def action_focus_next_pane(self) -> None:
        index = PANES.index(self.focus_pane)
        self._focus_pane(PANES[(index + 1) % len(PANES)])

    # -- rendering ------------------------------------------------------

    def refresh_view(self) -> None:
        too_small = self.size.width < MIN_WIDTH or self.size.height < MIN_HEIGHT
        self.query_one("#too-small", Static).display = too_small
        for selector in ("#hero", "#stats", "#body", "#footer-status", "#footer-hints"):
            self.query_one(selector).display = not too_small
        if too_small:
            self.query_one("#too-small", Static).update(
                f"terminal too small\nneed {MIN_WIDTH}×{MIN_HEIGHT} — "
                f"current {self.size.width}×{self.size.height}"
            )
            return

        self._render_hero()
        self._render_stats()
        self._render_backups()
        self._render_output()
        self._render_footer()
        self._layout_panes()

    def _render_hero(self) -> None:
        left = Text()
        left.append_text(gradient_text("TERMUX SYNC"))
        left.append("   backup dashboard", style=Style(color=MUTED))
        if self.size.width >= 100:
            left.append(
                f"   {self.snapshot.compression.upper()} · max {self.snapshot.max_backups}",
                style=Style(color=MUTED),
            )
        self.query_one("#hero-left", Static).update(left)

        count = len(self.snapshot.backups)
        label = "1 BACKUP" if count == 1 else f"{count} BACKUPS"
        count_color = MUTED if count == 0 else SUCCESS
        right = Text()
        right.append(f" {self.snapshot.storage.upper()} ", style=Style(color="black", bgcolor=ACCENT, bold=True))
        right.append(" ")
        right.append(f" {label} ", style=Style(color="black", bgcolor=count_color, bold=True))
        if self.snapshot.warning:
            right.append(" ")
            right.append("[!]", style=Style(color=WARN, bold=True))
        self.query_one("#hero-right", Static).update(right)

    def _render_stats(self) -> None:
        show = self.size.width >= SPLIT_WIDTH and self.size.height >= CARDS_MIN_HEIGHT
        stats = self.query_one("#stats", Horizontal)
        stats.display = show
        if not show:
            return

        total = sum(backup["size"] for backup in self.snapshot.backups)
        latest_packages = self.snapshot.backups[0]["packages"] if self.snapshot.backups else 0
        cards = (
            ("#stat-backups", str(len(self.snapshot.backups)), "saved", SUCCESS),
            ("#stat-stored", human_size(total), "", ACCENT),
            ("#stat-packages", str(latest_packages), "latest", MAGENTA),
            ("#stat-auto", self.snapshot.schedule, "", WARN),
        )
        for selector, value, unit, color in cards:
            text = Text()
            text.append(value, style=Style(color=color, bold=True))
            if unit:
                text.append(f" {unit}", style=Style(color=MUTED))
            self.query_one(selector, Static).update(text)

    def _render_backups(self) -> None:
        table = self.query_one("#backups", DataTable)
        empty = self.query_one("#empty", Static)
        if not table.columns:
            # on_resize can arrive before on_mount on a real terminal.
            table.add_columns("NAME", "AGE", "SIZE", "USED", "PKGS")
        table.clear()
        table.display = bool(self.snapshot.backups)
        empty.display = not self.snapshot.backups

        if not self.snapshot.backups:
            empty.update(
                f"No local backups\n{self.snapshot.warning or 'Press l to refresh.'}"
            )
            self.query_one("#backups-title", Static).update("▸ Backups")
            return

        maximum = max((backup["size"] for backup in self.snapshot.backups), default=1) or 1
        for backup in self.snapshot.backups:
            label = f"  {backup['label']}" if backup["label"] else ""
            used = Text(size_bar(backup["size"], maximum), style=Style(color=SUCCESS))
            table.add_row(
                f"{short_name(backup['name'])}{label}",
                backup_age(backup),
                human_size(backup["size"]),
                used,
                str(backup["packages"]),
                key=backup["name"],
            )
        self._update_backups_title()

    def _update_backups_title(self) -> None:
        backup = self.selected_backup()
        detail = ""
        if backup:
            parts = []
            if backup["label"]:
                parts.append(backup["label"])
            parts.append(f"v{backup['version']} {backup['compression']}")
            detail = " · " + " · ".join(parts)
        self.query_one("#backups-title", Static).update(f"▸ Backups{detail}")

    def _render_output(self) -> None:
        output = self.query_one("#output", Static)
        title = self.query_one("#output-title", Static)

        if self.output_text is not None:
            title.update(f"▸ Output · {self.output_title}" if self.output_title else "▸ Output")
            body = self.output_text.strip() or "(no output)"
            output.update(Text(body, style=Style(color=MUTED if not body else None)))
            return

        title.update("▸ Output · sync.log")
        if not self.snapshot.log:
            output.update(Text("No log entries yet.", style=Style(color=MUTED)))
            return
        text = Text()
        for index, line in enumerate(self.snapshot.log):
            if index:
                text.append("\n")
            text.append_text(render_log_line(line))
        output.update(text)

    def _render_footer(self) -> None:
        if self.busy:
            marker, marker_color = "◐", ACCENT
            label, label_color = f"running termux-sync {self.busy}…", MUTED
        else:
            marker = "●"
            marker_color = WARN if self.snapshot.warning else SUCCESS
            label = status_text(self.snapshot)
            label_color = WARN if self.snapshot.warning else MUTED
        status = Text()
        status.append(f" {marker} ", style=Style(color=marker_color))
        status.append(label, style=Style(color=label_color))
        self.query_one("#footer-status", Static).update(status)

        wide = self.size.width >= SPLIT_WIDTH
        hints = {
            "actions": [("↑↓", "move"), ("Enter", "run"), ("Tab", "focus"), ("l", "refresh"), ("?", "help"), ("q", "quit")],
            "backups": [("↑↓", "move"), ("Enter", "restore"), ("d", "delete"), ("Tab", "focus"), ("l", "refresh"), ("q", "quit")],
            "output": [("↑↓", "scroll"), ("Tab", "focus"), ("?", "help"), ("q", "quit")],
        }[self.focus_pane]

        line = Text()
        line.append(f" {self.focus_pane.upper()} ", style=Style(color="black", bgcolor=ACCENT, bold=True))
        for key, action in hints:
            line.append("  ")
            if wide:
                line.append(f" {key} ", style=Style(color="white", bgcolor="#555555", bold=True))
            else:
                line.append(key, style=Style(color=ACCENT, bold=True))
            if action and wide:
                line.append(f" {action}", style=Style(color=MUTED))
        self.query_one("#footer-hints", Static).update(line)

    # -- selection ------------------------------------------------------

    def selected_backup(self) -> dict | None:
        if not self.snapshot.backups:
            return None
        table = self.query_one("#backups", DataTable)
        try:
            row = table.cursor_row
        except (AttributeError, NoMatches):
            return None
        if row is None or row < 0 or row >= len(self.snapshot.backups):
            return None
        return self.snapshot.backups[row]

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        self._update_backups_title()

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        self.action_restore()

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        index = event.list_view.index
        if index is not None and 0 <= index < len(ACTIONS):
            self._run_action(ACTIONS[index][2])

    def _run_action(self, action: str) -> None:
        handlers = {
            "backup": self.action_backup,
            "restore": self.action_restore,
            "delete": self.action_delete,
            "list": lambda: self._capture(["list"]),
            "logs": self.action_show_logs,
            "disk": lambda: self._capture(["check", "~"]),
            "setup": self.action_setup,
            "schedule": lambda: self._hand_off(["schedule"]),
            "clear-cache": lambda: self._hand_off(["clear-cache"]),
            "refresh": self.action_reload,
            "help": self.action_help,
            "quit": self.action_quit,
        }
        handlers[action]()

    def _hand_off(self, argv: list[str]) -> None:
        from .dashboard import run_cli

        with self.suspend():
            run_cli(self.cli_prefix, argv)
        self.action_reload()

    @work(thread=True, exclusive=True)
    def _capture(self, argv: list[str]) -> None:
        """Read-only commands render inside the output pane, like the Rust cockpit."""
        from .dashboard import run_captured

        title = " ".join(argv)
        self.call_from_thread(self._set_busy, title)
        text = run_captured(self.cli_prefix, argv)
        self.call_from_thread(self._show_output, text, title)

    def _set_busy(self, title: str) -> None:
        self.busy = title
        self._render_footer()

    def _show_output(self, text: str, title: str) -> None:
        self.busy = None
        self.output_text = text
        self.output_title = title
        self._focus_pane("output")
        self._render_output()
        self._render_footer()

    def action_show_logs(self) -> None:
        self.output_text = None
        self.output_title = ""
        self.snapshot = load_snapshot()
        self._focus_pane("output")
        self._render_output()
        self._render_footer()

    # -- actions --------------------------------------------------------

    def action_reload(self) -> None:
        self.snapshot = load_snapshot()
        self.refresh_view()

    def action_backup(self) -> None:
        from .operation import OperationScreen

        self.push_screen(OperationScreen(self.cli_prefix, "backup"))

    def action_restore(self) -> None:
        from .restore_plan import RestorePlanScreen

        backup = self.selected_backup() or (self.snapshot.backups[0] if self.snapshot.backups else None)
        if backup is None:
            self.notify("No backups to restore.")
            return
        self.push_screen(RestorePlanScreen(self.cli_prefix, backup["name"]))

    def action_delete(self) -> None:
        backup = self.selected_backup()
        if backup is None:
            self.notify("No backups to delete.")
            return
        self._hand_off(["delete", "--name", backup["name"]])

    def action_setup(self) -> None:
        self._hand_off(["setup"])

    def action_help(self) -> None:
        self.push_screen(HelpScreen())
