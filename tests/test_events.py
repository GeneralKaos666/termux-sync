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
