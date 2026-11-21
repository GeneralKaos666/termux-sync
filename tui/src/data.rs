use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Config {
    pub storage: String,
    pub local_path: String,
    pub compression: String,
    pub max_backups: u64,
    pub schedule: String,
}

#[derive(Debug, Clone)]
pub struct Backup {
    pub name: String,
    pub date: String,
    pub label: String,
    pub packages: usize,
    pub size: u64,
    pub version: String,
    pub compression: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogParts {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

pub fn parse_log_line(line: &str) -> Option<LogParts> {
    let rest = line.strip_prefix('[')?;
    let (timestamp, rest) = rest.split_once(']')?;
    let rest = rest.trim_start().strip_prefix('[')?;
    let (level, message) = rest.split_once(']')?;
    Some(LogParts {
        timestamp: timestamp.to_string(),
        level: level.to_string(),
        message: message.trim_start().to_string(),
    })
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub config: Config,
    pub backups: Vec<Backup>,
    pub log: Vec<String>,
    pub warning: Option<String>,
}

pub fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

pub fn config_dir() -> PathBuf {
    home().join(".config").join("termux-sync")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn schedule_path() -> PathBuf {
    config_dir().join("cron.json")
}

pub fn log_path() -> PathBuf {
    config_dir().join("sync.log")
}

pub fn load_snapshot() -> Snapshot {
    let mut warning = None;

    let config = match fs::read_to_string(config_path()) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => {
                let schedule = fs::read_to_string(schedule_path())
                    .ok()
                    .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                    .unwrap_or(Value::Null);
                parse_config(&value, &schedule)
            }
            Err(err) => {
                warning = Some(format!("config.json is not valid JSON: {err}"));
                Config::default()
            }
        },
        Err(_) => Config::default(),
    };

    let backups = if config.storage == "local" {
        match load_local_backups(Path::new(&expand_home(&config.local_path))) {
            Ok(list) => list,
            Err(err) => {
                warning = Some(err);
                Vec::new()
            }
        }
    } else {
        warning = Some(format!(
            "{} storage: press L to run `termux-sync list` in the terminal",
            config.storage.to_uppercase()
        ));
        Vec::new()
    };

    Snapshot {
        config,
        backups,
        log: tail_log(120),
        warning,
    }
}

pub fn parse_config(cfg: &Value, schedule: &Value) -> Config {
    let storage = string_field(cfg, "storage", "local").to_lowercase();
    let local_path = string_field(cfg, "local_path", "~/termux-backups");
    let compression = string_field(cfg, "compression", "gz");
    let max_backups = cfg.get("max_backups").and_then(Value::as_u64).unwrap_or(5);
    let enabled = schedule
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let hour = schedule.get("hour").and_then(Value::as_u64).unwrap_or(2);
    let minute = schedule.get("minute").and_then(Value::as_u64).unwrap_or(0);
    let schedule = if enabled {
        format!("daily {hour:02}:{minute:02}")
    } else {
        "off".to_string()
    };

    Config {
        storage,
        local_path,
        compression,
        max_backups,
        schedule,
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            storage: "local".to_string(),
            local_path: "~/termux-backups".to_string(),
            compression: "gz".to_string(),
            max_backups: 5,
            schedule: "off".to_string(),
        }
    }
}

pub fn load_local_backups(root: &Path) -> Result<Vec<Backup>, String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(err) => {
            return Err(format!("cannot read {}: {err}", root.display()));
        }
    };

    let mut backups = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest = dir.join("manifest.json");
        let Ok(text) = fs::read_to_string(&manifest) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(backup) = parse_manifest(&value) {
            backups.push(backup);
        }
    }

    backups.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(backups)
}

pub fn parse_manifest(value: &Value) -> Option<Backup> {
    let name = value.get("name")?.as_str()?.to_string();
    if name.is_empty() {
        return None;
    }
    let label = value
        .get("label")
        .and_then(Value::as_str)
        .filter(|label| !label.is_empty() && *label != name)
        .unwrap_or_default()
        .to_string();
    let date = value
        .get("date")
        .and_then(Value::as_str)
        .unwrap_or("—")
        .chars()
        .take(16)
        .collect::<String>()
        .replace('T', " ");
    let packages = value
        .get("packages")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let size = value
        .get("files")
        .and_then(Value::as_object)
        .map(|files| {
            files
                .values()
                .filter_map(|file| file.get("size").and_then(Value::as_u64))
                .sum()
        })
        .unwrap_or(0);
    Some(Backup {
        name,
        date,
        label,
        packages,
        size,
        version: string_field(value, "version", "—"),
        compression: string_field(value, "compression", "—").to_uppercase(),
    })
}

