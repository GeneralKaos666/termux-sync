use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Cell, Clear, List, ListItem, Padding, Paragraph, Row, Table,
};

use crate::app::{ACTIONS, App, Focus};
use crate::data::{human_size, parse_log_line};

pub const MIN_WIDTH: u16 = 48;
pub const MIN_HEIGHT: u16 = 12;
const SPLIT_WIDTH: u16 = 80;

const ACCENT: Color = Color::Cyan;
const SUCCESS: Color = Color::Green;
const WARN: Color = Color::Yellow;
const DANGER: Color = Color::Red;
const MUTED: Color = Color::DarkGray;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutPlan {
    TooSmall,
    Single,
    Split,
}

pub fn plan(area: Rect) -> LayoutPlan {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        LayoutPlan::TooSmall
    } else if area.width < SPLIT_WIDTH {
        LayoutPlan::Single
    } else {
        LayoutPlan::Split
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    if plan(area) == LayoutPlan::TooSmall {
        render_too_small(frame, area);
        return;
    }

    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    render_header(frame, header, app);

    match plan(area) {
        LayoutPlan::Split => render_split(frame, body, app),
        _ => render_single(frame, body, app),
    }

    render_footer(frame, footer, app);

    if app.show_help {
        render_help(frame, area);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let config = &app.snapshot.config;
    let count = app.snapshot.backups.len();
    let count_label = if count == 1 {
        "1 BACKUP".to_string()
    } else {
        format!("{count} BACKUPS")
    };
    let count_color = if count == 0 { MUTED } else { SUCCESS };

    let mut line = vec![
        Span::styled("◆ ", fg(ACCENT).bold()),
        Span::styled("termux-sync", fg(ACCENT).bold()),
        Span::styled("  tui", fg(MUTED)),
        Span::raw("   "),
        badge(&config.storage.to_uppercase(), ACCENT),
        Span::raw(" "),
        badge(&count_label, count_color),
    ];

    if area.width >= 100 {
        line.push(Span::styled("   ", fg(MUTED)));
        line.push(Span::styled(&config.local_path, fg(MUTED)));
        line.push(Span::styled("  ·  ", fg(MUTED)));
        line.push(Span::styled(
            format!(
                "{} · max {} · auto {}",
                config.compression.to_uppercase(),
                config.max_backups,
                config.schedule
            ),
            fg(MUTED),
        ));
    }

    if app.snapshot.warning.is_some() {
        line.push(Span::styled("   [!]", fg(WARN).bold()));
    }

    frame.render_widget(Paragraph::new(Line::from(line)), area);
}

fn render_split(frame: &mut Frame, area: Rect, app: &mut App) {
    let [left, right] =
        Layout::horizontal([Constraint::Length(26), Constraint::Min(0)]).areas(area);
    let [backups, output] =
        Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(right);

    render_actions(frame, left, app);
    render_backups(frame, backups, app);
    render_output(frame, output, app);
}

fn render_single(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.focus {
        Focus::Actions => render_actions(frame, area, app),
        Focus::Backups => render_backups(frame, area, app),
        Focus::Output => render_output(frame, area, app),
    }
}

fn render_actions(frame: &mut Frame, area: Rect, app: &mut App) {
    let width = area.width.saturating_sub(4) as usize;
    let items: Vec<ListItem> = ACTIONS
        .iter()
        .map(|item| ListItem::new(format!("{:<width$}", item.label, width = width)))
        .collect();
    let list = List::new(items)
        .block(pane_block("Actions", app.focus == Focus::Actions))
        .highlight_style(selection_style());

    frame.render_stateful_widget(list, area, &mut app.action_state);
}

fn render_backups(frame: &mut Frame, area: Rect, app: &mut App) {
    let detail = app.selected_backup().map(|backup| {
        let mut parts = Vec::new();
        if !backup.label.is_empty() {
            parts.push(backup.label.clone());
        }
        parts.push(format!("v{} {}", backup.version, backup.compression));
        parts.join(" · ")
    });
    let block = pane_block_with_detail("Backups", detail.as_deref(), app.focus == Focus::Backups);

    if app.snapshot.backups.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [_, middle, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(2),
            Constraint::Fill(1),
        ])
        .areas(inner);
        let message = app
            .snapshot
            .warning
            .clone()
            .unwrap_or_else(|| "No local backups. Press R to refresh.".to_string());
        let paragraph = Paragraph::new(Text::from(vec![
            Line::from(Span::styled("No backups here", fg(MUTED).bold())),
            Line::from(Span::styled(message, fg(MUTED))),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(paragraph, middle);
        return;
    }

    let rows = app.snapshot.backups.iter().map(|backup| {
        let label = if backup.label.is_empty() {
            String::new()
        } else {
            format!("  {}", backup.label)
        };
        Row::new(vec![
            Cell::from(format!("{}{}", backup.name, label)),
            Cell::from(backup.date.clone()),
            Cell::from(human_size(backup.size)),
            Cell::from(backup.packages.to_string()),
        ])
    });

    let header = Row::new(["NAME", "DATE", "SIZE", "PKGS"])
        .style(fg(MUTED).bold())
        .bottom_margin(0);
    let widths = [
        Constraint::Min(12),
        Constraint::Length(16),
        Constraint::Length(9),
        Constraint::Length(4),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(selection_style());

    frame.render_stateful_widget(table, area, &mut app.backup_state);
}

fn render_output(frame: &mut Frame, area: Rect, app: &App) {
    let block = pane_block("Output", app.focus == Focus::Output);
    let inner_height = block.inner(area).height as usize;
    let len = app.output.len();
    let max_offset = len.saturating_sub(inner_height);
    let offset = app.output_offset.min(max_offset);
    let end = len.saturating_sub(offset);
    let start = end.saturating_sub(inner_height);

    let lines: Vec<Line> = app.output[start..end]
        .iter()
        .map(|line| render_log_line(line))
        .collect();

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn render_log_line(line: &str) -> Line<'static> {
    let Some(parts) = parse_log_line(line) else {
        if line.starts_with("$ ") {
            return Line::from(Span::styled(line.to_string(), fg(ACCENT).bold()));
        }
        return Line::from(line.to_string());
    };

    let level_style = match parts.level.as_str() {
        "INFO" => fg(SUCCESS),
        "WARN" | "WARNING" => fg(WARN).bold(),
        "ERROR" | "FAIL" => fg(DANGER).bold(),
        _ => fg(MUTED),
    };

    Line::from(vec![
        Span::styled(parts.timestamp, fg(MUTED)),
        Span::raw("  "),
        Span::styled(format!("{:<7}", parts.level), level_style),
        Span::raw(" "),
        Span::raw(parts.message),
    ])
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let (marker, marker_style) = if let Some(frame) = app.spinner() {
        (frame, fg(ACCENT).bold())
    } else if app.snapshot.warning.is_some() {
        ("●", fg(WARN))
    } else {
        ("●", fg(SUCCESS))
    };
    let status_style = if app.snapshot.warning.is_some() {
        fg(WARN)
    } else {
        fg(MUTED)
    };
    let status = Line::from(vec![
        Span::styled(format!(" {marker} "), marker_style),
        Span::styled(app.status.clone(), status_style),
    ]);

    let mut hint_spans = vec![Span::styled(
        format!(" ▸ {} ", app.focus.title()),
        fg(ACCENT).bold(),
    )];
    for (index, (key, action)) in hints(app.focus, area.width >= 80).iter().enumerate() {
        if index > 0 {
            hint_spans.push(Span::raw("  "));
        }
        hint_spans.push(Span::styled(*key, fg(ACCENT).bold()));
        if !action.is_empty() {
            hint_spans.push(Span::raw(" "));
            hint_spans.push(Span::styled(*action, fg(MUTED)));
        }
    }

    frame.render_widget(
        Paragraph::new(Text::from(vec![status, Line::from(hint_spans)])),
        area,
    );
}

fn hints(focus: Focus, wide: bool) -> &'static [(&'static str, &'static str)] {
    match (focus, wide) {
        (Focus::Actions, true) => &[
            ("↑↓", "move"),
            ("Enter", "run"),
            ("Tab", "focus"),
            ("R", "refresh"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (Focus::Actions, false) => &[
            ("↑↓", "move"),
            ("Enter", "run"),
            ("Tab", ""),
            ("R", ""),
            ("?", ""),
            ("q", ""),
        ],
        (Focus::Backups, true) => &[
            ("↑↓", "move"),
            ("Enter", "restore"),
            ("d", "delete"),
            ("Tab", "focus"),
            ("R", "refresh"),
            ("q", "quit"),
        ],
        (Focus::Backups, false) => &[
            ("↑↓", ""),
            ("Enter", ""),
            ("d", ""),
            ("Tab", ""),
            ("R", ""),
            ("q", ""),
        ],
        (Focus::Output, true) => &[
            ("↑↓", "scroll"),
            ("PgUp/PgDn", "page"),
            ("G", "bottom"),
            ("Tab", "focus"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (Focus::Output, false) => &[("↑↓", "scroll"), ("Tab", ""), ("?", ""), ("q", "")],
    }
}

fn render_too_small(frame: &mut Frame, area: Rect) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(WARN));
    let paragraph = Paragraph::new(Text::from(vec![
        Line::from(Span::styled("terminal too small", fg(WARN).bold())),
        Line::from(Span::styled(
            format!(
                "need {}×{} — current {}×{}",
                MIN_WIDTH, MIN_HEIGHT, area.width, area.height
            ),
            fg(MUTED),
        )),
    ]))
    .alignment(Alignment::Center)
    .block(block);
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(4),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(paragraph, middle);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = centered(area, 64, 24);
    let mut lines = vec![Line::from(Span::styled("Keys", fg(ACCENT).bold()))];
    for (key, action) in [
        ("↑ / ↓", "move selection"),
        ("Enter", "run action or restore selected backup"),
        ("d", "delete selected backup"),
        ("Tab", "switch pane"),
        ("R", "refresh backups and status"),
        ("g / G", "top / bottom of output"),
        ("?  / Esc", "close this help"),
        ("q", "quit"),
    ] {
        lines.push(Line::from(vec![
            Span::styled(format!("  {key:<9}"), fg(ACCENT).bold()),
            Span::styled(action, fg(MUTED)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Actions", fg(ACCENT).bold())));
    lines.push(Line::from(Span::styled(
        "  Backup, restore, delete, list, logs, disk check,",
        fg(MUTED),
    )));
    lines.push(Line::from(Span::styled(
        "  setup, schedule, clear cache.",
        fg(MUTED),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Interactive commands hand the terminal over to termux-sync.",
        fg(MUTED),
    )));

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(pane_block("Help", true)),
        popup,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn fg(color: Color) -> Style {
    if no_color() {
        Style::new()
    } else {
        Style::new().fg(color)
    }
}

fn selection_style() -> Style {
    if no_color() {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::new()
            .fg(Color::Black)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD)
    }
}

fn badge(text: &str, color: Color) -> Span<'static> {
    if no_color() {
        Span::styled(format!("[{text}]"), Style::new().bold())
    } else {
        Span::styled(
            format!(" {text} "),
            Style::new()
                .fg(Color::Black)
                .bg(color)
                .add_modifier(Modifier::BOLD),
        )
    }
}

fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

fn pane_block(title: &str, focused: bool) -> Block<'static> {
    pane_block_with_detail(title, None, focused)
}

fn pane_block_with_detail(title: &str, detail: Option<&str>, focused: bool) -> Block<'static> {
    let style = if focused {
        fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        fg(MUTED)
    };
    let marker = if focused { "▸" } else { " " };
    let mut title_spans = vec![Span::styled(format!(" {marker} {title} "), style)];
    if let Some(detail) = detail {
        title_spans.push(Span::styled(format!("· {detail} "), fg(MUTED)));
    }

    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(title_spans))
        .border_style(style)
        .padding(Padding::horizontal(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Backup, Config, Snapshot};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app() -> App {
        App::with_snapshot(Snapshot {
            config: Config::default(),
            backups: vec![Backup {
                name: "termux_backup_20260101_120000".to_string(),
                date: "2026-01-01 12:00".to_string(),
                label: "before-upgrade".to_string(),
                packages: 3,
                size: 3072,
                version: "1.3.9".to_string(),
                compression: "GZ".to_string(),
            }],
            log: vec!["2026-01-01 12:00 [INFO] Backup completed".to_string()],
            warning: None,
        })
    }

    fn render_at(width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = app();
        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw succeeds");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn layout_plan_has_floor_and_single_pane_band() {
        assert_eq!(plan(Rect::new(0, 0, 47, 12)), LayoutPlan::TooSmall);
        assert_eq!(plan(Rect::new(0, 0, 48, 11)), LayoutPlan::TooSmall);
        assert_eq!(plan(Rect::new(0, 0, 48, 12)), LayoutPlan::Single);
        assert_eq!(plan(Rect::new(0, 0, 79, 24)), LayoutPlan::Single);
        assert_eq!(plan(Rect::new(0, 0, 80, 24)), LayoutPlan::Split);
    }

    #[test]
    fn wide_frame_shows_badges_panes_and_selected_detail() {
        let frame = render_at(100, 30);
        assert!(frame.contains("termux-sync"));
        assert!(frame.contains("LOCAL"));
        assert!(frame.contains("Actions"));
        assert!(frame.contains("Backups"));
        assert!(frame.contains("Output"));
        assert!(frame.contains("termux_backup"));
        assert!(frame.contains("v1.3.9 GZ"));
        assert!(frame.contains("╭"));
    }

    #[test]
    fn output_pane_styles_log_lines() {
        let frame = render_at(100, 30);
        assert!(frame.contains("INFO"));
        assert!(frame.contains("Backup completed"));
    }

    #[test]
    fn narrow_frame_shows_only_focused_pane() {
        let frame = render_at(60, 20);
        assert!(frame.contains("Actions"));
        assert!(frame.contains("▸ Actions"));
        assert!(!frame.contains("Backups"));
    }

    #[test]
    fn too_small_frame_names_the_minimum() {
        let frame = render_at(40, 10);
        assert!(frame.contains("terminal too small"));
        assert!(frame.contains("need 48×12"));
    }

    #[test]
    fn help_overlay_renders_over_dashboard() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = app();
        app.show_help = true;
        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw succeeds");
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("Help"));
        assert!(content.contains("switch pane"));
    }
}
