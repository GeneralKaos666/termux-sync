"""Read-only view of the on-disk config and backup manifests for the cockpit."""

import json
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class Snapshot:
    storage: str = "unconfigured"
    local_path: str = ""
    compression: str = "gz"
    max_backups: int = 5
    schedule: str = "off"
    backups: list[dict] = field(default_factory=list)
    warning: str | None = None


def _read_json(path: Path):
    with open(path) as handle:
        return json.load(handle)


def _schedule_text(config_dir: Path) -> str:
    path = config_dir / "cron.json"
    if not path.exists():
        return "off"
    try:
        schedule = _read_json(path)
    except (OSError, json.JSONDecodeError):
        return "unreadable"
    if not schedule.get("enabled"):
        return "off"
    return f"daily at {int(schedule.get('hour', 2)):02d}:{int(schedule.get('minute', 0)):02d}"


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
    return {
        "name": str(manifest.get("name") or directory.name),
        "date": str(manifest.get("date") or ""),
        "label": str(manifest.get("label") or ""),
        "size": size,
        "packages": len(packages),
        "storage": str(manifest.get("storage") or ""),
    }


def load_snapshot(config_dir: Path | None = None) -> Snapshot:
    if config_dir is None:
        config_dir = Path.home() / ".config" / "termux-sync"
    config_dir = Path(config_dir)
    snapshot = Snapshot(schedule=_schedule_text(config_dir))

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
    snapshot.max_backups = int(cfg.get("max_backups", 5) or 5)

    if snapshot.storage != "local" or not snapshot.local_path:
        return snapshot

    root = Path(snapshot.local_path).expanduser()
    if not root.is_dir():
        return snapshot

    skipped = 0
    for directory in sorted(root.iterdir(), reverse=True):
        if not directory.is_dir():
            continue
        manifest = _read_manifest(directory / "manifest.json")
        if manifest is None:
            skipped += 1
            continue
        snapshot.backups.append(_backup_entry(directory, manifest))

    if skipped:
        snapshot.warning = f"{skipped} backup folder(s) had no readable manifest."
    return snapshot


def status_text(snapshot: Snapshot) -> str:
    return (
        f"storage {snapshot.storage} · compression {snapshot.compression} · "
        f"keep {snapshot.max_backups} · auto-backup {snapshot.schedule}"
    )


def format_date(value: str, wide: bool) -> str:
    text = value[:19].replace("T", " ")
    if wide:
        return text
    return text[5:16] if len(text) >= 16 else text


def human_size(size: int) -> str:
    value = float(size)
    for unit in ("B", "KiB", "MiB", "GiB", "TiB"):
        if value < 1024 or unit == "TiB":
            return f"{value:.1f} {unit}" if unit != "B" else f"{int(value)} B"
        value /= 1024
    return f"{int(size)} B"
