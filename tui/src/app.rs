use std::sync::mpsc::{self, Receiver, TryRecvError};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::{ListState, TableState};

use crate::data::{self, Backup, Snapshot};
use crate::ops::{Kind, Operation, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Operation,
}

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
    pub glyph: &'static str,
    pub ascii: &'static str,
    pub label: &'static str,
    pub action: Action,
}

pub const ACTIONS: &[ActionItem] = &[
    ActionItem {
        glyph: "↑",
        ascii: "+",
        label: "Backup now",
        action: Action::Backup,
    },
    ActionItem {
        glyph: "↺",
        ascii: "<",
        label: "Restore selected",
        action: Action::Restore,
    },
    ActionItem {
        glyph: "✕",
        ascii: "x",
        label: "Delete selected",
        action: Action::Delete,
    },
    ActionItem {
        glyph: "≡",
        ascii: "=",
        label: "List backups",
        action: Action::List,
    },
    ActionItem {
        glyph: "▤",
        ascii: "#",
        label: "Show logs",
        action: Action::Logs,
    },
    ActionItem {
        glyph: "◔",
        ascii: "%",
        label: "Disk check (~)",
        action: Action::DiskCheck,
    },
    ActionItem {
        glyph: "⚙",
        ascii: "*",
        label: "Setup",
        action: Action::Setup,
    },
    ActionItem {
        glyph: "◷",
        ascii: "@",
        label: "Schedule",
        action: Action::Schedule,
    },
    ActionItem {
        glyph: "◇",
        ascii: "-",
        label: "Clear cache",
        action: Action::ClearCache,
    },
    ActionItem {
        glyph: "⟳",
        ascii: "~",
        label: "Refresh",
        action: Action::Refresh,
    },
    ActionItem {
        glyph: "?",
        ascii: "?",
        label: "Help",
        action: Action::Help,
    },
    ActionItem {
        glyph: "↩",
        ascii: "q",
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

pub struct TaskOverlay {
    pub label: String,
    pub started_at: u64,
    pub done: bool,
    pub done_at: u64,
    pub visible: bool,
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
    pub help_opened_at: u64,
    pub task: Option<TaskOverlay>,
    pub loading: bool,
    pub help_scroll: usize,
    pub reduce_motion: bool,
    pub ascii: bool,
    pub screen: Screen,
    pub operation: Option<Operation>,
    pub op_result_handled: bool,
    pub pending_select: Option<String>,
    pub pending_confirm: Option<Action>,
    pub confirm_yes: Rect,
    pub confirm_no: Rect,
    pub actions_area: Rect,
    pub backups_area: Rect,
    pub output_area: Rect,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self::with_snapshot(Snapshot::default());
        app.loading = true;
        app.status = "loading backups…".to_string();
        app.refresh();
        app.status = "loading backups…".to_string();
        app
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
            help_opened_at: 0,
            task: None,
            loading: false,
            help_scroll: 0,
            reduce_motion: false,
            ascii: false,
            screen: Screen::Dashboard,
            operation: None,
            op_result_handled: false,
            pending_select: None,
            pending_confirm: None,
            confirm_yes: Rect::default(),
            confirm_no: Rect::default(),
            actions_area: Rect::default(),
            backups_area: Rect::default(),
            output_area: Rect::default(),
        }
    }

    pub fn open_help(&mut self) {
        self.show_help = true;
        self.help_opened_at = self.tick;
        self.help_scroll = 0;
    }

    pub fn help_progress(&self) -> f32 {
        if !self.show_help || self.reduce_motion {
            return 1.0;
        }
        progress(self.tick, self.help_opened_at, 4)
    }

    pub fn task_progress(&self) -> f32 {
        match &self.task {
            Some(task) if !self.reduce_motion => {
                progress(self.tick, task.started_at.saturating_add(3), 2)
            }
            _ => 1.0,
        }
    }

    pub fn needs_tick(&self) -> bool {
        self.worker.is_busy()
            || self.task.is_some()
            || self.help_progress() < 1.0
            || self
                .operation
                .as_ref()
                .is_some_and(|operation| operation.is_running())
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        let now = self.tick;
        let mut remove = false;

        if let Some(task) = &mut self.task {
            if !task.visible && now.saturating_sub(task.started_at) >= 3 {
                task.visible = true;
            }
            if task.done && now.saturating_sub(task.done_at) >= 3 {
                remove = true;
            }
        }
        if remove {
            self.task = None;
        }
    }

    pub fn spinner(&self) -> Option<&'static str> {
        if !self.worker.is_busy()
            && self.pending.is_none()
            && !self
                .operation
                .as_ref()
                .is_some_and(|operation| operation.is_running())
        {
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

    #[cfg(test)]
    pub fn handle_key(&mut self, code: KeyCode) {
        self.handle_key_with(code, KeyModifiers::NONE);
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        self.handle_key_with(key.code, key.modifiers);
    }

    fn handle_key_with(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        if let Some(action) = self.pending_confirm {
            match code {
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.pending_confirm = None;
                    self.execute_confirmed_action(action);
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {
                    self.pending_confirm = None;
                    self.status = "cancelled".to_string();
                }
                _ => {}
            }
            return;
        }
        if self.operation.is_some() {
            self.handle_operation_key(code, modifiers);
            return;
        }

        if modifiers.contains(KeyModifiers::CONTROL)
            && matches!(code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
            self.pending = None;
            self.should_quit = true;
            return;
        }

        if self.pending.is_some() {
            return;
        }
        if self.show_help {
            match code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                    self.show_help = false;
                    self.help_scroll = 0;
                }
                KeyCode::Down | KeyCode::Char('j') => self.help_scroll += 1,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                KeyCode::PageDown => self.help_scroll += 5,
                KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(5),
                _ => {}
            }
            return;
        }

        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Tab => self.switch_focus(self.focus.next()),
            KeyCode::BackTab => self.switch_focus(self.focus.previous()),
            KeyCode::Char('R') => self.refresh(),
            KeyCode::Char('L') => self.run(Action::List),
            _ => match self.focus {
                Focus::Actions => self.handle_actions_key(code),
                Focus::Backups => self.handle_backups_key(code),
                Focus::Output => self.handle_output_key(code),
            },
        }
    }

    fn switch_focus(&mut self, focus: Focus) {
        self.focus = focus;
    }

    fn handle_operation_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let ctrl_c = modifiers.contains(KeyModifiers::CONTROL)
            && matches!(code, KeyCode::Char('c') | KeyCode::Char('C'));
        let Some((status, is_done, diff_open)) = self
            .operation
            .as_ref()
            .map(|operation| (operation.status, operation.is_done(), operation.diff_open))
        else {
            return;
        };

        if ctrl_c {
            if is_done {
                self.should_quit = true;
            } else if !self
                .operation
                .as_mut()
                .is_some_and(Operation::request_cancel)
            {
                self.notice_operation("Restore in progress — cannot cancel safely");
            }
            return;
        }

        match status {
            Status::AwaitingConfirm => {
                if diff_open {
                    self.handle_diff_key(code, modifiers);
                } else {
                    self.handle_confirm_key(code, modifiers);
                }
            }
            Status::Running | Status::Cancelling => {
                if self
                    .operation
                    .as_ref()
                    .is_some_and(|operation| operation.cancel_prompt)
                {
                    match code {
                        KeyCode::Enter | KeyCode::Char('y') => {
                            self.operation.as_mut().map(Operation::request_cancel);
                            if let Some(operation) = &mut self.operation {
                                operation.cancel_prompt = false;
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {
                            if let Some(operation) = &mut self.operation {
                                operation.cancel_prompt = false;
                                operation.notice.clear();
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                match code {
                    KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('q')
                        if self
                            .operation
                            .as_ref()
                            .is_some_and(Operation::cancel_allowed) =>
                    {
                        if let Some(operation) = &mut self.operation {
                            operation.cancel_prompt = true;
                            operation.notice =
                                "Cancel operation and clean partial files? Enter yes · Esc no"
                                    .to_string();
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('q') => {
                        self.notice_operation("Restore in progress — cannot cancel safely");
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Some(operation) = &mut self.operation {
                            operation.log_offset = operation.log_offset.saturating_add(1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Some(operation) = &mut self.operation {
                            operation.log_offset = operation.log_offset.saturating_sub(1);
                        }
                    }
                    KeyCode::PageUp => {
                        if let Some(operation) = &mut self.operation {
                            operation.log_offset = operation.log_offset.saturating_add(10);
                        }
                    }
                    KeyCode::PageDown => {
                        if let Some(operation) = &mut self.operation {
                            operation.log_offset = operation.log_offset.saturating_sub(10);
                        }
                    }
                    KeyCode::Char('G') | KeyCode::End => {
                        if let Some(operation) = &mut self.operation {
                            operation.log_offset = 0;
                        }
                    }
                    KeyCode::Char('g') | KeyCode::Home => {
                        if let Some(operation) = &mut self.operation {
                            operation.log_offset = usize::MAX / 2;
                        }
                    }
                    _ => {}
                }
            }
            Status::Failed => match code {
                KeyCode::Char('r') => self.retry_operation(),
                KeyCode::Esc | KeyCode::Char('q') => self.back_to_dashboard(),
                _ => {}
            },
            Status::Succeeded | Status::Warnings | Status::Cancelled => match code {
                KeyCode::Esc | KeyCode::Char('q') => self.back_to_dashboard(),
                _ => {}
            },
        }
    }

    fn handle_confirm_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let plain = !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let Some(operation) = &mut self.operation else {
            return;
        };
        match code {
            KeyCode::Enter => {
                if operation.confirm_text.trim() == "RESTORE" {
                    if operation.confirm_restore() {
                        operation.confirm_text.clear();
                        operation.notice.clear();
                    }
                } else {
                    operation.notice = "Type RESTORE exactly to continue".to_string();
                }
            }
            KeyCode::Backspace => {
                operation.confirm_text.pop();
            }
            KeyCode::Esc => {
                operation.request_cancel();
            }
            KeyCode::Char('q') | KeyCode::Char('c') if plain => {
                operation.request_cancel();
            }
            KeyCode::Char('d') if plain => {
                operation.diff_open = true;
                operation.diff_scroll = 0;
            }
            KeyCode::Char(ch) if plain && !ch.is_control() && operation.confirm_text.len() < 32 => {
                operation.confirm_text.push(ch);
            }
            _ => {}
        }
    }

    fn handle_diff_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let plain = !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let Some(operation) = &mut self.operation else {
            return;
        };
        if operation.diff_filtering {
            match code {
                KeyCode::Esc | KeyCode::Enter => operation.diff_filtering = false,
                KeyCode::Backspace => {
                    operation.diff_filter.pop();
                }
                KeyCode::Char(ch) if plain && !ch.is_control() => operation.diff_filter.push(ch),
                _ => {}
            }
            operation.diff_scroll = 0;
            return;
        }
        match code {
            KeyCode::Esc => {
                operation.diff_open = false;
                operation.diff_filtering = false;
            }
            KeyCode::Char('q') | KeyCode::Char('c') if plain => {
                operation.request_cancel();
            }
            KeyCode::Char('d') if plain => {
                operation.diff_open = false;
                operation.diff_filtering = false;
            }
            KeyCode::Char('/') => {
                operation.diff_filtering = true;
                operation.diff_filter.clear();
                operation.diff_scroll = 0;
            }
            KeyCode::Char('1') => {
                operation.diff_section = 0;
                operation.diff_scroll = 0;
            }
            KeyCode::Char('2') => {
                operation.diff_section = 1;
                operation.diff_scroll = 0;
            }
            KeyCode::Char('3') => {
                operation.diff_section = 2;
                operation.diff_scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                operation.diff_scroll = operation.diff_scroll.saturating_add(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                operation.diff_scroll = operation.diff_scroll.saturating_sub(1)
            }
            KeyCode::PageUp => operation.diff_scroll = operation.diff_scroll.saturating_add(10),
            KeyCode::PageDown => operation.diff_scroll = operation.diff_scroll.saturating_sub(10),
            KeyCode::Char('G') | KeyCode::End => operation.diff_scroll = 0,
            KeyCode::Char('g') | KeyCode::Home => operation.diff_scroll = usize::MAX / 2,
            _ => {}
        }
    }

    fn notice_operation(&mut self, message: &str) {
        if let Some(operation) = &mut self.operation {
            operation.notice = message.to_string();
        }
    }

    fn start_operation(&mut self, kind: Kind, name: &str, args: Vec<String>) {
        if !crate::ops::supports_events() {
            self.status =
                "installed termux-sync is too old for the TUI (missing --json-events). Update it."
                    .to_string();
            return;
        }
        match Operation::start(kind, name, args, self.reduce_motion) {
            Ok(operation) => {
                self.operation = Some(operation);
                self.screen = Screen::Operation;
                self.op_result_handled = false;
                self.status = match kind {
                    Kind::Backup => "backup running".to_string(),
                    Kind::Restore => "restore plan".to_string(),
                };
            }
            Err(error) => {
                self.status = format!("cannot start termux-sync: {error}");
            }
        }
    }

    fn retry_operation(&mut self) {
        let retry = self
            .operation
            .as_ref()
            .map(|operation| (operation.kind, operation.retry_args()));
        if let Some((kind, args)) = retry {
            let name = self
                .operation
                .as_ref()
                .map(|operation| operation.name.clone())
                .unwrap_or_default();
            self.start_operation(kind, &name, args);
        }
    }

    fn back_to_dashboard(&mut self) {
        self.operation = None;
        self.screen = Screen::Dashboard;
        self.op_result_handled = false;
    }

    pub fn poll_operation(&mut self) -> bool {
        let Some(operation) = &mut self.operation else {
            return false;
        };
        let changed = operation.poll();
        let finished = operation.is_done() && !self.op_result_handled;
        let success_name =
            if finished && matches!(operation.status, Status::Succeeded | Status::Warnings) {
                Some(if operation.name.is_empty() {
                    operation
                        .result
                        .as_ref()
                        .map(|result| result.name.clone())
                        .unwrap_or_default()
                } else {
                    operation.name.clone()
                })
            } else {
                None
            };

        if finished {
            self.op_result_handled = true;
        }
        if let Some(name) = success_name {
            if !name.is_empty() {
                self.pending_select = Some(name);
            }
            self.refresh();
        }
        changed
    }

    pub fn handle_scroll(&mut self, column: u16, row: u16, up: bool) {
        if self.pending_confirm.is_some() {
            return;
        }
        if let Some(operation) = &mut self.operation {
            if operation.status == Status::AwaitingConfirm && operation.diff_open {
                operation.diff_scroll = if up {
                    operation.diff_scroll.saturating_add(3)
                } else {
                    operation.diff_scroll.saturating_sub(3)
                };
            } else if operation.is_running() {
                operation.log_offset = if up {
                    operation.log_offset.saturating_add(3)
                } else {
                    operation.log_offset.saturating_sub(3)
                };
            }
            return;
        }
        if self.pending.is_some() || self.task.is_some() || self.show_help {
            return;
        }
        let point = Position::new(column, row);
        if self.actions_area.contains(point) {
            self.move_action(if up { -1 } else { 1 });
            self.switch_focus(Focus::Actions);
        } else if self.backups_area.contains(point) {
            self.move_backup(if up { -1 } else { 1 });
            self.switch_focus(Focus::Backups);
        } else if self.output_area.contains(point) {
            self.output_offset = if up {
                self.output_offset.saturating_add(3)
            } else {
                self.output_offset.saturating_sub(3)
            };
            self.switch_focus(Focus::Output);
        }
    }

    pub fn handle_mouse(&mut self, column: u16, row: u16) {
        let point = Position::new(column, row);
        if let Some(action) = self.pending_confirm {
            if self.confirm_yes.contains(point) {
                self.pending_confirm = None;
                self.execute_confirmed_action(action);
            } else if self.confirm_no.contains(point) {
                self.pending_confirm = None;
                self.status = "cancelled".to_string();
            }
            return;
        }
        if self.operation.is_some() {
            return;
        }
        if self.pending.is_some() || self.task.is_some() || self.show_help {
            return;
        }

        if self.actions_area.contains(point) {
            let first_row = self.actions_area.y.saturating_add(1);
            let last_row = self
                .actions_area
                .y
                .saturating_add(self.actions_area.height)
                .saturating_sub(1);
            if row >= first_row && row < last_row {
                let index = (row - first_row) as usize + self.action_state.offset();
                if index < ACTIONS.len() {
                    self.action_state.select(Some(index));
                    self.switch_focus(Focus::Actions);
                    self.run(ACTIONS[index].action);
                }
            } else {
                self.switch_focus(Focus::Actions);
            }
            return;
        }

        if self.backups_area.contains(point) {
            self.switch_focus(Focus::Backups);
            let first_data = self.backups_area.y.saturating_add(2);
            if row >= first_data {
                let index = (row - first_data) as usize + self.backup_state.offset();
                if index < self.snapshot.backups.len() {
                    self.backup_state.select(Some(index));
                }
            }
            return;
        }

        if self.output_area.contains(point) {
            self.switch_focus(Focus::Output);
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
            Action::Backup => {
                self.start_operation(Kind::Backup, "backup", vec!["backup".to_string()])
            }
            Action::Restore => {
                if let Some(name) = self.selected_backup().map(|backup| backup.name.clone()) {
                    let args = restore_command(&name);
                    self.start_operation(Kind::Restore, &name, args);
                } else {
                    self.status = "no backup selected".to_string();
                }
            }
            Action::Delete => {
                if self.selected_backup().is_some() {
                    self.pending_confirm = Some(Action::Delete);
                } else {
                    self.status = "no backup selected".to_string();
                }
            }
            Action::List => self.start_task(Task::Command(vec!["list".to_string()])),
            Action::Logs => {
                self.output = data::tail_log(200);
                self.output_offset = 0;
                self.switch_focus(Focus::Output);
                self.status = "showing sync.log".to_string();
            }
            Action::DiskCheck => {
                self.start_task(Task::Command(vec!["check".to_string(), "~".to_string()]))
            }
            Action::Setup => self.queue_command(vec!["setup"]),
            Action::Schedule => self.queue_command(vec!["schedule"]),
            Action::ClearCache => self.pending_confirm = Some(Action::ClearCache),
            Action::Refresh => self.refresh(),
            Action::Help => self.open_help(),
            Action::Quit => self.should_quit = true,
        }
    }

    fn execute_confirmed_action(&mut self, action: Action) {
        match action {
            Action::Delete => {
                if let Some(name) = self.selected_backup().map(|backup| backup.name.clone()) {
                    self.start_task(Task::Command(vec![
                        "delete".to_string(),
                        "--name".to_string(),
                        name,
                        "--yes".to_string(),
                    ]));
                }
            }
            Action::ClearCache => {
                self.start_task(Task::Command(vec![
                    "clear-cache".to_string(),
                    "--yes".to_string(),
                ]));
            }
            _ => {}
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
            Task::Command(args) => format!("termux-sync {}", args.join(" ")),
        };
        let show_overlay = matches!(&task, Task::Command(_));
        if self.worker.start(task) {
            self.status = label.clone();
            if show_overlay {
                self.task = Some(TaskOverlay {
                    label,
                    started_at: self.tick,
                    done: false,
                    done_at: 0,
                    visible: false,
                });
            }
        }
    }

    pub fn refresh(&mut self) {
        self.start_task(Task::Refresh);
    }

    pub fn poll_worker(&mut self) -> bool {
        let Some(result) = self.worker.poll() else {
            return false;
        };
        let hide_fast_task = self.task.as_ref().is_some_and(|task| !task.visible);
        if hide_fast_task {
            self.task = None;
        } else if let Some(task) = &mut self.task {
            task.done = true;
            task.done_at = self.tick;
        }
        match result {
            Ok(WorkerMsg::Refreshed(snapshot)) => self.apply_snapshot(snapshot),
            Ok(WorkerMsg::Output { title, text }) => {
                self.output.clear();
                self.output.push(format!("$ termux-sync {title}"));
                self.output.extend(text.lines().map(str::to_string));
                self.output_offset = 0;
                self.switch_focus(Focus::Output);
                self.status = format!("termux-sync {title} finished");
            }
            Err(err) => {
                self.output.clear();
                self.output.extend(err.lines().map(str::to_string));
                self.output_offset = 0;
                self.switch_focus(Focus::Output);
                self.status = "command failed".to_string();
            }
        }
        true
    }

    pub fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let selected_name = self
            .pending_select
            .take()
            .or_else(|| self.selected_backup().map(|backup| backup.name.clone()));

        self.loading = false;
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

pub fn restore_command(name: &str) -> Vec<String> {
    vec![
        "restore".to_string(),
        "--name".to_string(),
        name.to_string(),
    ]
}

fn progress(tick: u64, start: u64, ticks: u64) -> f32 {
    if ticks == 0 {
        1.0
    } else {
        (tick.saturating_sub(start) as f32 / ticks as f32).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Config;
    use crate::ops::{Kind, Status, test_operation, test_plan};

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
                    modified: None,
                },
                Backup {
                    name: "backup-two".to_string(),
                    date: "2026-01-02 12:00".to_string(),
                    label: "before-upgrade".to_string(),
                    packages: 3,
                    size: 2048,
                    version: "1.3.9".to_string(),
                    compression: "GZ".to_string(),
                    modified: None,
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
    fn restore_command_uses_backup_name() {
        assert_eq!(
            restore_command("backup-two"),
            vec![
                "restore".to_string(),
                "--name".to_string(),
                "backup-two".to_string()
            ]
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

    #[test]
    fn handoff_is_queued_without_waiting() {
        let mut app = App::with_snapshot(snapshot());
        app.run(Action::Setup);
        assert_eq!(app.pending, Some(vec!["setup".to_string()]));
    }

    #[test]
    fn scroll_moves_the_action_selection() {
        let mut app = App::with_snapshot(snapshot());
        app.actions_area = Rect::new(0, 0, 26, 20);
        app.handle_scroll(2, 5, true);
        assert_eq!(app.action_state.selected(), Some(ACTIONS.len() - 1));
        app.handle_scroll(2, 5, false);
        assert_eq!(app.action_state.selected(), Some(0));
    }

    #[test]
    fn delete_and_clear_cache_confirm_inside_the_tui() {
        let mut app = App::with_snapshot(snapshot());
        app.run(Action::Delete);
        assert_eq!(app.pending_confirm, Some(Action::Delete));
        app.pending_confirm = None;

        app.run(Action::ClearCache);
        assert_eq!(app.pending_confirm, Some(Action::ClearCache));
    }

    #[test]
    fn clicking_an_action_runs_it() {
        let mut app = App::with_snapshot(snapshot());
        app.actions_area = Rect::new(0, 0, 26, 20);
        app.handle_mouse(2, 11);
        assert!(app.show_help);
    }

    #[test]
    fn clicking_a_backup_row_selects_it() {
        let mut app = App::with_snapshot(snapshot());
        app.backups_area = Rect::new(26, 0, 50, 10);
        app.handle_mouse(30, 3);
        assert_eq!(app.focus, Focus::Backups);
        assert_eq!(app.backup_state.selected(), Some(1));
    }

    #[test]
    fn task_card_waits_before_showing() {
        let mut app = App::with_snapshot(snapshot());
        app.task = Some(TaskOverlay {
            label: "termux-sync list".to_string(),
            started_at: 0,
            done: false,
            done_at: 0,
            visible: false,
        });
        app.on_tick();
        app.on_tick();
        assert!(!app.task.as_ref().unwrap().visible);
        app.on_tick();
        assert!(app.task.as_ref().unwrap().visible);
    }

    #[test]
    fn task_overlay_holds_completion_before_dismissing() {
        let mut app = App::with_snapshot(snapshot());
        app.task = Some(TaskOverlay {
            label: "termux-sync list".to_string(),
            started_at: 0,
            done: false,
            done_at: 0,
            visible: true,
        });
        for _ in 0..5 {
            app.on_tick();
        }
        assert_eq!(app.task_progress(), 1.0);

        app.task.as_mut().unwrap().done = true;
        app.task.as_mut().unwrap().done_at = app.tick;
        app.on_tick();
        app.on_tick();
        assert!(app.task.is_some());
        app.on_tick();
        assert!(app.task.is_none());
    }

    #[test]
    fn ctrl_c_quits_even_with_pending_command() {
        let mut app = App::with_snapshot(snapshot());
        app.run(Action::Setup);
        assert!(app.pending.is_some());
        app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
        assert!(app.pending.is_none());
    }

    #[test]
    fn help_scroll_moves_without_leaving_bounds() {
        let mut app = App::with_snapshot(snapshot());
        app.open_help();
        app.handle_key(KeyCode::Down);
        assert_eq!(app.help_scroll, 1);
        app.handle_key(KeyCode::PageDown);
        assert_eq!(app.help_scroll, 6);
        app.handle_key(KeyCode::PageUp);
        app.handle_key(KeyCode::PageUp);
        assert_eq!(app.help_scroll, 0);
    }

    #[test]
    fn idle_app_stops_requesting_ticks() {
        let mut app = App::with_snapshot(snapshot());
        for _ in 0..10 {
            if app.needs_tick() {
                app.on_tick();
            }
        }
        assert!(!app.needs_tick());
    }

    #[test]
    fn running_operation_asks_before_cancelling() {
        let mut app = App::with_snapshot(snapshot());
        app.operation = Some(test_operation(Kind::Backup));
        app.screen = Screen::Operation;

        app.handle_key(KeyCode::Esc);
        assert!(app.operation.as_ref().unwrap().cancel_prompt);
        assert_eq!(app.operation.as_ref().unwrap().status, Status::Running);

        app.handle_key(KeyCode::Enter);
        assert_eq!(app.operation.as_ref().unwrap().status, Status::Cancelling);
    }

    #[test]
    fn restore_plan_requires_typed_confirmation() {
        let mut app = App::with_snapshot(snapshot());
        let mut operation = test_operation(Kind::Restore);
        operation.status = Status::AwaitingConfirm;
        operation.plan = Some(test_plan());
        app.operation = Some(operation);
        app.screen = Screen::Operation;

        for ch in "RESTORE".chars() {
            app.handle_key(KeyCode::Char(ch));
        }
        let operation = app.operation.as_ref().unwrap();
        assert_eq!(operation.confirm_text, "RESTORE");

        app.handle_key(KeyCode::Enter);
        let operation = app.operation.as_ref().unwrap();
        assert!(operation.notice.contains("cannot write confirmation"));
    }

    #[test]
    fn protected_restore_refuses_cancel() {
        let mut app = App::with_snapshot(snapshot());
        let mut operation = test_operation(Kind::Restore);
        operation.protected = true;
        app.operation = Some(operation);
        app.screen = Screen::Operation;

        app.handle_key(KeyCode::Esc);
        let operation = app.operation.as_ref().unwrap();
        assert_eq!(operation.status, Status::Running);
        assert!(operation.notice.contains("cannot cancel"));
    }

    #[test]
    fn package_diff_filter_narrows_the_list() {
        let mut app = App::with_snapshot(snapshot());
        let mut operation = test_operation(Kind::Restore);
        operation.status = Status::AwaitingConfirm;
        operation.plan = Some(test_plan());
        operation
            .diff
            .apply("to_install", 1, 1, vec!["git".into(), "python".into()]);
        operation.diff.apply("already_installed", 1, 1, Vec::new());
        operation.diff.apply("device_only", 1, 1, Vec::new());
        app.operation = Some(operation);
        app.screen = Screen::Operation;

        app.handle_key(KeyCode::Char('d'));
        app.handle_key(KeyCode::Char('/'));
        for ch in "git".chars() {
            app.handle_key(KeyCode::Char(ch));
        }
        let operation = app.operation.as_ref().unwrap();
        assert!(operation.diff_open);
        assert!(operation.diff_filtering);
        assert_eq!(operation.filtered_diff(), vec!["git".to_string()]);
    }

    #[test]
    fn help_animation_completes_in_four_ticks() {
        let mut app = App::with_snapshot(snapshot());
        app.open_help();
        assert_eq!(app.help_progress(), 0.0);
        for _ in 0..4 {
            app.on_tick();
        }
        assert_eq!(app.help_progress(), 1.0);
    }

    #[test]
    fn reduced_motion_skips_help_and_task_motion() {
        let mut app = App::with_snapshot(snapshot());
        app.reduce_motion = true;
        app.open_help();
        assert_eq!(app.help_progress(), 1.0);
        app.task = Some(TaskOverlay {
            label: "termux-sync list".to_string(),
            started_at: 0,
            done: false,
            done_at: 0,
            visible: true,
        });
        assert_eq!(app.task_progress(), 1.0);
    }
}
