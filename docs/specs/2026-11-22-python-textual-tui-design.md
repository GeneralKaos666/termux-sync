# Python Textual TUI — Design

Date: 2026-11-22
Status: Approved for planning
Branch: `feat/python-textual-tui`

## Context

`spike/rust-tui` proved an operations cockpit for backup and restore: a
dashboard plus live stage/progress/log screens fed by a JSON Lines event
stream from the Python engine. That branch is a Rust spike. This spec ports
the same behaviour to Python with Rich and Textual so the cockpit ships inside
the project's own language, toolchain, and test suite.

The engine today is a single-file monolith, `termux-sync.py`, mirrored
byte-for-byte into `src/termux_sync/__init__.py`. The release installer copies
only `termux-sync.py` to `$PREFIX/lib/termux-sync/`. The TUI is a separate
package that talks to the engine over a child process and JSON Lines, exactly
as the Rust spike did.

Related specs:

- `spike/rust-tui:docs/specs/2026-11-21-operations-cockpit-design.md`
- `spike/rust-tui:docs/specs/2026-11-21-restore-cockpit-design.md`

## Goals

- Provide a Textual dashboard for status, configuration, and backups.
- Stream backup progress into the TUI with named stages, an overall gauge, a
  stage gauge when a total exists, and a live log.
- Stream restore preparation, package diff, download, verification,
  extraction, and package reinstall into the TUI.
- Require an exact typed confirmation before any restore extraction.
- Cancel backup at any point, and restore before extraction starts, with the
  engine's own cleanup path running.
- Keep every existing CLI behaviour unchanged when `--json-events` is absent.
- Ship the TUI as an optional extra so the core install stays small.
- Never expose tokens, passwords, or other secrets in events or TUI state.

## Non-goals

- Reimplementing backup or restore logic in the TUI.
- Changing backup formats, manifest schema, or storage backends.
- File-level or archive-content diff for restore.
- Uninstalling or pruning device-only packages during restore.
- Automatic Termux restart or shell replacement.
- Detached or background operations after quitting the TUI.
- Pixel parity with the Rust spike's hero, icons, or animations.
- Replacing the existing Rich CLI output.

## Decisions

- **Transport:** child process plus JSON Lines on stdout. No in-process import
  of engine internals. Crash isolation, real signal-based cancellation, and an
  untouched engine are worth more than avoiding the event layer.
- **Packaging:** `textual` lives in the `tui` optional extra. `termux-sync tui`
  prints an install hint when the package or Textual is unavailable.
- **Milestones:** one spec covering dashboard, backup cockpit, and restore
  cockpit.
- **Visual direction:** Textual-native parity. Same screens and information as
  the Rust cockpit, built from Textual widgets and styling.

## Architecture

```
termux-sync.py                    # engine + CLI: + --json-events, + tui command, + event writer
src/termux_sync/__init__.py       # byte-identical mirror of the above
src/termux_sync/tui/
  __init__.py                     # run(cli_prefix) -> TermuxSyncApp
  app.py                          # App, bindings, screen routing
  dashboard.py                    # status and backup table
  operation.py                    # backup and restore run screen
  restore_plan.py                 # plan, package diff, typed confirmation
  models.py                       # OperationState, stages, gauge math
  runner.py                       # spawn, JSONL reader, stderr ring, cancel
  widgets.py                      # StageStepper, GaugeRow, LogPane
  app.tcss                        # styles and narrow breakpoints
tests/test_events.py
tests/test_tui_models.py
tests/test_tui_runner.py
tests/test_tui_screens.py
```

### Engine boundary

`cmd_tui()` in the monolith imports `termux_sync.tui` and passes the command
prefix used to spawn the engine:

- package install: `[sys.executable, "-m", "termux_sync"]`
- single-file run: `[sys.executable, __file__]`

A failed import prints an actionable error, for example:

