"""Full-screen cockpit for termux-sync."""

from textual.app import App, ComposeResult
from textual.css.query import NoMatches
from textual.widgets import DataTable, Footer, Header, Static

from .dashboard import Snapshot, format_date, human_size, load_snapshot, status_text


class TermuxSyncApp(App):
    """Owns the screen stack and the engine child process."""

    TITLE = "termux-sync"
    CSS_PATH = "app.tcss"
    BINDINGS = [
        ("b", "backup", "Backup"),
        ("r", "restore", "Restore"),
        ("l", "reload", "Reload"),
        ("d", "delete", "Delete"),
        ("s", "setup", "Setup"),
        ("q", "quit", "Quit"),
        ("question_mark", "help", "Help"),
    ]

    def __init__(self, cli_prefix: list[str]):
        super().__init__()
        self.cli_prefix = list(cli_prefix)
        self.snapshot: Snapshot = load_snapshot()

    def compose(self) -> ComposeResult:
        yield Header()
        yield Static("", id="status")
        yield Static("", id="banner")
        yield DataTable(id="backups")
        yield Static("", id="empty")
        yield Footer()

    def on_mount(self) -> None:
        self.refresh_dashboard()

    def on_resize(self) -> None:
        try:
            self.refresh_dashboard()
        except NoMatches:
            pass

    def refresh_dashboard(self) -> None:
        self.query_one("#status", Static).update(status_text(self.snapshot))

        banner = self.query_one("#banner", Static)
        banner.update(self.snapshot.warning or "")
        banner.display = bool(self.snapshot.warning)

        table = self.query_one("#backups", DataTable)
        empty = self.query_one("#empty", Static)
        table.clear(columns=True)

        if not self.snapshot.backups:
            table.display = False
            empty.display = True
            empty.update("No backups yet — press b to create one.")
            return

        table.display = True
        empty.display = False
        wide = self.size.width >= 80
        columns = ["Name", "Date", "Size", "Pkgs"] + (["Label"] if wide else [])
        table.add_columns(*columns)
        for backup in self.snapshot.backups:
            row = [
                backup["name"],
                format_date(backup["date"], wide),
                human_size(backup["size"]),
                str(backup["packages"]),
            ]
            if wide:
                row.append(backup["label"] or "—")
            table.add_row(*row)

    def action_reload(self) -> None:
        self.snapshot = load_snapshot()
        self.refresh_dashboard()

    def action_backup(self) -> None:
        self.notify("Backup cockpit is not wired yet.")

    def action_restore(self) -> None:
        self.notify("Restore cockpit is not wired yet.")

    def action_delete(self) -> None:
        self.notify("Delete is not wired yet.")

    def action_setup(self) -> None:
        self.notify("Run 'termux-sync setup' in a terminal.")

    def action_help(self) -> None:
        self.notify("b backup · r restore · l reload · d delete · s setup · q quit")
