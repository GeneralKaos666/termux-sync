# termux-sync-tui (spike)

Ratatui dashboard around the existing `termux-sync` CLI. This is a throwaway
spike on branch `spike/rust-tui`, not part of the released Python package.

It never reimplements backup logic. Read-only views come from
`~/.config/termux-sync/config.json`, local `manifest.json` files, and
`sync.log`. Backup and restore run through the Python event protocol; setup,
schedule, delete and clear-cache still hand the terminal to `termux-sync`.

## What it shows

- Hero header: gradient `TERMUX SYNC` mark, tagline, storage and backup-count
  chips, plus compression and max-backup settings on wide terminals.
- Stat cards: backups saved, total stored size, latest package count and
  schedule. Cards appear at 80+ columns and 20+ rows.
- Actions pane: icon for every action, plus backup, restore, delete, list,
  logs, disk check, setup, schedule, clear cache, refresh, help, quit.
- Backups pane: short backup name, age, size, relative usage bar and package
  count, with the selected backup's label, version and compression in the
  pane title.
- Output pane: scrollable log tail and captured command output, with dimmed
  dates and INFO / WARNING / ERROR chips.
- Operations cockpit: backup streams stages, progress and logs inside the TUI.
  Restore first shows a plan, package diff and type-to-confirm gate, then
  streams download, verification, extraction and package reinstall.
- Restore safety: checksums must pass before extraction. Cancel is allowed
  only during metadata, download and verification. After extraction starts,
  cancel is refused and the screen says so. Cancel asks once: Enter confirms,
  Esc dismisses.
- Package diff: `d` from the restore plan opens `TO INSTALL`,
  `ALREADY INSTALLED` and `DEVICE ONLY` sections with a persistent warning
  that every backup package is reinstalled.
- Motion: help text reveals at a fixed size in about 200ms. Captured commands
  show a WORKING card only when they run longer than 150ms, hold DONE for
  150ms, then reveal output. Pane changes and output updates are instant.
- `--no-anim` or `TERMUX_SYNC_TUI_REDUCED_MOTION=1` removes reveal motion while
  keeping status and color feedback.
- Startup draws immediately and loads backups in a worker; the header shows
  `LOADING` until the first snapshot arrives.
- Idle app does not redraw on a timer. Frames are produced on input, worker
  results, resize, and active animations only. Ctrl+C always quits.

## Build and run

```bash
cd tui
cargo build --release
./target/release/termux-sync-tui
```

Install it beside the Python command with:

```bash
cd tui
./install.sh
```

`install.sh` accepts `--prefix PATH` and `--skip-build`. It copies the release
binary into `$PREFIX/bin`.

Requires a terminal on stdout. Non-TTY runs exit with status 2 instead of
drawing garbage.

It also requires a `termux-sync` build with `--json-events`. If the installed
CLI is older, the TUI refuses to start an operation and tells you to update it.

Use another binary with:

```bash
TERMUX_SYNC_BIN=/path/to/termux-sync ./target/release/termux-sync-tui
```

`--help` and `--version` work without a terminal.

Disable motion with `--no-anim` or `TERMUX_SYNC_TUI_REDUCED_MOTION=1`.
Use ASCII icons with `--ascii` or `TERMUX_SYNC_TUI_ASCII=1`.

## Keys

| Key | Action |
|---|---|
| `↑` / `↓` or `k` / `j` | move selection |
| `Enter` | run action; restore selected backup from Backups pane |
| `d` | delete selected backup from Backups pane |
| `Tab` / `Shift-Tab` | switch pane |
| `R` | refresh backups and status |
| `L` | list backups (inline, including remote output) |
| `g` / `G` | top / bottom of output |
| `?` | help |
| `↑` / `↓` in help | scroll help at small sizes |
| `Ctrl+C` | quit immediately; refused during a protected restore |
| `q` | quit |

Operation keys:

| Key | Action |
|---|---|
| `Esc` / `c` | cancel while allowed; asks once, or explains why not |
| `d` | open package diff on the restore plan |
| `1` / `2` / `3` | jump diff sections |
| `/` | filter the current diff section |
| `r` | retry a failed operation |
| `↑` / `↓`, `PgUp` / `PgDn`, `G` | scroll logs or diff |
| type `RESTORE`, then `Enter` | confirm a restore |

Mouse: click an action to run it, click a backup row to select it, click a
pane to focus it, and scroll the wheel over lists and logs. Delete, clear-cache
and operation-cancel dialogs have clickable confirm/cancel areas. Mouse capture
disables terminal text selection; hold Shift to select text in most terminals.

## Layout

- `>= 80` columns and 20+ rows: hero, stat cards, actions, backups and output.
- `>= 80` columns below 20 rows: hero drops the stat cards.
- `48–79` columns: hero plus one pane at a time; `Tab` switches.
- `< 48×12`: "terminal too small" message naming the minimum.
- `NO_COLOR=1`: same layout, gradient, chips and focus markers without color.

## Safety notes

- The config parser keeps only display fields. Tokens and passwords in
  `config.json` are never read into the TUI state.
- Status, backup list and logs are read-only.
- Backup, restore, delete, setup, schedule and clear-cache run the real
  `termux-sync` command with the terminal restored, then return to the TUI.

## Runbook

Recovery steps for protected restore, cancel timeout, panics, verification
states, and old CLI installs: [RUNBOOK.md](RUNBOOK.md).

## Spike limits

- Remote storage (Google Drive, GitHub) shows no backup list; use the List
  action, which captures `termux-sync list` output.
- Backup labels cannot be typed in the TUI yet.
- Restore cannot be cancelled after extraction starts.
- Setup and schedule still hand over the terminal.
- Delete and clear-cache run captured after an in-TUI confirmation using
  `termux-sync --yes`.
- The operations protocol is version 1 and backup/restore only.
