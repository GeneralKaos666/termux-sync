use std::sync::mpsc::{self, Receiver, TryRecvError};

use ratatui::crossterm::event::KeyCode;
use ratatui::widgets::{ListState, TableState};

use crate::data::{self, Backup, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Actions,
    Backups,
    Output,
}

impl Focus {
    pub fn next(self) -> Self {
        match self {
            Focus::Actions => Focus::Backups,
            Focus::Backups => Focus::Output,
            Focus::Output => Focus::Actions,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Focus::Actions => Focus::Output,
            Focus::Backups => Focus::Actions,
            Focus::Output => Focus::Backups,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Focus::Actions => "Actions",
            Focus::Backups => "Backups",
            Focus::Output => "Output",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Backup,
    Restore,
    Delete,
    List,
    Logs,
    DiskCheck,
    Setup,
    Schedule,
    ClearCache,
    Refresh,
    Help,
    Quit,
}

pub struct ActionItem {
    pub label: &'static str,
    pub action: Action,
}

pub const ACTIONS: &[ActionItem] = &[
    ActionItem {
        label: "Backup now",
        action: Action::Backup,
    },
    ActionItem {
        label: "Restore selected",
        action: Action::Restore,
    },
    ActionItem {
        label: "Delete selected",
        action: Action::Delete,
    },
    ActionItem {
        label: "List backups",
        action: Action::List,
    },
    ActionItem {
        label: "Show logs",
        action: Action::Logs,
    },
    ActionItem {
        label: "Disk check (~)",
        action: Action::DiskCheck,
    },
    ActionItem {
        label: "Setup",
        action: Action::Setup,
    },
    ActionItem {
        label: "Schedule",
        action: Action::Schedule,
    },
    ActionItem {
        label: "Clear cache",
        action: Action::ClearCache,
    },
    ActionItem {
        label: "Refresh",
        action: Action::Refresh,
    },
    ActionItem {
        label: "Help",
        action: Action::Help,
    },
    ActionItem {
        label: "Quit",
        action: Action::Quit,
    },
];

pub enum Task {
    Refresh,
    Command(Vec<String>),
}

pub enum WorkerMsg {
    Refreshed(Snapshot),
    Output { title: String, text: String },
}

#[derive(Default)]
pub struct Worker {
    rx: Option<Receiver<Result<WorkerMsg, String>>>,
    busy: bool,
}

impl Worker {
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn start(&mut self, task: Task) -> bool {
        if self.busy {
            return false;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = match task {
                Task::Refresh => Ok(WorkerMsg::Refreshed(data::load_snapshot())),
                Task::Command(args) => data::run_captured(&args).map(|text| WorkerMsg::Output {
                    title: args.join(" "),
                    text,
                }),
            };
            let _ = tx.send(result);
        });
        self.rx = Some(rx);
        self.busy = true;
        true
    }

    pub fn poll(&mut self) -> Option<Result<WorkerMsg, String>> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(message) => {
                self.rx = None;
                self.busy = false;
                Some(message)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                self.busy = false;
                Some(Err("background task stopped unexpectedly".to_string()))
            }
        }
    }
}

pub struct App {
    pub snapshot: Snapshot,
    pub action_state: ListState,
    pub backup_state: TableState,
    pub focus: Focus,
    pub output: Vec<String>,
    pub output_offset: usize,
    pub status: String,
    pub show_help: bool,
    pub should_quit: bool,
    pub pending: Option<Vec<String>>,
    pub worker: Worker,
    pub tick: u64,
}

impl App {
    pub fn new() -> Self {
        Self::with_snapshot(data::load_snapshot())
    }

    pub fn with_snapshot(snapshot: Snapshot) -> Self {
        let mut action_state = ListState::default();
        action_state.select(Some(0));
        let mut backup_state = TableState::default();
        if !snapshot.backups.is_empty() {
            backup_state.select(Some(0));
        }
        let output = snapshot.log.clone();
        let status = snapshot
            .warning
            .clone()
            .unwrap_or_else(|| "ready".to_string());

        Self {
            snapshot,
            action_state,
            backup_state,
            focus: Focus::Actions,
            output,
            output_offset: 0,
            status,
            show_help: false,
            should_quit: false,
            pending: None,
            worker: Worker::default(),
            tick: 0,
        }
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn spinner(&self) -> Option<&'static str> {
        if !self.worker.is_busy() {
            return None;
        }
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        Some(FRAMES[(self.tick as usize / 2) % FRAMES.len()])
    }

