import io
import json

import pytest

import termux_sync as ts


def test_emit_writes_one_json_line_with_version_and_timestamp():
    buf = io.StringIO()
    writer = ts.EventWriter(buf)
    writer.emit("log", level="INFO", message="hello")
    payload = json.loads(buf.getvalue())
    assert payload["v"] == 1
    assert payload["event"] == "log"
    assert payload["level"] == "INFO"
    assert payload["message"] == "hello"
    assert isinstance(payload["ts"], int)
    assert buf.getvalue().endswith("\n")


def test_stage_helper_matches_envelope():
    buf = io.StringIO()
    ts.EventWriter(buf).stage("packages", "Packages", "start", 1, 7)
    payload = json.loads(buf.getvalue())
    assert payload["event"] == "stage"
    assert payload["id"] == "packages"
    assert payload["state"] == "start"
    assert payload["index"] == 1
    assert payload["total"] == 7


def test_redacts_every_secret_in_string_fields():
    buf = io.StringIO()
    writer = ts.EventWriter(buf, secrets=("tok-123", "hunter2"))
    writer.emit("error", message="bad tok-123", hint="hunter2 wrong", exit_code=1)
    raw = buf.getvalue()
    assert "tok-123" not in raw
    assert "hunter2" not in raw
    payload = json.loads(raw)
    assert payload["message"] == "bad ***"
    assert payload["hint"] == "*** wrong"


def test_confirm_reads_exact_restore_token():
    stdin = io.StringIO('{"confirm":"RESTORE"}\n')
    assert ts.EventWriter(io.StringIO(), stdin=stdin).confirm() is True
    assert ts.EventWriter(io.StringIO(), stdin=io.StringIO('{"confirm":"no"}\n')).confirm() is False
    assert ts.EventWriter(io.StringIO(), stdin=io.StringIO("")).confirm() is False


def test_config_secrets_skips_empty_values():
    assert ts._config_secrets({"github_token": "t", "encrypt_password": ""}) == ("t",)


def test_parser_accepts_json_events():
    args = ts.build_parser().parse_args(["backup", "--json-events"])
    assert args.json_events is True


def test_json_events_rejected_for_read_only_commands(capsys):
    with pytest.raises(SystemExit) as info:
        ts.main(["list", "--json-events"])
    assert info.value.code == 2
    assert "--json-events" in capsys.readouterr().out


def test_json_events_keeps_stdout_clean(local_cfg, termux_tree, monkeypatch, capsys):
    monkeypatch.setattr(ts, "load_config", lambda: local_cfg)
    ts.main(["backup", "--json-events"])
    out, err = capsys.readouterr()
    for line in out.splitlines():
        assert json.loads(line)["v"] == 1
    assert "Backup" in err


def test_json_events_restores_console_afterwards(local_cfg, termux_tree, monkeypatch):
    before = ts.console
    monkeypatch.setattr(ts, "load_config", lambda: local_cfg)
    ts.main(["backup", "--json-events"])
    assert ts.console is before


def _events(capsys):
    out, err = capsys.readouterr()
    return [json.loads(line) for line in out.splitlines()], err


def _run_json(monkeypatch, capsys, argv, cfg):
    monkeypatch.setattr(ts, "load_config", lambda: cfg)
    ts.main(argv)
    return _events(capsys)


def test_backup_emits_stages_in_order(termux_tree, local_cfg, monkeypatch, capsys):
    events, _ = _run_json(monkeypatch, capsys, ["backup", "--json-events"], local_cfg)
    started = [e["id"] for e in events if e["event"] == "stage" and e["state"] == "start"]
    assert started == ["packages", "archive:home", "archive:usr_etc", "manifest", "upload", "verify"]


def test_backup_emits_result_with_verification(termux_tree, local_cfg, monkeypatch, capsys):
    events, _ = _run_json(monkeypatch, capsys, ["backup", "--json-events"], local_cfg)
    result = [e for e in events if e["event"] == "result"]
    assert len(result) == 1
    assert result[0]["ok"] is True
    assert result[0]["verified"] is True
    assert result[0]["name"].startswith("termux_backup_")
    assert result[0]["size"] > 0


def test_backup_upload_failure_emits_error_without_result(termux_tree, local_cfg, monkeypatch, capsys):
    def boom(*args, **kwargs):
        raise OSError("upload exploded")

    monkeypatch.setattr(ts.LocalStorage, "upload", boom)
    events, _ = _run_json(monkeypatch, capsys, ["backup", "--json-events"], local_cfg)
    errors = [e for e in events if e["event"] == "error"]
    assert errors and errors[0]["exit_code"] == 1
    assert not [e for e in events if e["event"] == "result"]


