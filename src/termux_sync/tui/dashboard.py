"""Read-only view of the on-disk config, backup manifests, and log for the cockpit."""

import json
import subprocess
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

from rich.style import Style
from rich.text import Text

ACCENT = "#46dcff"
SUCCESS = "#5fd75f"
WARN = "#ffd75f"
DANGER = "#ff5f5f"
MAGENTA = "#d278ff"
MUTED = "#8a8a8a"
GRADIENT_FROM = (70, 220, 255)
GRADIENT_TO = (210, 120, 255)

LOG_LINES = 120
SIZE_BAR_WIDTH = 6


@dataclass
class Snapshot:
    storage: str = "unconfigured"
    local_path: str = ""
    compression: str = "gz"
    max_backups: int = 5
    schedule: str = "off"
    backups: list[dict] = field(default_factory=list)
    log: list[str] = field(default_factory=list)
    warning: str | None = None


def _read_json(path: Path):
    with open(path) as handle:
        return json.load(handle)


def _as_int(value, default: int) -> int:
    """Config files are hand-editable; a bad value must not stop the cockpit."""
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _schedule_text(config_dir: Path) -> str:
    path = config_dir / "cron.json"
    if not path.exists():
        return "off"
    try:
        schedule = _read_json(path)
    except (OSError, json.JSONDecodeError):
        return "unreadable"
    if not isinstance(schedule, dict) or not schedule.get("enabled"):
        return "off"
    hour = _as_int(schedule.get("hour", 2), 2)
    minute = _as_int(schedule.get("minute", 0), 0)
    return f"daily {hour:02d}:{minute:02d}"


def _read_manifest(path: Path) -> dict | None:
    try:
        manifest = _read_json(path)
    except (OSError, json.JSONDecodeError):
        return None
    if not isinstance(manifest, dict):
        return None
    return manifest


def _backup_entry(directory: Path, manifest: dict) -> dict:
    files = manifest.get("files") or {}
    size = sum(int(info.get("size", 0) or 0) for info in files.values() if isinstance(info, dict))
    packages = manifest.get("packages") or []
    try:
        modified = (directory / "manifest.json").stat().st_mtime
    except OSError:
        modified = None
    date = str(manifest.get("date") or "")[:16].replace("T", " ")
    return {
        "name": str(manifest.get("name") or directory.name),
        "date": date or "—",
        "label": str(manifest.get("label") or ""),
        "size": size,
        "packages": len(packages),
        "version": str(manifest.get("version") or "—"),
        "compression": str(manifest.get("compression") or "—").upper(),
        "modified": modified,
    }


def load_snapshot(config_dir: Path | None = None) -> Snapshot:
    if config_dir is None:
        config_dir = Path.home() / ".config" / "termux-sync"
    config_dir = Path(config_dir)
    snapshot = Snapshot(schedule=_schedule_text(config_dir), log=tail_log(config_dir))

    config_path = config_dir / "config.json"
    if not config_path.exists():
        snapshot.warning = "No configuration yet — press s to run setup."
        return snapshot

    try:
        cfg = _read_json(config_path)
        if not isinstance(cfg, dict):
            raise ValueError("config is not an object")
    except (OSError, json.JSONDecodeError, ValueError):
        snapshot.warning = "config.json could not be read."
        return snapshot

    snapshot.storage = str(cfg.get("storage", "local"))
    snapshot.local_path = str(cfg.get("local_path", ""))
    snapshot.compression = str(cfg.get("compression", "gz"))
    snapshot.max_backups = _as_int(cfg.get("max_backups", 5), 5)

    if snapshot.storage != "local":
        snapshot.warning = (
            f"{snapshot.storage.upper()} storage: press l to run "
            "`termux-sync list` in the terminal"
        )
        return snapshot

    if not snapshot.local_path:
        return snapshot

    root = Path(snapshot.local_path).expanduser()
    if not root.is_dir():
        return snapshot

    skipped = 0
    try:
        directories = sorted(root.iterdir(), reverse=True)
    except OSError as exc:
        snapshot.warning = f"Backup folder could not be read: {exc.strerror or exc}"
        return snapshot

    for directory in directories:
        if not directory.is_dir():
            continue
        manifest = _read_manifest(directory / "manifest.json")
        if manifest is None:
            skipped += 1
            continue
        snapshot.backups.append(_backup_entry(directory, manifest))

    snapshot.backups.sort(key=lambda backup: backup["date"], reverse=True)
    if skipped:
        snapshot.warning = f"{skipped} backup folder(s) had no readable manifest."
    return snapshot


