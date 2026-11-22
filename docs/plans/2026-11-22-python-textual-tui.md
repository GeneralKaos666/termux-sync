# Python Textual TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the operations cockpit (dashboard, streaming backup, restore plan) from the Rust spike to Python using Rich and Textual, fed by a `--json-events` mode in the engine.

**Architecture:** The engine stays a single-file monolith and gains an additive `--json-events` flag that writes JSON Lines to stdout while Rich output moves to stderr. A separate `termux_sync.tui` package spawns the engine as a child process, reads the event stream, and renders Textual screens. Cancellation is SIGINT to the child process group.

**Tech Stack:** Python 3.10+, Rich (already a dependency), Textual 8+ (optional extra), pytest, pytest-asyncio, ruff, mypy.

**Spec:** `docs/specs/2026-11-22-python-textual-tui-design.md`

## Global Constraints

- `requires-python = ">=3.10"`; code must not use 3.11+ syntax.
- `textual>=8.0` goes only in the `tui` optional extra, never core dependencies.
- `src/termux_sync/__init__.py` and `termux-sync.py` stay byte-identical. After every engine edit run `diff -q termux-sync.py src/termux_sync/__init__.py`.
- Tests import the package (`import termux_sync as ts`), never the script.
- Without `--json-events`, CLI behaviour and output are unchanged; every existing test must keep passing.
- With `--json-events`: stdout carries only JSON Lines, one object per line, flushed; human output goes to stderr.
- Event schema version is `1`. Unknown event types and unknown fields are ignored by the TUI.
- Secrets (`github_token`, `encrypt_password`) are replaced with `***` before any event leaves the engine.
- Ruff line length 100; `ruff check` and `mypy src/termux_sync` must pass.
- No test performs a real network upload or a full-size backup.
- Every commit is GPG-signed: `git commit -S`, then `git verify-commit HEAD`.

## Review Focus

1. Child dies immediately (bad interpreter, permission denied): the UI shows a failure card with the stderr tail and never hangs waiting for events.
2. Terminal resized below 48×12 mid-operation: no crash, operation keeps running, layout degrades to text.
3. Ctrl+C during restore extraction: extraction is not interrupted, the app still exits and leaves no orphaned child.
4. Backup directory holds a corrupt or unreadable manifest: dashboard skips that row, shows a warning banner, stays usable.
5. Storage backend prints non-UTF-8 or very long lines: decoded lossily, truncated at 64 KiB, UI stays responsive.

## File Structure

- `src/termux_sync/__init__.py` — engine, CLI, event writer (mirror: `termux-sync.py`).
- `src/termux_sync/tui/__init__.py` — `run(cli_prefix)` entry.
- `src/termux_sync/tui/app.py` — `TermuxSyncApp`, bindings, screen routing.
- `src/termux_sync/tui/dashboard.py` — snapshot loader and dashboard screen.
- `src/termux_sync/tui/operation.py` — operation screen and widgets.
- `src/termux_sync/tui/restore_plan.py` — restore plan screen.
- `src/termux_sync/tui/modals.py` — result, failure, help, cancel confirmation.
- `src/termux_sync/tui/models.py` — `Stage`, `OperationState`, gauge math, event application.
- `src/termux_sync/tui/runner.py` — child spawn, JSONL reader, cancel.
- `src/termux_sync/tui/app.tcss` — styles and narrow breakpoints.
- `tests/test_events.py`, `tests/test_tui_models.py`, `tests/test_tui_runner.py`, `tests/test_tui_screens.py`.
- `tests/fixtures/fake_engine.py` — deterministic JSONL emitter for runner and UI tests.

---

### Task 1: Event writer and redaction