```
TUI not available
Textual is not installed.
Install it with: pip install 'termux-sync[tui]'
```

The TUI imports nothing from the engine. Dashboard data is read directly from
disk: `~/.config/termux-sync/config.json`, `cron.json`, `sync.log`, and backup
manifests in the configured storage path, following the Rust spike's `data.rs`
approach.

### Dependency

```toml
[project.optional-dependencies]
tui = ["textual>=8.0"]
```

`textual` is not added to core dependencies.

## Event protocol

`--json-events` is additive and applies to `backup` and `restore` only.

- Enabled: UTF-8 JSON Lines on stdout, flushed per line. Rich and human output
  go to stderr.
- Absent: behaviour and output are unchanged.
- `termux-sync.py` and `src/termux_sync/__init__.py` are updated in lockstep.
- Unknown event types and unknown fields are ignored for forward compatibility.

### Envelope

```json
{"v":1,"event":"stage","ts":1763770000123,"id":"packages","label":"Collecting packages","state":"start","index":1,"total":7}
```

`v` is the schema version, currently `1`. `ts` is Unix milliseconds.

### Events

`start`

```json
{"v":1,"event":"start","ts":0,"command":"backup","name":"termux_backup_20261121_120000","storage":"local","version":"1.3.9"}
```

`stage`

```json
{"v":1,"event":"stage","ts":0,"id":"archive:home","label":"Home directory","state":"start","index":3,"total":7}
```

`state` is one of `start`, `complete`, `failed`, `skipped`.

`progress`

```json
{"v":1,"event":"progress","ts":0,"stage":"archive:home","current":12582912,"total":23907532800,"unit":"bytes","detail":"home/.cache"}
```

`current` and `total` are optional. `unit` is `bytes`, `items`, or `parts`.
`detail` is optional and redacted.

`log`

```json
{"v":1,"event":"log","ts":0,"level":"INFO","message":"Collected 1947 packages"}
```

`level` is one of `DEBUG`, `INFO`, `WARNING`, `ERROR`.

`verify`

```json
{"v":1,"event":"verify","ts":0,"state":"verified","details":"local checksums match"}
```

`state` is one of `verified`, `failed`, `skipped`.

`plan`

```json
{"v":1,"event":"plan","ts":0,"name":"termux_backup_20261121_120000","size":22300000000,"created":"2026-11-21T12:00:00Z","storage":"local","packages":1947,"targets":["home","usr/etc"]}
```

`package_diff`

```json
{"v":1,"event":"package_diff","ts":0,"chunk":1,"chunks":3,"install":["python","git"],"present":["bash"],"missing":["nano"]}
```

`result`

```json
{"v":1,"event":"result","ts":0,"ok":true,"name":"termux_backup_20261121_120000","size":22300000000,"location":"/sdcard/termux-backups/termux_backup_20261121_120000","verified":true,"duration_ms":812345}
```

`error`

```json
{"v":1,"event":"error","ts":0,"message":"Upload failed","hint":"Check the GitHub token","exit_code":1}
```

`cancelled`

```json
{"v":1,"event":"cancelled","ts":0,"reason":"user"}
```

`reason` is `user` or `declined`.

### Restore confirmation

Restore reads one line from stdin after the plan and package diff are emitted.
The accepted line is exactly:

```json
{"confirm":"RESTORE"}
```

Anything else, EOF, or `{"confirm":"no"}` counts as declined: temporary files
are cleaned and `cancelled` is emitted with reason `declined`.

### Stage ids and weights

| id | label | weight |
| --- | --- | ---: |
| `preflight` | Preflight | 2 |
| `packages` | Packages | 5 |
| `archive:home` | Home | 50 |
| `archive:usr_etc` | Config | 13 |
| `manifest` | Manifest | 5 |
| `upload` | Upload | 20 |
| `verify` | Verify | 5 |

