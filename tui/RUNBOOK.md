# TUI Runbook

Operational recovery notes for `termux-sync-tui`.

## Event protocol

- The TUI runs `termux-sync backup|restore|list --json-events`.
- Events are JSON Lines on stdout; human Rich output goes to stderr.
- Protocol version is `1`. A newer major version is refused with an update message.
- No GitHub token or encryption password is emitted. Event strings pass through
  the redactor in `EventWriter`.

## Old CLI installs

At start of an operation the TUI probes the installed `termux-sync`:

- missing `--json-events`: backup and restore refuse before starting.
- missing `--yes`: delete and clear-cache fall back to the terminal handoff and
  the CLI asks its own confirmation.

Update `termux-sync` before using the cockpit if backup or restore refuses.

## Backup cancel

`Esc` / `c` on a running backup opens a confirmation card. Enter sends SIGINT
to the child process group; the Python cleanup path removes temporary files and
incomplete stream parts. The TUI records `cancelled`.

If the process does not exit within 10 seconds, the TUI sends SIGKILL and says
cleanup may be incomplete. Temporary files can remain under `$PREFIX/tmp` or
`$TMPDIR`; search for `termux_backup_*` and remove only paths you recognize.

## Restore cancel

Cancellation is allowed during metadata, download, verification, and while the
plan is open.

Once the first `extract:*` stage starts, cancel and quit are refused. Python
also ignores SIGINT during extraction. The footer shows `CANNOT CANCEL SAFELY`.
Wait for completion or failure.

If the TUI process dies during extraction:

- The terminal restore hook runs for normal exits and panics.
- The child may continue restoring; do not start a second restore.
- After restarting Termux, inspect the log at `~/.config/termux-sync/sync.log`
  and re-run the restore if necessary.

Closing the terminal externally is outside the guarantee.

## Verification states

- `VERIFIED`: local copy checksums matched, or GitHub stream digest matched.
- `CORRUPT`: local checksum mismatch. Backups are not deleted automatically.
- `SKIPPED`: the backend cannot prove integrity (for example Google Drive).
  Treat as unverified, not as failed.

Restore refuses to extract when any archive checksum fails.

## Layout recovery

Minimum layout is `48×12`. Below that the TUI shows the required dimensions.
At `< 80` columns it shows one pane; `Tab` switches focus.

If icons render as boxes or wide gaps, use `--ascii` or
`TERMUX_SYNC_TUI_ASCII=1`.

## Files

- Config: `~/.config/termux-sync/config.json`
- Log: `~/.config/termux-sync/sync.log`
- Schedule: `~/.config/termux-sync/cron.json`
- TUI history: `~/.config/termux-sync/tui-history.json`

Config stores the GitHub token in plaintext. The TUI never reads it into
state or events, but the file is readable by anything with your Termux user.

## Restore completion

The TUI never restarts Termux. Close and reopen Termux after a successful
restore so packages, shell files, and environment changes take effect.