**Files:**
- Modify: `src/termux_sync/__init__.py`
- Mirror: `termux-sync.py`
- Test: `tests/test_events.py`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `class EventWriter` with `__init__(self, stream, secrets: tuple[str, ...] = (), stdin=None) -> None`, `redact(self, text: str) -> str`, `emit(self, event: str, **fields) -> None`, `stage(self, stage_id: str, label: str, state: str, index: int, total: int) -> None`, `progress(self, stage: str, current=None, total=None, unit: str = "bytes", detail: str = "") -> None`, `log(self, level: str, message: str) -> None`, `confirm(self, expected: str = "RESTORE") -> bool`.
  - Module global `EVENTS: Optional[EventWriter] = None`.
  - `enable_events(stream=None, secrets: tuple[str, ...] = (), stdin=None) -> EventWriter` sets the global and returns it.
  - `_config_secrets(cfg: dict) -> tuple[str, ...]` returns non-empty `github_token` and `encrypt_password`.

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_events.py
import io
import json

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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_events.py -v`
Expected: FAIL with `AttributeError: module 'termux_sync' has no attribute 'EventWriter'`.

- [ ] **Step 3: Implement `EventWriter` in `src/termux_sync/__init__.py`**

Place it after the `log()` function. `emit` builds `{"v": 1, "event": event, "ts": int(time.time() * 1000), **redacted_fields}`, applies `redact` to every `str` value, writes `json.dumps(...) + "\n"`, and flushes. `redact` replaces each non-empty secret with `***`. `confirm` reads one line from `self.stdin or sys.stdin`, parses JSON, and returns `payload.get("confirm") == expected`, treating any parse error or EOF as `False`. `_config_secrets` returns a tuple of non-empty values for `github_token` and `encrypt_password`.

- [ ] **Step 4: Mirror the engine file and verify it is identical**

```bash
cp src/termux_sync/__init__.py termux-sync.py
diff -q termux-sync.py src/termux_sync/__init__.py
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `pytest tests/test_events.py -v`
Expected: 5 passed.

- [ ] **Step 6: Commit**

```bash
git add src/termux_sync/__init__.py termux-sync.py tests/test_events.py
git commit -S -m "feat: add JSON Lines event writer with secret redaction"
```

---

### Task 2: `--json-events` flag and stdout/stderr split

**Files:**
- Modify: `src/termux_sync/__init__.py`
- Mirror: `termux-sync.py`
- Test: `tests/test_events.py`

**Interfaces:**
- Consumes: `EventWriter`, `enable_events`, `_config_secrets` from Task 1.
- Produces: `build_parser()` gains `--json-events` (`args.json_events: bool`); `main()` rejects the flag for commands other than `backup` and `restore` with exit code 2; when enabled it swaps the module-global `console` to `Console(stderr=True)` and restores it in a `finally` block.

- [ ] **Step 1: Write the failing tests**

```python
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
    assert out.strip()
    for line in out.splitlines():
        assert json.loads(line)["v"] == 1
    assert "Backup" in err


def test_json_events_restores_console_afterwards(local_cfg, termux_tree, monkeypatch):
    before = ts.console
    monkeypatch.setattr(ts, "load_config", lambda: local_cfg)
    ts.main(["backup", "--json-events"])
    assert ts.console is before
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_events.py -v -k json_events`
Expected: FAIL — `args.json_events` missing.

- [ ] **Step 3: Implement the flag plumbing**

Add `parser.add_argument("--json-events", action="store_true")` in `build_parser`. In `main()`, after parsing, if `args.json_events` and `args.command not in ("backup", "restore")`, call `print_error("Invalid usage", "--json-events is only supported for backup and restore.")` and `sys.exit(2)`. After `cfg = load_config()`, when the flag is set: save `old_console = console`, assign `console = Console(stderr=True)`, call `enable_events(sys.stdout, secrets=_config_secrets(cfg))`, and restore both globals in a `finally` around the handler dispatch (`console = old_console`, `EVENTS = None`) so no test or later command inherits them.

- [ ] **Step 4: Mirror the engine file**

```bash
cp src/termux_sync/__init__.py termux-sync.py
diff -q termux-sync.py src/termux_sync/__init__.py
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `pytest tests/test_events.py tests/test_cli.py -v`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/termux_sync/__init__.py termux-sync.py tests/test_events.py
git commit -S -m "feat: add --json-events flag with stderr human output"
```

---

### Task 3: Backup instrumentation

**Files:**
- Modify: `src/termux_sync/__init__.py` (`cmd_backup`, `_abort`)
- Mirror: `termux-sync.py`
- Test: `tests/test_events.py`

**Interfaces:**
- Consumes: `EVENTS` global from Task 1.
- Produces: `_emit(event: str, **fields) -> None` that no-ops when `EVENTS is None`; `_verify_backup(cfg: dict, storage, backup_name: str, meta: dict) -> tuple[bool | None, str]` returning `(verified, details)`; backup emits `start`, `stage`, `progress`, `log`, `verify`, `result`, `error`, `cancelled`.

- [ ] **Step 1: Write the failing tests**

```python
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_events.py -v -k backup`
Expected: FAIL — no `stage` events.