def test_backup_cancel_emits_cancelled_and_exits_130(termux_tree, local_cfg, monkeypatch, capsys):
    monkeypatch.setattr(ts, "load_config", lambda: local_cfg)
    monkeypatch.setattr(ts, "get_installed_packages", lambda: (_ for _ in ()).throw(KeyboardInterrupt()))
    with pytest.raises(SystemExit) as info:
        ts.main(["backup", "--json-events"])
    assert info.value.code == 130
    events, _ = _events(capsys)
    assert [e["reason"] for e in events if e["event"] == "cancelled"] == ["user"]


def test_backup_without_flag_emits_no_json(local_cfg, termux_tree, monkeypatch, capsys):
    monkeypatch.setattr(ts, "load_config", lambda: local_cfg)
    ts.main(["backup"])
    out, _ = capsys.readouterr()
    assert not any(line.startswith("{") for line in out.splitlines())


def test_verify_backup_reports_skip_for_gdrive(termux_tree, local_cfg, monkeypatch, capsys):
    cfg = dict(local_cfg, storage="gdrive")
    monkeypatch.setattr(ts, "get_storage", lambda c: type("G", (), {
        "upload": lambda self, d, n, p: "gdrive://x",
        "delete_old": lambda self, n: None,
    })())
    events, _ = _run_json(monkeypatch, capsys, ["backup", "--json-events"], cfg)
    verify = [e for e in events if e["event"] == "verify"]
    assert verify and verify[0]["state"] == "skipped"
    result = [e for e in events if e["event"] == "result"][0]
    assert result["verified"] is None


def _backup_then_restore(monkeypatch, capsys, cfg, confirm, corrupt=False):
    monkeypatch.setattr(ts, "load_config", lambda: cfg)
    ts.main(["backup", "--json-events"])
    capsys.readouterr()
    if corrupt:
        root = ts.LocalStorage(cfg).root
        archive = next(root.glob("termux_backup_*/*.tar.*"))
        archive.write_bytes(b"not a tarball")
    monkeypatch.setattr(ts.sys, "stdin", io.StringIO(json.dumps({"confirm": confirm}) + "\n"))
    code = None
    try:
        ts.main(["restore", "--json-events"])
    except SystemExit as exc:
        code = exc.code
    events, _ = _events(capsys)
    return events, code


def test_restore_emits_plan_and_package_diff(termux_tree, local_cfg, monkeypatch, capsys):
    events, code = _backup_then_restore(monkeypatch, capsys, local_cfg, "RESTORE")
    plan = [e for e in events if e["event"] == "plan"]
    assert plan and plan[0]["packages"] == 2
    assert [e for e in events if e["event"] == "package_diff"]
    assert code in (0, None)


def test_restore_declined_emits_cancelled_and_extracts_nothing(termux_tree, local_cfg, monkeypatch, capsys):
    target = termux_tree / "home" / ".bashrc"
    target.write_text("changed after backup\n")
    events, _ = _backup_then_restore(monkeypatch, capsys, local_cfg, "no")
    assert [e["reason"] for e in events if e["event"] == "cancelled"] == ["declined"]
    assert target.read_text() == "changed after backup\n"


def test_restore_checksum_failure_blocks_extraction(termux_tree, local_cfg, monkeypatch, capsys):
    target = termux_tree / "home" / ".bashrc"
    target.write_text("keep me\n")
    events, code = _backup_then_restore(monkeypatch, capsys, local_cfg, "RESTORE", corrupt=True)
    assert [e["state"] for e in events if e["event"] == "verify"] == ["failed"]
    assert [e for e in events if e["event"] == "error"]
    assert target.read_text() == "keep me\n"
    assert code == 1


def test_restore_defers_interrupt_during_extraction(termux_tree, local_cfg, monkeypatch, capsys):
    calls = []

    def interrupting(archive, dest, name, progress, expected_top=None):
        calls.append(name)
        raise KeyboardInterrupt()

    monkeypatch.setattr(ts, "extract_archive", interrupting)
    events, _ = _backup_then_restore(monkeypatch, capsys, local_cfg, "RESTORE")
    assert len(calls) == 2
    assert any(e["event"] == "log" and "deferred" in e["message"].lower() for e in events)