    pub fn selected_backup(&self) -> Option<&Backup> {
        self.backup_state
            .selected()
            .and_then(|index| self.snapshot.backups.get(index))
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        if self.show_help {
            if matches!(code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                self.show_help = false;
            }
            return;
        }

        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.previous(),
            KeyCode::Char('R') => self.refresh(),
            _ => match self.focus {
                Focus::Actions => self.handle_actions_key(code),
                Focus::Backups => self.handle_backups_key(code),
                Focus::Output => self.handle_output_key(code),
            },
        }
    }

    fn handle_actions_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.move_action(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_action(1),
            KeyCode::Enter => self.activate_selected_action(),
            _ => {}
        }
    }

    fn handle_backups_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.move_backup(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_backup(1),
            KeyCode::Enter => self.run(Action::Restore),
            KeyCode::Char('d') => self.run(Action::Delete),
            _ => {}
        }
    }

    fn handle_output_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.output_offset += 1,
            KeyCode::Down | KeyCode::Char('j') => {
                self.output_offset = self.output_offset.saturating_sub(1);
            }
            KeyCode::PageUp => self.output_offset += 10,
            KeyCode::PageDown => self.output_offset = self.output_offset.saturating_sub(10),
            KeyCode::Char('g') | KeyCode::Home => self.output_offset = usize::MAX / 2,
            KeyCode::Char('G') | KeyCode::End => self.output_offset = 0,
            _ => {}
        }
    }

    fn move_action(&mut self, delta: isize) {
        let len = ACTIONS.len();
        if len == 0 {
            return;
        }
        let current = self.action_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(len as isize) as usize;
        self.action_state.select(Some(next));
    }

    fn move_backup(&mut self, delta: isize) {
        let len = self.snapshot.backups.len();
        if len == 0 {
            return;
        }
        let current = self.backup_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(len as isize) as usize;
        self.backup_state.select(Some(next));
    }

    fn activate_selected_action(&mut self) {
        let index = self.action_state.selected().unwrap_or(0);
        if let Some(item) = ACTIONS.get(index) {
            self.run(item.action);
        }
    }

    pub fn run(&mut self, action: Action) {
        match action {
            Action::Backup => self.queue_command(vec!["backup"]),
            Action::Restore => {
                if let Some(name) = self.selected_backup().map(|backup| backup.name.clone()) {
                    self.queue_command(vec!["restore", "--name", &name]);
                } else {
                    self.status = "no backup selected".to_string();
                }
            }
            Action::Delete => {
                if let Some(name) = self.selected_backup().map(|backup| backup.name.clone()) {
                    self.queue_command(vec!["delete", "--name", &name]);
                } else {
                    self.status = "no backup selected".to_string();
                }
            }
            Action::List => self.start_task(Task::Command(vec!["list".to_string()])),
            Action::Logs => {
                self.output = data::tail_log(200);
                self.output_offset = 0;
                self.focus = Focus::Output;
                self.status = "showing sync.log".to_string();
            }
            Action::DiskCheck => {
                self.start_task(Task::Command(vec!["check".to_string(), "~".to_string()]))
            }
            Action::Setup => self.queue_command(vec!["setup"]),
            Action::Schedule => self.queue_command(vec!["schedule"]),
            Action::ClearCache => self.queue_command(vec!["clear-cache"]),
            Action::Refresh => self.refresh(),
            Action::Help => self.show_help = true,
            Action::Quit => self.should_quit = true,
        }
    }

    fn queue_command(&mut self, args: Vec<&str>) {
        let argv: Vec<String> = args.into_iter().map(str::to_string).collect();
        self.status = format!("running termux-sync {}", argv.join(" "));
        self.pending = Some(argv);
    }

    fn start_task(&mut self, task: Task) {
        if self.worker.is_busy() {
            self.status = "background task already running".to_string();
            return;
        }
        let label = match &task {
            Task::Refresh => "refreshing".to_string(),
            Task::Command(args) => format!("running termux-sync {}", args.join(" ")),
        };
        if self.worker.start(task) {
            self.status = label;
        }
    }

    pub fn refresh(&mut self) {
        self.start_task(Task::Refresh);
    }

    pub fn poll_worker(&mut self) {
        let Some(result) = self.worker.poll() else {
            return;
        };
        match result {
            Ok(WorkerMsg::Refreshed(snapshot)) => self.apply_snapshot(snapshot),
            Ok(WorkerMsg::Output { title, text }) => {
                self.output.clear();
                self.output.push(format!("$ termux-sync {title}"));
                self.output.extend(text.lines().map(str::to_string));
                self.output_offset = 0;
                self.focus = Focus::Output;
                self.status = format!("termux-sync {title} finished");
            }
            Err(err) => {
                self.output.clear();
                self.output.extend(err.lines().map(str::to_string));
                self.output_offset = 0;
                self.focus = Focus::Output;
                self.status = "command failed".to_string();
            }
        }
    }

    pub fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let selected_name = self.selected_backup().map(|backup| backup.name.clone());

        self.status = snapshot
            .warning
            .clone()
            .unwrap_or_else(|| "refreshed".to_string());
        self.snapshot = snapshot;
        self.output = self.snapshot.log.clone();
        self.output_offset = 0;

        if self.snapshot.backups.is_empty() {
            self.backup_state.select(None);
        } else {
            let index = selected_name
                .and_then(|name| {
                    self.snapshot
                        .backups
                        .iter()
                        .position(|backup| backup.name == name)
                })
                .unwrap_or(0);
            self.backup_state.select(Some(index));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Config;

    fn snapshot() -> Snapshot {
        Snapshot {
            config: Config::default(),
            backups: vec![
                Backup {
                    name: "backup-one".to_string(),
                    date: "2026-01-01 12:00".to_string(),
                    label: String::new(),
                    packages: 2,
                    size: 1024,
                    version: "1.3.9".to_string(),
                    compression: "GZ".to_string(),
                },
                Backup {
                    name: "backup-two".to_string(),
                    date: "2026-01-02 12:00".to_string(),
                    label: "before-upgrade".to_string(),
                    packages: 3,
                    size: 2048,
                    version: "1.3.9".to_string(),
                    compression: "GZ".to_string(),
                },
            ],
            log: vec!["ready".to_string()],
            warning: None,
        }
    }

    #[test]
    fn navigation_wraps_both_directions() {
        let mut app = App::with_snapshot(snapshot());
        app.handle_key(KeyCode::Up);
        assert_eq!(app.action_state.selected(), Some(ACTIONS.len() - 1));
        app.handle_key(KeyCode::Down);
        assert_eq!(app.action_state.selected(), Some(0));
    }

    #[test]
    fn tab_cycles_focus() {
        let mut app = App::with_snapshot(snapshot());
        assert_eq!(app.focus, Focus::Actions);
        app.handle_key(KeyCode::Tab);
        assert_eq!(app.focus, Focus::Backups);
        app.handle_key(KeyCode::BackTab);
        assert_eq!(app.focus, Focus::Actions);
    }

    #[test]
    fn restore_uses_selected_backup_name() {
        let mut app = App::with_snapshot(snapshot());
        app.handle_key(KeyCode::Tab);
        app.handle_key(KeyCode::Down);
        app.handle_key(KeyCode::Enter);
        assert_eq!(
            app.pending,
            Some(vec![
                "restore".to_string(),
                "--name".to_string(),
                "backup-two".to_string()
            ])
        );
    }

    #[test]
    fn delete_without_backup_reports_status() {
        let mut snapshot = snapshot();
        snapshot.backups.clear();
        let mut app = App::with_snapshot(snapshot);
        app.run(Action::Delete);
        assert!(app.pending.is_none());
        assert_eq!(app.status, "no backup selected");
    }

    #[test]
    fn quit_sets_flag() {
        let mut app = App::with_snapshot(snapshot());
        app.handle_key(KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn refresh_keeps_selected_backup() {
        let mut app = App::with_snapshot(snapshot());
        app.backup_state.select(Some(1));
        let mut refreshed = snapshot();
        refreshed.backups.reverse();
        app.apply_snapshot(refreshed);
        assert_eq!(
            app.selected_backup().map(|backup| backup.name.as_str()),
            Some("backup-two")
        );
    }

    #[test]
    fn tick_advances_without_panicking() {
        let mut app = App::with_snapshot(snapshot());
        app.on_tick();
        app.on_tick();
        assert_eq!(app.tick, 2);
        assert!(app.spinner().is_none());
    }
}