- [ ] **Step 3: Instrument `cmd_backup` and `_abort`**

In `cmd_backup`: emit `start` with `command="backup"`, `name`, `storage`, `version`; emit `stage` for `packages`, `archive:home`, `archive:usr_etc` (skipped when the source is missing), `manifest`, `upload`, `verify`, each with `index`/`total`; emit `log` at the existing `log()` call sites; emit `progress` from the archive loop using bytes completed per archive; emit `verify` with `state` from `_verify_backup`; emit `result` on success with `name`, `size`, `location`, `verified`, `duration_ms`; emit `error` in the upload-failure branch with redacted `message`, `hint`, `exit_code=1`. `_verify_backup` hashes each stored archive for `local` storage, trusts the stream digest for GitHub streaming, and returns `(None, "Google Drive cannot verify remotely")` for gdrive. In `_abort`, call `_emit("cancelled", reason="user")` before the existing exit.

- [ ] **Step 4: Mirror the engine file**

```bash
cp src/termux_sync/__init__.py termux-sync.py
diff -q termux-sync.py src/termux_sync/__init__.py
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `pytest tests/test_events.py tests/test_commands.py -v`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/termux_sync/__init__.py termux-sync.py tests/test_events.py
git commit -S -m "feat: emit backup events for the TUI cockpit"
```

---

### Task 4: Restore instrumentation and typed confirmation

**Files:**
- Modify: `src/termux_sync/__init__.py` (`cmd_restore`)
- Mirror: `termux-sync.py`
- Test: `tests/test_events.py`

**Interfaces:**
- Consumes: `EVENTS.confirm(expected="RESTORE")`, `_emit`, `_verify_backup` pattern from Task 3.
- Produces: restore emits `plan`, chunked `package_diff`, `stage`, `verify`, `result`, `error`, `cancelled`. In event mode it never calls `Confirm.ask`; the confirmation comes from stdin. Checksum failure in event mode emits `verify` failed plus `error` and exits 1 without extracting.

- [ ] **Step 1: Write the failing tests**

```python
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
    return _events(capsys), code


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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_events.py -v -k restore`
Expected: FAIL — no `plan` event.

- [ ] **Step 3: Instrument `cmd_restore`**

Emit `plan` after the manifest is read, with `name`, `size`, `created`, `storage`, `packages`, `targets`. Compute the package diff against `get_installed_packages()` and emit `package_diff` in chunks of 200 names with `chunk`/`chunks`/`install`/`present`/`missing`. Emit `stage` for `plan`, `download`, `verify`, `extract:home`, `extract:usr_etc`, `packages`, `cleanup`. When `EVENTS` is active, replace both `Confirm.ask` calls: the preview confirmation uses `EVENTS.confirm("RESTORE")`; a false answer cleans the temp directory and emits `cancelled` with reason `declined`, then exits 130. When checksums fail in event mode, emit `verify` failed and `error` with `exit_code=1`, clean up, and exit 1 — no "continue anyway" prompt. Wrap each `extract_archive` call in `except KeyboardInterrupt` that emits a `log` warning containing "cancel deferred until restore finishes" and continues with the next archive, so extraction and package reinstall are never interrupted. Emit `result` after a successful restore.

- [ ] **Step 4: Mirror the engine file**

```bash
cp src/termux_sync/__init__.py termux-sync.py
diff -q termux-sync.py src/termux_sync/__init__.py
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `pytest tests/test_events.py tests/test_commands.py -v`
Expected: all pass, including the existing CLI restore tests.

- [ ] **Step 6: Commit**

```bash
git add src/termux_sync/__init__.py termux-sync.py tests/test_events.py
git commit -S -m "feat: emit restore plan and package diff events"
```

---

### Task 5: `tui` command and optional extra

**Files:**
- Modify: `src/termux_sync/__init__.py` (`cmd_tui`, `COMMAND_GROUPS`, handler map)
- Mirror: `termux-sync.py`
- Modify: `pyproject.toml`
- Test: `tests/test_cli.py`

**Interfaces:**
- Consumes: `termux_sync.tui.run(cli_prefix: list[str])` from Task 8.
- Produces: `cmd_tui() -> None`; `tui` appears in `COMMANDS`; `[project.optional-dependencies]` gains `tui = ["textual>=8.0"]` and dev gains `pytest-asyncio>=0.23`.

- [ ] **Step 1: Write the failing tests**

```python
# add `import sys` to the imports at the top of tests/test_cli.py
def test_tui_is_a_known_command():
    assert "tui" in ts.COMMANDS


