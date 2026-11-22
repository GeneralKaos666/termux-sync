# termux-sync-tui (spike)

Ratatui dashboard around the existing `termux-sync` CLI. This is a throwaway
spike on branch `spike/rust-tui`, not part of the released Python package.

It never reimplements backup logic. Read-only views come from
`~/.config/termux-sync/config.json`, local `manifest.json` files, and
`sync.log`. Interactive commands hand the terminal to `termux-sync`.

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
- Motion: help expands from a small card, narrow layouts reveal the incoming
  pane, handoff commands show a spinner card before the terminal changes
  hands, and captured commands (`list`, `disk check`) show a WORKING card
  that holds a brief DONE state before revealing output.

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

## Spike limits

- Remote storage (Google Drive, GitHub) shows no backup list; use the List
  action, which captures `termux-sync list` output.
- Backup labels cannot be typed in the TUI yet.
- Interactive commands block the dashboard while they own the terminal, by
  design.