def tail_log(config_dir: Path, lines: int = LOG_LINES) -> list[str]:
    try:
        text = (config_dir / "sync.log").read_text(errors="replace")
    except OSError:
        return []
    return text.splitlines()[-lines:]


def parse_log_line(line: str) -> tuple[str, str, str] | None:
    """Split ``[timestamp] [LEVEL] message`` into its three parts."""
    rest = line[1:] if line.startswith("[") else ""
    if "]" not in rest:
        return None
    timestamp, rest = rest.split("]", 1)
    rest = rest.lstrip()
    if not rest.startswith("["):
        return None
    rest = rest[1:]
    if "]" not in rest:
        return None
    level, message = rest.split("]", 1)
    return timestamp, level, message.lstrip()


def render_log_line(line: str) -> Text:
    parts = parse_log_line(line)
    if parts is None:
        style = Style(color=ACCENT, bold=True) if line.startswith("$ ") else None
        return Text(line, style=style)

    timestamp, level, message = parts
    colors = {
        "INFO": SUCCESS,
        "WARN": WARN,
        "WARNING": WARN,
        "ERROR": DANGER,
        "FAIL": DANGER,
        "DEBUG": MUTED,
    }
    label = {"WARNING": "WARN", "FAIL": "ERROR"}.get(level, level)
    text = Text()
    text.append(timestamp, style=Style(color=MUTED))
    text.append(" ")
    text.append(f" {label} ", style=Style(color="black", bgcolor=colors.get(level, MUTED), bold=True))
    text.append(" ")
    text.append(message)
    return text


def short_name(name: str) -> str:
    prefix = "termux_backup_"
    return name[len(prefix):] if name.startswith(prefix) else name


def size_bar(size: int, maximum: int, width: int = SIZE_BAR_WIDTH) -> str:
    if maximum <= 0:
        return " " * width
    filled = round(size / maximum * width)
    filled = max(0, min(width, filled))
    return "▰" * filled + "▱" * (width - filled)


def _format_age(seconds: int) -> str:
    if seconds < 60:
        return "now"
    if seconds < 3600:
        return f"{seconds // 60}m"
    if seconds < 86_400:
        return f"{seconds // 3600}h"
    if seconds < 14 * 86_400:
        return f"{seconds // 86_400}d"
    if seconds < 60 * 86_400:
        return f"{seconds // (7 * 86_400)}w"
    if seconds < 365 * 86_400:
        return f"{seconds // (30 * 86_400)}mo"
    return f"{seconds // (365 * 86_400)}y"


def backup_age(backup: dict, now: float | None = None) -> str:
    now = time.time() if now is None else now
    modified = backup.get("modified")
    if modified:
        return _format_age(max(0, int(now - float(modified))))
    try:
        then = datetime.strptime(str(backup.get("date", "")), "%Y-%m-%d %H:%M")
    except ValueError:
        return "—"
    return _format_age(max(0, int(now - then.timestamp())))


def human_size(size: int) -> str:
    value = float(size)
    for unit in ("B", "KB", "MB", "GB"):
        if value < 1024:
            return f"{value:.1f} {unit}"
        value /= 1024
    return f"{value:.1f} TB"


def _lerp_color(start: tuple[int, int, int], end: tuple[int, int, int], t: float) -> str:
    mix = [round(a + (b - a) * t) for a, b in zip(start, end)]
    return "#%02x%02x%02x" % tuple(mix)


def gradient_text(text: str) -> Text:
    """Colour each character along the hero gradient, like the Rust cockpit."""
    chars = list(text)
    last = max(1, len(chars) - 1)
    rendered = Text()
    for index, char in enumerate(chars):
        style = Style(color=_lerp_color(GRADIENT_FROM, GRADIENT_TO, index / last), bold=True)
        rendered.append(char, style=style)
    return rendered


def status_text(snapshot: Snapshot) -> str:
    if snapshot.warning:
        return snapshot.warning
    count = len(snapshot.backups)
    if count == 1:
        return "Loaded 1 backup"
    return f"Loaded {count} backups"


def run_cli(cli_prefix: list[str], argv: list[str]) -> None:
    """Hand the terminal to the existing CLI while the app is suspended."""
    subprocess.run(cli_prefix + list(argv))