def test_tui_reports_missing_textual(monkeypatch, capsys):
    monkeypatch.setitem(sys.modules, "termux_sync.tui", None)
    ts.cmd_tui()
    out = capsys.readouterr().out
    assert "TUI not available" in out
    assert "pip install 'termux-sync[tui]'" in out
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_cli.py -v -k tui`
Expected: FAIL — `tui` not in `COMMANDS`.

- [ ] **Step 3: Implement `cmd_tui` and packaging**

Add `("tui", "Open the full-screen interface")` to an existing `COMMAND_GROUPS` entry and map `"tui": lambda: cmd_tui()` in `main()`. `cmd_tui` imports `termux_sync.tui`; on `ImportError` it calls `print_error("TUI not available", "Textual is not installed.", "Install it with: pip install 'termux-sync[tui]'")` and returns. Otherwise it builds the prefix — `[sys.executable, "-m", "termux_sync"]` when `__package__` is set, else `[sys.executable, os.path.abspath(__file__)]` — and calls `run(prefix)`.

- [ ] **Step 4: Mirror the engine file and update `pyproject.toml`**

```bash
cp src/termux_sync/__init__.py termux-sync.py
diff -q termux-sync.py src/termux_sync/__init__.py
```

Add to `pyproject.toml`:

```toml
tui = ["textual>=8.0"]
```

and append `"pytest-asyncio>=0.23"` to the `dev` extra. Add `asyncio_mode = "auto"` under `[tool.pytest.ini_options]`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `pytest tests/test_cli.py -v`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/termux_sync/__init__.py termux-sync.py pyproject.toml tests/test_cli.py
git commit -S -m "feat: add tui command and optional textual extra"
```

---

### Task 6: Operation models and gauge math

**Files:**
- Create: `src/termux_sync/tui/__init__.py`, `src/termux_sync/tui/models.py`
- Test: `tests/test_tui_models.py`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `BACKUP_STAGES: dict[str, int]` = `{"preflight": 2, "packages": 5, "archive:home": 50, "archive:usr_etc": 13, "manifest": 5, "upload": 20, "verify": 5}`.
  - `RESTORE_STAGES: dict[str, int]` = `{"plan": 5, "download": 25, "verify": 10, "extract:home": 35, "extract:usr_etc": 10, "packages": 10, "cleanup": 5}`.
  - `@dataclass class Stage` with `id: str`, `label: str`, `state: str = "pending"`, `weight: int = 0`, `current: int | None = None`, `total: int | None = None`.
  - `@dataclass class OperationState` with `command: str`, `stages: list[Stage]`, `status: str = "running"`, `name: str = ""`, `storage: str = ""`, `verified: bool | None = None`, `verify_detail: str = ""`, `result: dict | None = None`, `failure: dict | None = None`, `logs: deque[str]`, `started_at: float`; methods `overall_percent(self) -> float` and `current_stage(self) -> Stage | None`.
  - `new_operation(command: str) -> OperationState`.
  - `apply_event(state: OperationState, event: dict) -> None`.

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_tui_models.py
from termux_sync.tui.models import apply_event, new_operation


def test_new_backup_operation_has_weighted_stages():
    state = new_operation("backup")
    assert [s.id for s in state.stages][0] == "preflight"
    assert sum(s.weight for s in state.stages) == 100
    assert state.overall_percent() == 0.0


def test_completed_stage_adds_its_weight():
    state = new_operation("backup")
    apply_event(state, {"event": "stage", "id": "preflight", "label": "Preflight", "state": "complete", "index": 1, "total": 7})
    assert state.overall_percent() == 2.0


def test_progress_inside_stage_scales_weight():
    state = new_operation("backup")
    apply_event(state, {"event": "stage", "id": "archive:home", "label": "Home", "state": "start", "index": 3, "total": 7})
    apply_event(state, {"event": "progress", "stage": "archive:home", "current": 50, "total": 100})
    assert state.overall_percent() == 25.0


def test_skipped_stage_normalises_remaining_weights():
    state = new_operation("backup")
    apply_event(state, {"event": "stage", "id": "preflight", "label": "Preflight", "state": "skipped", "index": 1, "total": 7})
    assert [s.id for s in state.stages if s.state == "skipped"] == []
    assert sum(s.weight for s in state.stages) == 100


