import io
import json
import urllib.request

import pytest

import termux_sync as ts


@pytest.fixture
def events(monkeypatch):
    buffer = io.StringIO()
    writer = ts.EventWriter(secrets=["secret-token", "secret-pass"])
    writer.stream = buffer
    monkeypatch.setattr(ts, "EVENTS", writer)
    return buffer


def parsed(buffer):
    buffer.seek(0)
    return [json.loads(line) for line in buffer.read().splitlines() if line]


def make_backup(local_cfg):
    ts.cmd_backup(local_cfg, label="events")
    return ts.LocalStorage(local_cfg).list_backups()[0]


def test_event_writer_redacts_secrets():
    buffer = io.StringIO()
    writer = ts.EventWriter(secrets=["secret-token"])
    writer.stream = buffer
    writer.emit("log", level="INFO", message="token=secret-token")
    event = parsed(buffer)[0]
    assert event["message"] == "token=***"


def test_backup_emits_stages_and_verified_result(events, local_cfg, termux_tree):
    make_backup(local_cfg)
    events_list = parsed(events)
    names = [event["event"] for event in events_list]

    assert names[0] == "start"
    assert "plan" not in names
    stages = [event["id"] for event in events_list if event["event"] == "stage"]
    assert stages[0] == "preflight"
    assert "packages" in stages
    assert "archive:home" in stages
    assert "upload" in stages
    assert "verify" in stages

    verify = [event for event in events_list if event["event"] == "verify"][0]
    assert verify["state"] == "verified"

    result = [event for event in events_list if event["event"] == "result"][0]
    assert result["ok"] is True
    assert result["verified"] is True
    assert result["name"].startswith("termux_backup_")
    assert "secret-token" not in events.getvalue()


def test_package_diff_chunks_at_500(events):
    packages = [f"pkg-{index}" for index in range(1201)]
    ts._emit_package_diff("to_install", packages)
    events_list = parsed(events)
    assert len(events_list) == 3
    assert [event["part"] for event in events_list] == [1, 2, 3]
    assert all(event["parts"] == 3 for event in events_list)
    assert len(events_list[0]["packages"]) == 500
    assert len(events_list[2]["packages"]) == 201


def test_package_diff_unavailable_without_dpkg(monkeypatch):
    def fail(*args, **kwargs):
        raise FileNotFoundError("dpkg")

    monkeypatch.setattr(ts.subprocess, "run", fail)
    available, to_install, already, device_only = ts._restore_package_diff(["git"])
    assert available is False
    assert to_install == []
    assert already == []
    assert device_only == []


def test_restore_plan_diff_confirm_and_result(events, local_cfg, termux_tree, monkeypatch):
    manifest = make_backup(local_cfg)
    events.seek(0)
    events.truncate(0)

    monkeypatch.setattr(ts, "install_packages", lambda packages, console: [])
    monkeypatch.setattr(ts.sys, "stdin", io.StringIO('{"confirm":"RESTORE"}\n'))

    ts.cmd_restore(local_cfg, backup_name=manifest["name"])
    events_list = parsed(events)
    names = [event["event"] for event in events_list]

    plan = [event for event in events_list if event["event"] == "plan"][0]
    assert plan["name"] == manifest["name"]
    assert plan["diff_available"] is True
    assert plan["packages"] == 2
    assert plan["checksums"] == "pending"

    categories = {
        event["category"]
        for event in events_list
        if event["event"] == "package_diff"
    }
    assert categories == {"to_install", "already_installed", "device_only"}

    verify = [event for event in events_list if event["event"] == "verify"][0]
    assert verify["state"] == "verified"
    assert "extract:home" in [event.get("id") for event in events_list if event["event"] == "stage"]

    result = [event for event in events_list if event["event"] == "result"][0]
    assert result["ok"] is True
    assert result["warnings"] is False
    assert result["restart_required"] is True
    assert names.index("plan") < names.index("verify")


def test_restore_decline_emits_cancelled(events, local_cfg, termux_tree, monkeypatch):
    manifest = make_backup(local_cfg)
    events.seek(0)
    events.truncate(0)
    monkeypatch.setattr(ts.sys, "stdin", io.StringIO('{"confirm":"CANCEL"}\n'))

    ts.cmd_restore(local_cfg, backup_name=manifest["name"])
    events_list = parsed(events)
    names = [event["event"] for event in events_list]
    assert "cancelled" in names
    assert "verify" not in names
    assert all(event.get("id") != "extract:home" for event in events_list)


def test_restore_checksum_failure_never_extracts(events, local_cfg, termux_tree, monkeypatch):
    manifest = make_backup(local_cfg)
    events.seek(0)
    events.truncate(0)
    stored = ts.Path(local_cfg["local_path"]) / manifest["name"] / manifest["files"]["home"]["archive"]
    stored.write_bytes(b"corrupted")

    monkeypatch.setattr(ts, "install_packages", lambda packages, console: [])
    monkeypatch.setattr(ts.sys, "stdin", io.StringIO('{"confirm":"RESTORE"}\n'))

    ts.cmd_restore(local_cfg, backup_name=manifest["name"])
    events_list = parsed(events)
    assert any(event["event"] == "error" for event in events_list)
    assert all(event.get("id") != "extract:home" for event in events_list)
    verify = [event for event in events_list if event["event"] == "verify"][0]
    assert verify["state"] == "failed"


