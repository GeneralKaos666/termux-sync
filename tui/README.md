# termux-sync-tui (spike)

Ratatui dashboard around the existing `termux-sync` CLI. This is a throwaway
spike on branch `spike/rust-tui`, not part of the released Python package.

It never reimplements backup logic. Read-only views come from
`~/.config/termux-sync/config.json`, local `manifest.json` files, and
`sync.log`. Interactive commands hand the terminal to `termux-sync`.

## What it shows

- Header: storage and backup-count badges, and on wide terminals the local
  path, compression, max-backup count and schedule.
- Actions pane: backup, restore, delete, list, logs, disk check, setup,
  schedule, clear cache, refresh, help, quit.
- Backups pane: local backup name, label, date, size and package count, with
  the selected backup's label, version and compression in the pane title.
- Output pane: scrollable log tail and captured command output, with dates
  dimmed and INFO / WARNING / ERROR levels color-coded.

## Build and run

```bash
cd tui
cargo build --release
./target/release/termux-sync-tui
```

Requires a terminal on stdout. Non-TTY runs exit with status 2 instead of
drawing garbage.

Use another binary with:

```bash
TERMUX_SYNC_BIN=/path/to/termux-sync ./target/release/termux-sync-tui
```

`--help` and `--version` work without a terminal.

## Keys

| Key | Action |
|---|---|
| `↑` / `↓` or `k` / `j` | move selection |
| `Enter` | run action; restore selected backup from Backups pane |
| `d` | delete selected backup from Backups pane |
| `Tab` / `Shift-Tab` | switch pane |
| `R` | refresh backups and status |
| `g` / `G` | top / bottom of output |
| `?` | help |
| `q` | quit |

## Layout

- `>= 80` columns: actions, backups and output side by side.
- `48–79` columns: one pane at a time; `Tab` switches.
- `< 48×12`: "terminal too small" message naming the minimum.
- `NO_COLOR=1`: same layout, focus markers and badges without color.

## Safety notes

- The config parser keeps only display fields. Tokens and passwords in
  `config.json` are never read into the TUI state.
- Status, backup list and logs are read-only.
- Backup, restore, delete, setup, schedule and clear-cache run the real
  `termux-sync` command with the terminal restored, then return to the TUI.

## Spike limits

- Remote storage (Google Drive, GitHub) shows no backup list; use the List
  action, which captures `termux-sync list` output.
- Backup labels cannot be typed in the TUI yet.
- Interactive commands block the dashboard while they own the terminal, by
  design.