def test_unknown_stage_is_ignored():
    state = new_operation("backup")
    apply_event(state, {"event": "stage", "id": "future:thing", "label": "?", "state": "start", "index": 1, "total": 1})
    assert all(s.id != "future:thing" for s in state.stages)


def test_result_and_error_set_terminal_status():
    ok = new_operation("backup")
    apply_event(ok, {"event": "result", "ok": True, "name": "b1", "size": 10, "verified": True})
    assert ok.status == "succeeded" and ok.result["name"] == "b1"
    bad = new_operation("backup")
    apply_event(bad, {"event": "error", "message": "boom", "hint": "fix", "exit_code": 1})
    assert bad.status == "failed" and bad.failure["message"] == "boom"
    stopped = new_operation("backup")
    apply_event(stopped, {"event": "cancelled", "reason": "user"})
    assert stopped.status == "cancelled"


def test_restore_stages_use_restore_weights():
    state = new_operation("restore")
    assert [s.id for s in state.stages][0] == "plan"
    assert sum(s.weight for s in state.stages) == 100
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_tui_models.py -v`
Expected: FAIL — `ModuleNotFoundError: termux_sync.tui`.

- [ ] **Step 3: Implement `models.py`**

`new_operation` builds `Stage` objects from the weight dict for the command. `overall_percent` sums completed weights plus the in-progress fraction of the current stage (`current / total`, or `0` when no total), divided by the live total weight. A `skipped` stage is removed and the remaining weights are rescaled to 100. `apply_event` dispatches on `event`: `start` records `name`/`storage`; `stage` updates or completes; `progress` sets `current`/`total`; `log` appends to `logs` (cap 2000); `verify` sets `verified`/`verify_detail`; `result`, `error`, `cancelled` set the terminal status.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_tui_models.py -v`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src/termux_sync/tui/__init__.py src/termux_sync/tui/models.py tests/test_tui_models.py
git commit -S -m "feat: add TUI operation models and gauge math"
```

---

### Task 7: Child process runner

**Files:**
- Create: `src/termux_sync/tui/runner.py`, `tests/fixtures/fake_engine.py`
- Test: `tests/test_tui_runner.py`

**Interfaces:**
- Consumes: nothing.
- Produces: `class OperationRunner` with `__init__(self, cli_prefix: list[str], on_event: Callable[[dict], None], on_exit: Callable[[int], None])`, `start(self, command: str, name: str = "") -> None`, `confirm(self, text: str = "RESTORE") -> None`, `cancel(self) -> None`, `kill(self) -> None`, `close(self) -> None`, `pid: int | None`.

- [ ] **Step 1: Write the fake engine and the failing tests**

```python
# tests/fixtures/fake_engine.py
import json
import sys
import time

mode = sys.argv[2] if len(sys.argv) > 2 else "ok"
if mode == "ok":
    print(json.dumps({"v": 1, "event": "start", "command": "backup", "name": "fake"}))
    print(json.dumps({"v": 1, "event": "result", "ok": True, "name": "fake", "size": 1, "verified": True}))
elif mode == "garbage":
    print("{not json")
    print(json.dumps({"v": 1, "event": "result", "ok": True, "name": "fake", "size": 1, "verified": True}))
elif mode == "huge":
    print("x" * 70000)
elif mode == "nonutf8":
    sys.stdout.buffer.write(b'{"v":1,"event":"log","message":"\xff\xfe"}\n')
    sys.stdout.buffer.flush()
elif mode == "silent":
    sys.exit(3)
elif mode == "hang":
    time.sleep(60)
```

```python
# tests/test_tui_runner.py
import sys
from pathlib import Path

import pytest

from termux_sync.tui.runner import OperationRunner

FAKE = Path(__file__).parent / "fixtures" / "fake_engine.py"


def collect(mode, timeout=10):
    events, exits = [], []
    done = threading.Event()
    runner = OperationRunner(
        [sys.executable, str(FAKE)],
        on_event=events.append,
        on_exit=lambda code: (exits.append(code), done.set()),
    )
    runner.start("backup", mode)
    assert done.wait(timeout)
    runner.close()
    return events, exits


def test_streams_events_and_exit_code():
    events, exits = collect("ok")
    assert [e["event"] for e in events] == ["start", "result"]
    assert exits == [0]