pub fn tail_log(lines: usize) -> Vec<String> {
    let Ok(text) = fs::read_to_string(log_path()) else {
        return Vec::new();
    };
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

pub fn run_captured(args: &[String]) -> Result<String, String> {
    let output = cli_command(args)
        .output()
        .map_err(|err| format!("cannot start termux-sync: {err}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let text = strip_ansi(&text).trim().to_string();

    if output.status.success() {
        Ok(text)
    } else if text.is_empty() {
        Err(format!("termux-sync exited with {}", output.status))
    } else {
        Err(text)
    }
}

pub fn cli_command(args: &[String]) -> Command {
    let bin = env::var("TERMUX_SYNC_BIN").unwrap_or_else(|_| "termux-sync".to_string());
    let mut command = Command::new(bin);
    command.args(args);
    command
}

pub fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        return home().join(rest).to_string_lossy().into_owned();
    }
    if path == "~" {
        return home().to_string_lossy().into_owned();
    }
    path.to_string()
}

pub fn human_size(bytes: u64) -> String {
    let mut value = bytes as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if value < 1024.0 {
            return format!("{value:.1} {unit}");
        }
        value /= 1024.0;
    }
    format!("{value:.1} TB")
}

fn string_field(value: &Value, key: &str, default: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Value {
        serde_json::json!({
            "name": "termux_backup_20260101_120000",
            "date": "2026-01-01T12:00:00.123456",
            "label": "before-upgrade",
            "packages": ["a", "b", "c"],
            "files": {
                "home.tar.gz": {"size": 2048},
                "usr_etc.tar.gz": {"size": 1024}
            },
            "version": "1.3.9",
            "compression": "gz"
        })
    }

    #[test]
    fn config_parser_keeps_only_display_fields() {
        let raw = serde_json::json!({
            "storage": "GITHUB",
            "local_path": "/tmp/backups",
            "compression": "xz",
            "max_backups": 3,
            "github_token": "secret-token",
            "encrypt_password": "secret-password"
        });
        let schedule = serde_json::json!({"enabled": true, "hour": 4, "minute": 5});
        let config = parse_config(&raw, &schedule);

        assert_eq!(config.storage, "github");
        assert_eq!(config.local_path, "/tmp/backups");
        assert_eq!(config.compression, "xz");
        assert_eq!(config.max_backups, 3);
        assert_eq!(config.schedule, "daily 04:05");
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret-token"));
        assert!(!debug.contains("secret-password"));
    }

    #[test]
    fn manifest_parser_reads_summary_fields() {
        let backup = parse_manifest(&manifest()).expect("manifest parses");

        assert_eq!(backup.name, "termux_backup_20260101_120000");
        assert_eq!(backup.date, "2026-01-01 12:00");
        assert_eq!(backup.label, "before-upgrade");
        assert_eq!(backup.packages, 3);
        assert_eq!(backup.size, 3072);
        assert_eq!(backup.version, "1.3.9");
        assert_eq!(backup.compression, "GZ");
    }

    #[test]
    fn log_line_parser_splits_timestamp_level_and_message() {
        let parts = parse_log_line("[2026-11-21 01:17:33] [INFO] Collected 1947 packages").unwrap();
        assert_eq!(parts.timestamp, "2026-11-21 01:17:33");
        assert_eq!(parts.level, "INFO");
        assert_eq!(parts.message, "Collected 1947 packages");

        assert!(parse_log_line("plain text").is_none());
        assert!(parse_log_line("[2026-11-21 01:17:33] no level").is_none());
    }

    #[test]
    fn manifest_without_name_is_skipped() {
        let value = serde_json::json!({"date": "2026-01-01"});
        assert!(parse_manifest(&value).is_none());
    }

    #[test]
    fn human_size_uses_binary_units() {
        assert_eq!(human_size(0), "0.0 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn ansi_sequences_are_removed() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m text"), "red text");
    }
}