- Skipped or absent stages are removed and remaining weights normalise to 100.
- Progress inside a stage scales that stage's weight.
- A stage without a total contributes 0 until it completes, then its full
  weight.

Restore uses its own stage list and weights:

| id | label | weight |
| --- | --- | ---: |
| `plan` | Plan | 5 |
| `download` | Download | 25 |
| `verify` | Verify | 10 |
| `extract:home` | Home | 35 |
| `extract:usr_etc` | Config | 10 |
| `packages` | Packages | 10 |
| `cleanup` | Cleanup | 5 |

The same normalisation rules apply.

### Instrumentation points

- `preflight`: before storage validation.
- `packages`: around package collection.
- `archive:home`, `archive:usr_etc`: around each archive in `cmd_backup`;
  existing `create_archive` progress callbacks emit `progress`.
- `manifest`: around `write_manifest`.
- `upload`: around `storage.upload`. Local emits per-file progress. GitHub
  streaming emits part progress. Google Drive emits start/complete only.
- `verify`: local hashing, GitHub stream digest, or Google Drive skip.
- `result`: after the success summary is known.
- `error`: where `print_error` and upload errors are handled.
- `cancelled`: from the existing `_abort` path, plus exit 130.
- Restore: `plan` and `package_diff` after the manifest is read, then
  `download`, `verify`, `extract:*`, `packages`, `cleanup`.

### Redaction

The event writer receives secret values at operation start: `github_token`,
`encrypt_password`, and any future secret keys. Every event string passes
through a redactor that replaces each non-empty secret with `***`. No config
object is serialised wholesale. Error messages and hints are redacted before
emission. The TUI never stores raw config.

## Runner

`runner.py` owns the child process:

- `Popen(prefix + [command, "--json-events"], stdout=PIPE, stderr=PIPE,
  stdin=PIPE, start_new_session=True, text=False)`.
- Reader thread parses stdout line by line: 64 KiB cap per line, lossy UTF-8
  decoding, unknown fields ignored, malformed line becomes a warning log entry.
- stderr thread keeps a bounded ring buffer of the last 200 lines for failure
  display.
- Events reach the UI through `app.call_from_thread`; no asyncio mixing.
- Cancel: `os.killpg(os.getpgid(pid), SIGINT)`, 10 s grace, then SIGKILL with
  a "cleanup may be incomplete" warning.
- EOF without `result`, `error`, or `cancelled`: synthesise a failure with the
  exit code and stderr tail. Exit 130 with no result counts as cancelled.
- One operation at a time.

## Screens

Header and footer come from Textual. One stylesheet provides `@media`
breakpoints at 80, 60, and 48 columns.

### Dashboard

Storage badge (local, gdrive, github), config summary, schedule, last-backup
age, and a `DataTable` of backups: name, date, size, packages, label, newest
first. Empty state: "No backups yet — press b".

Bindings: `b` backup, `r` restore, `l` refresh, `d` delete, `s` setup
(`d` and `s` suspend the app and hand the terminal to the existing CLI), `q`
quit, `?` help.

After a successful backup the dashboard refreshes and selects the new backup by
name. Actions are disabled while an operation runs.

### Operation

Shared by backup and restore.

- Header: operation name, storage, elapsed time, updated every second.
- Stage stepper across all stages with complete, failed, and skipped markers.
  Below 80 columns it collapses to `Home · 3/7`.
- Weighted overall `ProgressBar` and a stage bar when that stage reports a
  total. Stages without totals show static text rather than a fake sweep.
- `RichLog` follows output by default. Manual scroll pauses follow, `G`
  resumes.
- States: Running, Cancelling, Succeeded, Failed, Cancelled.
- Bindings: `Esc`/`c` cancel with confirmation, `r` retry after failure,
  `g`/`G` scroll, `?` help, `q` quit with cancel confirmation.

### Restore plan