def test_malformed_line_becomes_warning():
    events, _ = collect("garbage")
    warnings = [e for e in events if e["event"] == "log" and e["level"] == "WARNING"]
    assert warnings
    assert [e["event"] for e in events][-1] == "result"


def test_oversized_line_is_discarded_with_warning():
    events, _ = collect("huge")
    assert any(e.get("level") == "WARNING" for e in events if e["event"] == "log")


def test_non_utf8_line_decodes_lossily():
    events, _ = collect("nonutf8")
    assert any("message" in e for e in events if e["event"] == "log")


def test_exit_without_result_is_reported():
    events, exits = collect("silent")
    assert events == []
    assert exits == [3]


def test_cancel_signals_process_group():
    events, exits = collect("hang", timeout=0.1)
    runner = OperationRunner([sys.executable, str(FAKE)], on_event=events.append, on_exit=lambda c: None)
    runner.start("backup", "hang")
    runner.cancel()
    assert runner.wait(10) == 130
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_tui_runner.py -v`
Expected: FAIL — `ModuleNotFoundError: termux_sync.tui.runner`.

- [ ] **Step 3: Implement `runner.py`**

`start` spawns `subprocess.Popen(cli_prefix + [command, "--json-events", "--name", name] if name else cli_prefix + [command, "--json-events"], stdin=PIPE, stdout=PIPE, stderr=PIPE, start_new_session=True)`. One thread reads stdout with `readline(65536)`; longer lines are discarded with a synthetic warning event; each line is parsed with `json.JSONDecodeError` caught into a warning; bytes are decoded with `errors="replace"`. A second thread keeps the last 200 stderr lines. A third thread waits for the process and calls `on_exit(returncode)`. `cancel` sends `os.killpg(os.getpgid(pid), signal.SIGINT)`. `wait(timeout)` joins the process and returns the code, sending SIGKILL after 10 s. `confirm` writes `json.dumps({"confirm": text}) + "\n"` to stdin and flushes.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_tui_runner.py -v`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src/termux_sync/tui/runner.py tests/fixtures/fake_engine.py tests/test_tui_runner.py
git commit -S -m "feat: add TUI child process runner"
```

---

### Task 8: App shell, snapshot loader, dashboard

**Files:**
- Create: `src/termux_sync/tui/app.py`, `src/termux_sync/tui/dashboard.py`, `src/termux_sync/tui/app.tcss`
- Modify: `src/termux_sync/tui/__init__.py`
- Test: `tests/test_tui_screens.py`

**Interfaces:**
- Consumes: `OperationRunner` (Task 7).
- Produces: `run(cli_prefix: list[str]) -> None`; `class Snapshot` with `storage: str`, `local_path: str`, `compression: str`, `max_backups: int`, `schedule: str`, `backups: list[dict]`, `warning: str | None`; `load_snapshot(config_dir: Path | None = None) -> Snapshot`; `class TermuxSyncApp(App)` with `cli_prefix`, `snapshot`, `BINDINGS`, screens `dashboard`, `operation`, `restore-plan`.

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_tui_screens.py
import json
from pathlib import Path

import pytest
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


def make_app(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    (tmp_path / ".config" / "termux-sync").mkdir(parents=True)
    (tmp_path / ".config" / "termux-sync" / "config.json").write_text(json.dumps({"storage": "local", "local_path": str(tmp_path / "backups")}))
    return TermuxSyncApp(cli_prefix=["python"])


def test_snapshot_lists_backups_newest_first(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    write_backup(tmp_path / "backups", "termux_backup_20260101_000000")
    write_backup(tmp_path / "backups", "termux_backup_20260202_000000")
    snap = load_snapshot(tmp_path / ".config" / "termux-sync")
    assert [b["name"] for b in snap.backups] == ["termux_backup_20260202_000000", "termux_backup_20260101_000000"]


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
        assert "No backups yet" in str(app.screen.query_one("#empty").renderable)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_tui_screens.py -v`
Expected: FAIL — `ModuleNotFoundError: termux_sync.tui.app`.

- [ ] **Step 3: Implement the app shell, loader, and dashboard**

