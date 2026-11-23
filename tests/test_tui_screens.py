import contextlib
import json
from pathlib import Path

import pytest
from textual.widgets import DataTable

from termux_sync.tui.app import TermuxSyncApp
from termux_sync.tui.dashboard import load_snapshot
from termux_sync.tui.modals import ConfirmCancelModal
from termux_sync.tui.operation import OperationScreen
from termux_sync.tui.restore_plan import RestorePlanScreen
from termux_sync.tui.widgets import StageStepper


def write_backup(root: Path, name: str, packages=2, size=1024):
    d = root / name
    d.mkdir(parents=True)
    (d / "manifest.json").write_text(json.dumps({
        "name": name, "date": "2026-11-22T10:00:00", "label": "",
        "storage": "local", "compression": "gz", "version": "1.3.9",
        "files": {"home": {"archive": "home.tar.gz", "size": size, "checksum": "x", "source": "~"}},
        "packages": ["git", "python"][:packages],
    }))


def write_config(tmp_path: Path) -> Path:
    config_dir = tmp_path / ".config" / "termux-sync"
    config_dir.mkdir(parents=True)
    (config_dir / "config.json").write_text(
        json.dumps({"storage": "local", "local_path": str(tmp_path / "backups")})
    )
    return config_dir


def make_app(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    write_config(tmp_path)
    return TermuxSyncApp(cli_prefix=["python"])


def test_snapshot_lists_backups_newest_first(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    write_backup(tmp_path / "backups", "termux_backup_20260101_000000")
    write_backup(tmp_path / "backups", "termux_backup_20260202_000000")
    snap = load_snapshot(write_config(tmp_path))
    assert [b["name"] for b in snap.backups] == [
        "termux_backup_20260202_000000",
        "termux_backup_20260101_000000",
    ]


def test_snapshot_warns_on_corrupt_manifest(tmp_path):
    write_backup(tmp_path / "backups", "good")
    bad = tmp_path / "backups" / "bad"
    bad.mkdir()
    (bad / "manifest.json").write_text("{oops")
    config_dir = tmp_path / ".config" / "termux-sync"
    config_dir.mkdir(parents=True)
    (config_dir / "config.json").write_text(json.dumps({"storage": "local", "local_path": str(tmp_path / "backups")}))
    snap = load_snapshot(config_dir)
    assert snap.warning
    assert [b["name"] for b in snap.backups] == ["good"]


async def test_dashboard_renders_backup_rows(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    write_backup(tmp_path / "backups", "termux_backup_20260101_000000")
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test() as pilot:
        await pilot.pause()
        table = app.query_one("#backups", DataTable)
        assert table.row_count == 1


async def test_dashboard_empty_state(tmp_path, monkeypatch):
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test() as pilot:
        await pilot.pause()
        assert "No backups yet" in str(app.screen.query_one("#empty").content)


class FakeRunner:
    """Stands in for a real child process so screen tests never spawn one."""

    def __init__(self):
        self.cancelled = 0
        self.confirmed = []
        self.pid = 4242

    def start(self, command, name=""):
        return None

    def confirm(self, text="RESTORE"):
        self.confirmed.append(text)

    def cancel(self):
        self.cancelled += 1

    def wait(self, timeout=None):
        return 0

    def close(self):
        return None


@pytest.fixture
def fake_runner(monkeypatch):
    runner = FakeRunner()
    monkeypatch.setattr("termux_sync.tui.operation.OperationRunner", lambda *a, **k: runner)
    monkeypatch.setattr("termux_sync.tui.restore_plan.OperationRunner", lambda *a, **k: runner)
    return runner


async def test_operation_screen_shows_stages_and_log(tmp_path, monkeypatch, fake_runner):
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test() as pilot:
        screen = OperationScreen(cli_prefix=["python"], command="backup")
        app.push_screen(screen)
        await pilot.pause()
        screen.handle_event({"event": "start", "command": "backup", "name": "b1", "storage": "local"})
        screen.handle_event({"event": "stage", "id": "packages", "label": "Packages", "state": "complete", "index": 1, "total": 7})
        screen.handle_event({"event": "log", "level": "INFO", "message": "Collected 1947 packages"})
        await pilot.pause()
        assert "Packages" in str(screen.query_one(StageStepper).renderable)
        assert "1947" in str(screen.query_one("#log").lines[-1])


async def test_operation_screen_narrow_layout_keeps_current_stage(tmp_path, monkeypatch, fake_runner):
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test(size=(48, 12)) as pilot:
        screen = OperationScreen(cli_prefix=["python"], command="backup")
        app.push_screen(screen)
        screen.handle_event({"event": "stage", "id": "archive:home", "label": "Home", "state": "start", "index": 3, "total": 7})
        await pilot.pause()
        text = str(screen.query_one(StageStepper).renderable)
        assert "Home" in text and "3/7" in text


async def test_operation_screen_survives_tiny_terminal(tmp_path, monkeypatch, fake_runner):
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test(size=(40, 10)) as pilot:
        screen = OperationScreen(cli_prefix=["python"], command="backup")
        app.push_screen(screen)
        screen.handle_event({"event": "stage", "id": "packages", "label": "Packages", "state": "start", "index": 1, "total": 7})
        await pilot.pause()
        assert screen.state.status == "running"


async def test_operation_screen_shows_failure_card(tmp_path, monkeypatch, fake_runner):
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test() as pilot:
        screen = OperationScreen(cli_prefix=["python"], command="backup")
        app.push_screen(screen)
        screen.handle_event({"event": "error", "message": "Upload failed", "hint": "Check token", "exit_code": 1})
        await pilot.pause()
        assert app.screen.query_one("#failure").display is True
        assert "Check token" in str(app.screen.query_one("#failure").renderable)


async def test_child_death_without_events_shows_failure(tmp_path, monkeypatch, fake_runner):
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test() as pilot:
        screen = OperationScreen(cli_prefix=["python"], command="backup")
        app.push_screen(screen)
        screen.handle_exit(3)
        await pilot.pause()
        assert screen.state.status == "failed"
        assert screen.state.failure["exit_code"] == 3


async def test_restore_plan_requires_exact_token(tmp_path, monkeypatch, fake_runner):
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test() as pilot:
        screen = RestorePlanScreen(cli_prefix=["python"], backup_name="b1")
        app.push_screen(screen)
        screen.handle_event({"event": "plan", "name": "b1", "size": 1024, "created": "2026-11-22", "storage": "local", "packages": 2, "targets": ["home"]})
        screen.handle_event({"event": "package_diff", "chunk": 1, "chunks": 1, "install": ["git"], "present": ["python"], "missing": []})
        await pilot.pause()
        assert screen.can_confirm() is False
        screen.submit_confirmation("restore")
        assert screen.can_confirm() is False
        screen.submit_confirmation("RESTORE")
        assert screen.can_confirm() is True
        assert fake_runner.confirmed == ["RESTORE"]


async def test_restore_plan_decline_cancels(tmp_path, monkeypatch, fake_runner):
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test() as pilot:
        screen = RestorePlanScreen(cli_prefix=["python"], backup_name="b1")
        app.push_screen(screen)
        screen.action_decline()
        await pilot.pause()
        assert screen.declined is True


async def test_cancel_modal_confirms_with_enter(tmp_path, monkeypatch, fake_runner):
    app = make_app(tmp_path, monkeypatch)
    async with app.run_test() as pilot:
        screen = OperationScreen(cli_prefix=["python"], command="backup")
        app.push_screen(screen)
        screen.action_cancel()
        await pilot.pause()
        assert isinstance(app.screen, ConfirmCancelModal)
        await pilot.press("enter")
        await pilot.pause()
        assert screen.state.status == "cancelling"
        assert fake_runner.cancelled == 1


async def test_reduced_motion_disables_animation(monkeypatch):
    monkeypatch.setenv("TERMUX_SYNC_TUI_REDUCED_MOTION", "1")
    from termux_sync.tui.modals import reduced_motion
    assert reduced_motion() is True


async def test_dashboard_delete_suspends_for_cli(tmp_path, monkeypatch):
    app = make_app(tmp_path, monkeypatch)
    calls = []
    monkeypatch.setattr("termux_sync.tui.dashboard.run_cli", lambda prefix, argv: calls.append(argv))
    monkeypatch.setattr(type(app), "suspend", lambda self: contextlib.nullcontext())
    async with app.run_test() as pilot:
        await pilot.press("d")
        await pilot.pause()
    assert calls and calls[0][0] == "delete"