- Plan summary: name, size, created, storage, targets.
- Package diff table with counts for install, already present, and missing.
- Destructive-action warning and the instruction to close other Termux
  sessions.
- `Input` requiring exactly `RESTORE`. Enter writes the confirmation line to
  child stdin. `Esc` declines, which produces `cancelled` with reason
  `declined`.
- Extraction stays locked until every checksum passes.

### Result and failure cards

Result: name, size, location, verification (`VERIFIED`, `CORRUPT`, or
`SKIPPED` with reason), duration. `Esc` returns to the dashboard.

Failure: message, hint, exit code, stderr tail. `r` retries, `Esc` returns.

### Help and cancel confirmation

`HelpScreen` lists bindings. `ConfirmCancelModal` asks "Cancel backup and clean
partial files?"; Enter confirms, `Esc` dismisses.

### Reduced motion

`TERMUX_SYNC_TUI_REDUCED_MOTION=1` disables animated indicators. Stage text and
gauges keep updating.

## Error handling

- App exception or `Ctrl+C`: Textual restores the terminal; the runner signals
  the child process group best-effort (SIGINT, SIGKILL after 2 s) so no orphan
  survives.
- Terminal below 48×12: size warning screen that still accepts `q`.
- Missing config: setup prompt instead of a traceback.
- Unreadable backup directory or corrupt manifest: warning banner and skipped
  rows.
- Cancel after the child exited: no-op.
- Child exit before `start`: failure card with the stderr tail.
- Write error on child stdin: failure card with the OS error.

## Testing

- `tests/test_events.py`: JSONL shape per event, backup stage order with fake
  storage and mocked archiving, redaction of a token appearing in a message,
  cancel emitting `cancelled` and exiting 130, and Rich output unchanged when
  `--json-events` is absent. No real 22 GB backup and no network.
- `tests/test_tui_models.py`: weighted gauge, skipped-stage normalisation,
  cancel transition, exit-code mapping.
- `tests/test_tui_runner.py`: fixture emitter script covering a valid stream,
  malformed line, 64 KiB overflow, non-UTF-8 bytes, EOF without `result`, and
  cancel signalling the process group.
- `tests/test_tui_screens.py`: Textual `run_test()` pilot. Dashboard renders
  rows; operation screen reacts to fake events; restore plan refuses anything
  but `RESTORE`; rendering holds at 48×12.
- `pytest-asyncio` joins the dev extras. No snapshot plugin.
- Gate: `pytest`, `ruff`, `mypy`, and a manual `termux-sync tui` smoke run.

## Acceptance criteria

- `termux-sync backup` and `termux-sync restore` without `--json-events` behave
  exactly as before.
- With `--json-events`, stdout carries only JSON Lines and human output goes to
  stderr.
- Backup shows at least the seven stages, an overall gauge, and a live log.
- Normal cancel runs the engine's cleanup, emits `cancelled`, exits, and leaves
  no temporary backup. A hard-kill timeout reports that cleanup may be
  incomplete.
- Quitting during an operation cannot leave an orphaned child.
- Backup success refreshes the dashboard and selects the new backup.
- Failure shows message and hint; retry starts a fresh operation.
- Restore shows a plan and package diff before any write, and refuses to
  extract unless the user types `RESTORE`.
- Restore blocks extraction when checksums fail.
- Verification reports `VERIFIED`, `CORRUPT`, or `SKIPPED` honestly.
- No secret appears in any event, log line, or TUI state.
- Reduced motion disables moving indicators.
- `pip install 'termux-sync[tui]'` then `termux-sync tui` launches the app.
- All Python tests, `ruff`, and `mypy` pass.

## Rollout

- Additive engine flag and a new optional extra; no migration.
- Event schema version 1; unknown events ignored.
- Both engine copies updated together.
- The TUI stays on `feat/python-textual-tui` until acceptance passes; no PyPI
  or `tsctl` release change in this milestone.
- The Rust spike branch remains untouched as reference.