`load_snapshot` reads `config.json`, `cron.json`, and each subdirectory's `manifest.json`; unreadable or invalid manifests are skipped and counted into `warning`. A missing `config.json` yields `storage = "unconfigured"` and a dashboard banner pointing at `s` (setup) instead of an exception. `TermuxSyncApp` sets `TITLE = "termux-sync"`, installs the dashboard as the initial screen, and stores `cli_prefix`. The dashboard uses `Header`, a status bar showing storage/compression/schedule, a `DataTable(id="backups")` with columns name, date, size, packages, label, an `#empty` `Static` for the empty state, and `Footer`. Bindings `b`, `r`, `l`, `d`, `s`, `q`, `?` are declared; `l` reloads the snapshot, `q` exits, others are wired in later tasks. `app.tcss` sets the layout and a `@media (max-width: 79)` block that hides the label column and shortens dates.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_tui_screens.py -v`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src/termux_sync/tui/app.py src/termux_sync/tui/dashboard.py src/termux_sync/tui/app.tcss src/termux_sync/tui/__init__.py tests/test_tui_screens.py
git commit -S -m "feat: add TUI app shell and dashboard"
```

---

### Task 9: Operation screen

**Files:**
- Create: `src/termux_sync/tui/operation.py`, `src/termux_sync/tui/widgets.py`
- Modify: `src/termux_sync/tui/app.py`, `src/termux_sync/tui/app.tcss`
- Test: `tests/test_tui_screens.py`

**Interfaces:**
- Consumes: `OperationState`, `apply_event`, `OperationRunner`.
- Produces: `class OperationScreen(Screen)` with `__init__(self, cli_prefix: list[str], command: str, backup_name: str = "")`, `state: OperationState`, `runner: OperationRunner | None`, methods `handle_event(self, event: dict) -> None`, `handle_exit(self, code: int) -> None`, `action_cancel(self) -> None`, `action_retry(self) -> None`; `class StageStepper(Static)`, `class GaugeRow(Vertical)`.

- [ ] **Step 1: Write the failing tests**

```python
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_tui_screens.py -v -k operation`
Expected: FAIL — `ModuleNotFoundError: termux_sync.tui.operation`.

- [ ] **Step 3: Implement the operation screen**

`OperationScreen` creates `OperationState` via `new_operation`, starts `OperationRunner` in `on_mount`, and routes `handle_event` through `apply_event` before refreshing the header (name, storage, elapsed from a 1 s `set_interval`), `StageStepper`, `GaugeRow`, and `RichLog(id="log")`. `StageStepper` renders every stage with `✓`, `✗`, `·` markers; under 80 columns it renders only `current label · index/total`. When the terminal is smaller than 48×12 the screen shows a `#size-warning` `Static` and keeps the operation running underneath, so shrinking the terminal never aborts work. `GaugeRow` holds the overall `ProgressBar` and a stage `ProgressBar`; a stage without a total renders a `Static` "measuring…" instead. Failure and success swap in the `#failure` and `#result` cards. `handle_exit` synthesises a failure when the stream ended without a terminal event, treating 130 as cancelled. `action_cancel` opens `ConfirmCancelModal`; on confirm it calls `runner.cancel()` and sets `status="cancelling"`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_tui_screens.py -v -k operation`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add src/termux_sync/tui/operation.py src/termux_sync/tui/widgets.py src/termux_sync/tui/app.py src/termux_sync/tui/app.tcss tests/test_tui_screens.py
git commit -S -m "feat: add TUI operation screen with live stages and log"
```

---

### Task 10: Restore plan screen

**Files:**
- Create: `src/termux_sync/tui/restore_plan.py`
- Modify: `src/termux_sync/tui/app.py`
- Test: `tests/test_tui_screens.py`

**Interfaces:**
- Consumes: `OperationScreen`, `OperationRunner`.
- Produces: `class RestorePlanScreen(Screen)` with `__init__(self, cli_prefix: list[str], backup_name: str)`, `plan: dict | None`, `diff: dict[str, list[str]]`, `handle_event(self, event: dict) -> None`, `action_decline(self) -> None`, `submit_confirmation(self, value: str) -> None`.

- [ ] **Step 1: Write the failing tests**

```python
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_tui_screens.py -v -k restore_plan`
Expected: FAIL — module missing.

- [ ] **Step 3: Implement the plan screen**

The screen starts the runner with `restore --name <backup_name>` on mount, collects `plan` and `package_diff` chunks into `self.diff`, and renders a summary panel plus a `DataTable` of install/present/missing counts. An `Input(id="confirm")` with placeholder `Type RESTORE to continue` drives `submit_confirmation`, which sets `can_confirm()` true only for the exact uppercase token and then writes it to the child via `runner.confirm("RESTORE")`. `Esc` calls `action_decline`, which sets `declined = True` and lets the engine clean up. Below 80 columns the diff table collapses to counts only.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_tui_screens.py -v -k restore_plan`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/termux_sync/tui/restore_plan.py src/termux_sync/tui/app.py tests/test_tui_screens.py
git commit -S -m "feat: add TUI restore plan screen with typed confirmation"
```

---

### Task 11: Modals, reduced motion, and handoff actions

**Files:**
- Create: `src/termux_sync/tui/modals.py`
- Modify: `src/termux_sync/tui/app.py`, `src/termux_sync/tui/dashboard.py`, `src/termux_sync/tui/app.tcss`
- Test: `tests/test_tui_screens.py`

**Interfaces:**
- Consumes: dashboard and operation screens.
- Produces: `class ConfirmCancelModal(ModalScreen[bool])`, `class HelpScreen(ModalScreen)`, `class ResultCard(Static)`, `class FailureCard(Static)`; `reduced_motion() -> bool` reading `TERMUX_SYNC_TUI_REDUCED_MOTION`.

- [ ] **Step 1: Write the failing tests**

```python
# add `import contextlib` to tests/test_tui_screens.py
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_tui_screens.py -v -k "modal or motion or suspend"`
Expected: FAIL — modals module missing.

- [ ] **Step 3: Implement modals, motion flag, and CLI handoff**

`ConfirmCancelModal` returns `True` on Enter and `False` on Esc. `HelpScreen` lists the bindings. `ResultCard` and `FailureCard` render the fields named in the spec. `reduced_motion` reads the environment variable. `dashboard.run_cli(cli_prefix, argv)` runs `subprocess.run(cli_prefix + argv)` inside `app.suspend()` so `d` (delete) and `s` (setup) hand the terminal to the existing CLI and then reload the snapshot. When `reduced_motion()` is true, `app.tcss` classes drop the `ProgressBar` pulse and any transition durations.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_tui_screens.py -v`
Expected: all screen tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/termux_sync/tui/modals.py src/termux_sync/tui/app.py src/termux_sync/tui/dashboard.py src/termux_sync/tui/app.tcss tests/test_tui_screens.py
git commit -S -m "feat: add TUI modals, reduced motion, and CLI handoff"
```

---

### Task 12: End-to-end smoke, docs, and full gate

**Files:**
- Modify: `README.md`, `CHANGELOG.md`
- Test: `tests/test_tui_runner.py` (add integration test)

**Interfaces:**
- Consumes: everything above.
- Produces: documented `termux-sync tui` usage and a passing full gate.

- [ ] **Step 1: Add the integration test**

```python
def test_fake_engine_stream_drives_operation_state():
    from termux_sync.tui.models import apply_event, new_operation

    state = new_operation("backup")
    events, _ = collect("ok")
    for event in events:
        apply_event(state, event)
    assert state.status == "succeeded"
    assert state.result["verified"] is True
