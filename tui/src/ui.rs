use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Cell, Clear, List, ListItem, Paragraph, Row, Table};

use crate::app::{ACTIONS, App, Focus};
use crate::data::human_size;

pub const MIN_WIDTH: u16 = 48;
pub const MIN_HEIGHT: u16 = 12;
const SPLIT_WIDTH: u16 = 80;

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
    let mut line = vec![
        Span::styled(" termux-sync", fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" tui", fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(config.storage.to_uppercase(), fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled(
            format!("{} backups", app.snapshot.backups.len()),
            fg(Color::DarkGray),
        ),
    ];
    if area.width >= 100 {
        line.push(Span::raw("  "));
        line.push(Span::styled(&config.local_path, fg(Color::DarkGray)));
        line.push(Span::raw("  "));
        line.push(Span::styled(
            format!(
                "{} · max {} · auto {}",
                config.compression.to_uppercase(),
                config.max_backups,
                config.schedule
            ),
            fg(Color::DarkGray),
        ));
    }
    if app.snapshot.warning.is_some() {
        line.push(Span::styled("  [!]", fg(Color::Yellow)));
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
    let items: Vec<ListItem> = ACTIONS
        .iter()
        .map(|item| ListItem::new(item.label))
        .collect();
    let list = List::new(items)
        .block(pane_block("Actions", app.focus == Focus::Actions))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED).bold());

    frame.render_stateful_widget(list, area, &mut app.action_state);
}

fn render_backups(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = pane_block("Backups", app.focus == Focus::Backups);

    if app.snapshot.backups.is_empty() {
        let message = app
            .snapshot
            .warning
            .clone()
            .unwrap_or_else(|| "No local backups found. Press R to refresh.".to_string());
        let paragraph = Paragraph::new(message)
            .block(block)
            .style(fg(Color::DarkGray));
        frame.render_widget(paragraph, area);
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

    let header = Row::new(["Name", "Date", "Size", "Pkgs"])
        .style(fg(Color::DarkGray))
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
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(table, area, &mut app.backup_state);
}

fn render_output(frame: &mut Frame, area: Rect, app: &App) {
    let block = pane_block("Output", app.focus == Focus::Output);
    let inner_height = area.height.saturating_sub(2) as usize;
    let len = app.output.len();
    let max_offset = len.saturating_sub(inner_height);
    let offset = app.output_offset.min(max_offset);
    let end = len.saturating_sub(offset);
    let start = end.saturating_sub(inner_height);

    let lines: Vec<Line> = app.output[start..end]
        .iter()
        .map(|line| Line::from(line.clone()))
        .collect();

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let status_style = if app.snapshot.warning.is_some() {
        fg(Color::Yellow)
    } else {
        fg(Color::DarkGray)
    };
    let status = Line::from(vec![
        Span::styled(" status ", fg(Color::DarkGray)),
        Span::styled(app.status.clone(), status_style),
    ]);

    let wide = area.width >= 80;
    let hints = match (app.focus, wide) {
        (Focus::Actions, true) => "↑↓ move  Enter run  Tab focus  R refresh  ? help  q quit",
        (Focus::Actions, false) => "↑↓ move  Enter  Tab  R  ?  q",
        (Focus::Backups, true) => "↑↓ move  Enter restore  d delete  Tab focus  R refresh  q quit",
        (Focus::Backups, false) => "↑↓ Enter  d  Tab  R  q",
        (Focus::Output, true) => "↑↓ scroll  PgUp/PgDn page  G bottom  Tab focus  ? help  q quit",
        (Focus::Output, false) => "↑↓ scroll  Tab  ?  q",
    };
    let hints = Line::from(vec![
        Span::styled(format!(" focus: {} ", app.focus.title()), fg(Color::Cyan)),
        Span::styled(hints, fg(Color::DarkGray)),
    ]);

    frame.render_widget(Paragraph::new(Text::from(vec![status, hints])), area);
}

fn render_too_small(frame: &mut Frame, area: Rect) {
    let message = format!(
        "terminal too small — need {}×{}\ncurrent: {}×{}",
        MIN_WIDTH, MIN_HEIGHT, area.width, area.height
    );
    let paragraph = Paragraph::new(message)
        .alignment(ratatui::layout::Alignment::Center)
        .style(fg(Color::Yellow));
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(2),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(paragraph, middle);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = centered(area, 62, 22);
    let lines = vec![
        Line::from(Span::styled("Keys", fg(Color::Cyan).bold())),
        Line::from("  ↑ / ↓      move selection"),
        Line::from("  Enter      run action or restore selected backup"),
        Line::from("  d          delete selected backup"),
        Line::from("  Tab        switch pane"),
        Line::from("  R          refresh backups and status"),
        Line::from("  g / G      top / bottom of output"),
        Line::from("  ?          close this help"),
        Line::from("  q          quit"),
        Line::from(""),
        Line::from(Span::styled("Actions", fg(Color::Cyan).bold())),
        Line::from("  Backup, restore, delete, list, logs, disk check,"),
        Line::from("  setup, schedule, clear cache."),
        Line::from(""),
        Line::from(Span::styled(
            "Interactive commands hand the terminal over to termux-sync.",
            fg(Color::DarkGray),
        )),
    ];
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

fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

fn pane_block(title: &str, focused: bool) -> Block<'static> {
    let (label, style) = if focused {
        (
            format!("> {title} "),
            fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )
    } else {
        (format!("  {title} "), fg(Color::DarkGray))
    };
    Block::bordered()
        .title(Span::styled(label, style))
        .border_style(style)
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
    fn wide_frame_shows_all_panes() {
        let frame = render_at(100, 30);
        assert!(frame.contains("termux-sync"));
        assert!(frame.contains("Actions"));
        assert!(frame.contains("Backups"));
        assert!(frame.contains("Output"));
        assert!(frame.contains("termux_backup"));
    }

    #[test]
    fn narrow_frame_shows_only_focused_pane() {
        let frame = render_at(60, 20);
        assert!(frame.contains("Actions"));
        assert!(!frame.contains("Backups"));
    }

    #[test]
    fn too_small_frame_names_the_minimum() {
        let frame = render_at(40, 10);
        assert!(frame.contains("terminal too small"));
        assert!(frame.contains("48"));
        assert!(frame.contains("12"));
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