def test_delete_yes_skips_confirmation(local_cfg, termux_tree, monkeypatch):
    manifest = make_backup(local_cfg)

    def fail(*args, **kwargs):
        raise AssertionError("confirmation should not be asked")

    monkeypatch.setattr(ts.Confirm, "ask", staticmethod(fail))
    ts.cmd_delete(local_cfg, backup_name=manifest["name"], assume_yes=True)
    stored = ts.Path(local_cfg["local_path"]) / manifest["name"]
    assert not stored.exists()


def test_clear_cache_yes_removes_without_prompts(tmp_path, monkeypatch):
    cache = tmp_path / "cache"
    cache.mkdir()
    (cache / "junk").write_text("x")
    monkeypatch.setattr(ts, "CACHE_TARGETS", [("test cache", cache)])

    def fail(*args, **kwargs):
        raise AssertionError("confirmation should not be asked")

    monkeypatch.setattr(ts.Confirm, "ask", staticmethod(fail))
    ts.cmd_clear_cache(assume_yes=True)
    assert cache.exists()
    assert list(cache.iterdir()) == []


def test_list_emits_backup_events(events, local_cfg, termux_tree):
    make_backup(local_cfg)
    events.seek(0)
    events.truncate(0)
    ts.cmd_list(local_cfg)
    items = [e for e in parsed(events) if e["event"] == "backup"]
    assert len(items) == 1
    assert items[0]["name"].startswith("termux_backup_")
    assert items[0]["packages"] == 2
    assert items[0]["storage"] == "local"
    result = [e for e in parsed(events) if e["event"] == "result"][0]
    assert result["count"] == 1


def test_local_manifest_fetch_does_not_copy_archives(local_cfg, termux_tree, monkeypatch):
    manifest = make_backup(local_cfg)
    storage = ts.LocalStorage(local_cfg)
    monkeypatch.setattr(
        storage,
        "download",
        lambda *a, **k: (_ for _ in ()).throw(AssertionError("download called")),
    )
    assert storage.fetch_manifest(manifest["name"])["name"] == manifest["name"]


def test_gdrive_manifest_fetch_uses_rclone_cat(monkeypatch):
    calls = []

    def fake_run(args, **kwargs):
        calls.append(args)
        return type("R", (), {"stdout": '{"name":"b1"}'})()

    monkeypatch.setattr(ts.subprocess, "run", fake_run)
    monkeypatch.setattr(ts.shutil, "which", lambda name: "/bin/rclone")
    storage = ts.GDriveStorage({"gdrive_folder_id": ""})
    assert storage.fetch_manifest("b1")["name"] == "b1"
    assert any("cat" in call for call in calls)


def test_rclone_progress_emits_bytes(events, monkeypatch):
    class Fake:
        stderr = iter([
            "Transferred: 5 MiB / 10 MiB, 50%",
            "Transferred: 10 MiB / 10 MiB, 100%",
        ])
        returncode = 0

        def wait(self):
            return 0

    monkeypatch.setattr(ts.subprocess, "Popen", lambda *a, **k: Fake())
    ts._rclone_progress(["rclone", "copy", "a", "b"], "download")
    progress = [e for e in parsed(events) if e["event"] == "progress"]
    assert progress[-1]["current"] == 10 * 1024 * 1024
    assert progress[-1]["total"] == 10 * 1024 * 1024


def test_rclone_bytes_units():
    assert ts._bytes_from_rclone("5", "MiB") == 5 * 1024 * 1024
    assert ts._bytes_from_rclone("1.5", "GiB") == int(1.5 * 1024**3)
    assert ts._bytes_from_rclone("7", "B") == 7


def test_github_upload_emits_progress(events, tmp_path, monkeypatch):
    storage = ts.GitHubStorage({"github_token": "t", "github_repo": "o/r"})
    part = tmp_path / "home.tar.gz.part0000"
    part.write_bytes(b"x" * 32)

    class FakeResponse:
        def __enter__(self):
            return self
        def __exit__(self, *args):
            return False
        def read(self):
            return b"{}"

    monkeypatch.setattr(urllib.request, "urlopen", lambda *a, **k: FakeResponse())
    with ts.make_progress() as progress:
        task = progress.add_task("upload", total=32)
        storage._upload_one_asset(1, part, task, progress)

    progress_events = [e for e in parsed(events) if e["event"] == "progress"]
    assert progress_events[-1]["stage"] == "upload"
    assert progress_events[-1]["current"] == 32


def test_local_upload_records_checksums_and_verifies_without_rehash(tmp_path, monkeypatch):
    src = tmp_path / "src"
    src.mkdir()
    (src / "a.tar.gz").write_bytes(b"abc")
    dest = tmp_path / "dest"
    storage = ts.LocalStorage({"local_path": str(dest)})
    with ts.make_progress() as progress:
        storage.upload(src, "b1", progress)
    checksum = ts.hashlib.sha256(b"abc").hexdigest()
    assert storage.stored_checksums["a.tar.gz"] == checksum

    monkeypatch.setattr(
        ts, "sha256", lambda *a, **k: (_ for _ in ()).throw(AssertionError("rehash"))
    )
    ok, details = ts._verify_local_backup(
        dest / "b1",
        {"home": {"archive": "a.tar.gz", "checksum": checksum}},
        storage.stored_checksums,
    )
    assert ok and details == "local checksums match"


def test_local_verify_flags_corrupt_stored_file(tmp_path):
    root = tmp_path / "b1"
    root.mkdir()
    (root / "a.tar.gz").write_bytes(b"tampered")
    expected = ts.hashlib.sha256(b"abc").hexdigest()
    ok, details = ts._verify_local_backup(
        root,
        {"home": {"archive": "a.tar.gz", "checksum": expected, "size": 3}},
        {"a.tar.gz": expected},
    )
    assert not ok
    assert "size mismatch" in details