```

- [ ] **Step 2: Run it**

Run: `pytest tests/test_tui_runner.py::test_fake_engine_stream_drives_operation_state -v`
Expected: PASS. If it fails, the runner or `apply_event` contract broke in an earlier task — fix that task, not this test.

- [ ] **Step 3: Document the feature**

Reuse the existing `collect` helper. Add a `README.md` section under usage:

```markdown
### Full-screen interface

    pip install 'termux-sync[tui]'
    termux-sync tui

Keys: `b` backup, `r` restore, `l` refresh, `d` delete, `s` setup, `q` quit.
Inside an operation: `Esc`/`c` cancel, `r` retry, `g`/`G` scroll, `?` help.
```

Add a `CHANGELOG.md` entry under the unreleased heading for the event mode and the TUI.

- [ ] **Step 4: Run the full gate**

```bash
pytest -q
ruff check src tests
mypy src/termux_sync
diff -q termux-sync.py src/termux_sync/__init__.py
```

Expected: all pass, diff silent.

- [ ] **Step 5: Manual smoke**

```bash
uv pip install -e '.[tui,dev]'
termux-sync tui
```

Expected: dashboard renders, `b` streams a real backup with stages and a log, `Esc` cancels with confirmation, `q` exits with the terminal restored and no `termux-sync` child left behind (`pgrep -f "termux-sync.*backup"` empty).

- [ ] **Step 6: Commit**

```bash
git add README.md CHANGELOG.md tests/test_tui_runner.py
git commit -S -m "docs: document the Textual TUI and event mode"
```
