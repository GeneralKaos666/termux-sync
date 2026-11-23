import json
from pathlib import Path

from textual.widgets import DataTable

from termux_sync.tui.app import TermuxSyncApp
from termux_sync.tui.dashboard import load_snapshot


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
