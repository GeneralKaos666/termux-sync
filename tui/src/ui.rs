use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Cell, Clear, List, ListItem, Padding, Paragraph, Row, Table,
};

use crate::app::{ACTIONS, App, Focus};
use crate::data::{backup_age, human_size, parse_log_line};

pub const MIN_WIDTH: u16 = 48;
pub const MIN_HEIGHT: u16 = 12;
const SPLIT_WIDTH: u16 = 80;

const ACCENT: Color = Color::Cyan;
const SUCCESS: Color = Color::Green;
const WARN: Color = Color::Yellow;
const DANGER: Color = Color::Red;
const MAGENTA: Color = Color::Magenta;
const MUTED: Color = Color::DarkGray;
const GRADIENT_FROM: Color = Color::Rgb(70, 220, 255);
const GRADIENT_TO: Color = Color::Rgb(210, 120, 255);

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

    app.actions_area = Rect::default();
    app.backups_area = Rect::default();
    app.output_area = Rect::default();

    if plan(area) == LayoutPlan::TooSmall {
        render_too_small(frame, area);
        return;
    }

    let show_cards = area.width >= SPLIT_WIDTH && area.height >= 20;

    if show_cards {
        let [hero, cards, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .areas(area);

        render_hero(frame, hero, app);
        render_stats(frame, cards, app);
        render_body(frame, body, app);
        render_footer(frame, footer, app);
    } else {
        let [hero, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .areas(area);

        render_hero(frame, hero, app);
        render_body(frame, body, app);
        render_footer(frame, footer, app);
    }

    if app.show_help {
        render_help(frame, area, app);
    }

    if app.pending.is_some() {
        render_launch(frame, area, app);
    }

    if app.task.is_some() {
        render_task(frame, area, app);
    }
}

fn render_hero(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(ACCENT))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(inner);

    let mut title = gradient_spans("TERMUX SYNC");
    title.push(Span::styled("   backup dashboard", fg(MUTED)));
    if area.width >= 100 {
        title.push(Span::styled(
            format!(
                "   {} · max {}",
                app.snapshot.config.compression.to_uppercase(),
                app.snapshot.config.max_backups
            ),
            fg(MUTED),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(title)), left);

    let count = app.snapshot.backups.len();
    let count_label = if app.loading {
        "LOADING".to_string()
    } else if count == 1 {
        "1 BACKUP".to_string()
    } else {
        format!("{count} BACKUPS")
    };
    let count_color = if app.loading {
        ACCENT
    } else if count == 0 {
        MUTED
    } else {
        SUCCESS
    };
    let mut chips = vec![
        badge(&app.snapshot.config.storage.to_uppercase(), ACCENT),
        Span::raw(" "),
        badge(&count_label, count_color),
    ];
    if app.snapshot.warning.is_some() {
        chips.push(Span::raw(" "));
        chips.push(Span::styled("[!]", fg(WARN).bold()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(chips)).alignment(Alignment::Right),
        right,
    );
}

fn render_stats(frame: &mut Frame, area: Rect, app: &App) {
    let [backups, stored, packages, auto] = Layout::horizontal([Constraint::Fill(1); 4])
        .spacing(1)
        .areas(area);

    let total_size: u64 = app.snapshot.backups.iter().map(|backup| backup.size).sum();
    let latest_packages = app
        .snapshot
        .backups
        .first()
        .map_or(0, |backup| backup.packages);
    let placeholder = app.loading.then_some("…".to_string());
    let backups_value = placeholder
        .clone()
        .unwrap_or_else(|| app.snapshot.backups.len().to_string());
    let stored_value = placeholder
        .clone()
        .unwrap_or_else(|| human_size(total_size));
    let packages_value = placeholder
        .clone()
        .unwrap_or_else(|| latest_packages.to_string());
    let auto_value = placeholder.unwrap_or_else(|| app.snapshot.config.schedule.clone());

    stat_card(frame, backups, "BACKUPS", &backups_value, "saved", SUCCESS);
    stat_card(frame, stored, "STORED", &stored_value, "", ACCENT);
    stat_card(
        frame,
        packages,
        "PACKAGES",
        &packages_value,
        "latest",
        MAGENTA,
    );
    stat_card(frame, auto, "AUTO", &auto_value, "", WARN);
}

fn stat_card(frame: &mut Frame, area: Rect, title: &str, value: &str, unit: &str, color: Color) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(color))
        .title(Span::styled(format!(" {title} "), fg(color).bold()));
    let line = Line::from(vec![
        Span::styled(value.to_string(), fg(color).bold()),
        Span::styled(format!(" {unit}"), fg(MUTED)),
    ]);
    frame.render_widget(Paragraph::new(line).block(block), area);
}

fn render_body(frame: &mut Frame, area: Rect, app: &mut App) {
    if plan(frame.area()) == LayoutPlan::Split {
        let [left, right] =
            Layout::horizontal([Constraint::Length(26), Constraint::Min(0)]).areas(area);
        let [backups, output] =
            Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(right);

        render_actions(frame, left, app);
        render_backups(frame, backups, app);
        render_output(frame, output, app);
    } else {
        let progress = ease_out(app.pane_progress());
        let area = if progress < 1.0 {
            frame.render_widget(Clear, area);
            let width = (area.width as f32 * (0.55 + 0.45 * progress))
                .round()
                .clamp(1.0, area.width as f32) as u16;
            if app.pane_dir >= 0 {
                Rect {
                    x: area.x + area.width - width,
                    width,
                    ..area
                }
            } else {
                Rect { width, ..area }
            }
        } else {
            area
        };
        match app.focus {
            Focus::Actions => render_actions(frame, area, app),
            Focus::Backups => render_backups(frame, area, app),
            Focus::Output => render_output(frame, area, app),
        }
    }
}

fn render_actions(frame: &mut Frame, area: Rect, app: &mut App) {
    app.actions_area = area;
    let inner = area.width.saturating_sub(4) as usize;
    let label_width = inner.saturating_sub(2);
    let items: Vec<ListItem> = ACTIONS
        .iter()
        .map(|item| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", item.glyph), fg(ACCENT).bold()),
                Span::raw(format!("{:<width$}", item.label, width = label_width)),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(pane_block(
            "Actions",
            None,
            app.focus == Focus::Actions,
            ACCENT,
        ))
        .highlight_style(selection_style(ACCENT));

    frame.render_stateful_widget(list, area, &mut app.action_state);
}

fn render_backups(frame: &mut Frame, area: Rect, app: &mut App) {
    app.backups_area = area;
    let detail = app.selected_backup().map(|backup| {
        let mut parts = Vec::new();
        if !backup.label.is_empty() {
            parts.push(backup.label.clone());
        }
        parts.push(format!("v{} {}", backup.version, backup.compression));
        parts.join(" · ")
    });
    let block = pane_block(
        "Backups",
        detail.as_deref(),
        app.focus == Focus::Backups,
        SUCCESS,
    );

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
        let (title, message) = if app.loading {
            (
                format!("{} Loading backups", app.spinner().unwrap_or("◐")),
                "Reading manifests and log…".to_string(),
            )
        } else {
            ("No backups here".to_string(), message)
        };
        let paragraph = Paragraph::new(Text::from(vec![
            Line::from(Span::styled(title, fg(MUTED).bold())),
            Line::from(Span::styled(message, fg(MUTED))),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(paragraph, middle);
        return;
    }

    let max_size = app
        .snapshot
        .backups
        .iter()
        .map(|backup| backup.size)
        .max()
        .unwrap_or(1)
        .max(1);

    let rows = app.snapshot.backups.iter().map(|backup| {
        let label = if backup.label.is_empty() {
            String::new()
        } else {
            format!("  {}", backup.label)
        };
        Row::new(vec![
            Cell::from(format!("{}{}", short_name(&backup.name), label)),
            Cell::from(backup_age(backup)),
            Cell::from(human_size(backup.size)),
            Cell::from(size_bar(backup.size, max_size, 6)).style(fg(SUCCESS)),
            Cell::from(backup.packages.to_string()),
        ])
    });

    let header = Row::new(["NAME", "AGE", "SIZE", "USED", "PKGS"])
        .style(fg(MUTED).bold())
        .bottom_margin(0);
    let widths = [
        Constraint::Min(12),
        Constraint::Length(5),
        Constraint::Length(9),
        Constraint::Length(6),
        Constraint::Length(4),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(selection_style(SUCCESS));

    frame.render_stateful_widget(table, area, &mut app.backup_state);
}

fn render_output(frame: &mut Frame, area: Rect, app: &mut App) {
    app.output_area = area;
    let block = pane_block("Output", None, app.focus == Focus::Output, MAGENTA);
    let inner = block.inner(area);
    let inner_height = inner.height as usize;
    let len = app.output.len();
    let max_offset = len.saturating_sub(inner_height);
    let offset = app.output_offset.min(max_offset);
    let end = len.saturating_sub(offset);
    let start = end.saturating_sub(inner_height);

    let mut lines: Vec<Line> = app.output[start..end]
        .iter()
        .map(|line| render_log_line(line))
        .collect();

    let progress = ease_out(app.output_progress());
    if progress < 1.0 {
        frame.render_widget(Clear, inner);
        let shift = (((1.0 - progress) * inner_height as f32) * 0.6).round() as usize;
        if shift > 0 {
            let mut shifted = vec![Line::default(); shift];
            shifted.append(&mut lines);
            lines = shifted;
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn render_log_line(line: &str) -> Line<'static> {
    let Some(parts) = parse_log_line(line) else {
        if line.starts_with("$ ") {
            return Line::from(Span::styled(line.to_string(), fg(ACCENT).bold()));
        }
        return Line::from(line.to_string());
    };

    Line::from(vec![
        Span::styled(parts.timestamp, fg(MUTED)),
        Span::raw(" "),
        level_chip(&parts.level),
        Span::raw(" "),
        Span::raw(parts.message),
    ])
}

fn level_chip(level: &str) -> Span<'static> {
    let (label, color) = match level {
        "INFO" => ("INFO", SUCCESS),
        "WARN" | "WARNING" => ("WARN", WARN),
        "ERROR" | "FAIL" => ("ERROR", DANGER),
        "DEBUG" => ("DEBUG", MUTED),
        other => (other, MUTED),
    };
    if no_color() {
        Span::styled(format!("[{label}]"), Style::new().bold())
    } else {
        Span::styled(
            format!(" {label} "),
            Style::new()
                .fg(Color::Black)
                .bg(color)
                .add_modifier(Modifier::BOLD),
        )
    }
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

    let wide = area.width >= 80;
    let mut hint_spans = vec![
        badge(&app.focus.title().to_uppercase(), ACCENT),
        Span::raw("  "),
    ];
    for (index, (key, action)) in hints(app.focus, wide).iter().enumerate() {
        if index > 0 {
            hint_spans.push(Span::raw("  "));
        }
        if wide {
            hint_spans.push(keycap(key));
        } else {
            hint_spans.push(Span::styled(*key, fg(ACCENT).bold()));
        }
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
            ("↑↓", ""),
            ("Enter", ""),
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
        (Focus::Output, false) => &[("↑↓", ""), ("Tab", ""), ("?", ""), ("q", "")],
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

fn render_help(frame: &mut Frame, area: Rect, app: &App) {
    let progress = ease_out(app.help_progress());
    let max_width = area.width.saturating_sub(2).max(20) as f32;
    let max_height = area.height.saturating_sub(2).max(7) as f32;
    let width = (28.0 + 36.0 * progress).min(max_width) as u16;
    let height = (8.0 + 16.0 * progress).min(max_height) as u16;
    let popup = centered(area, width, height);
    let mut lines = vec![Line::from(vec![
        Span::styled("◆ ", fg(ACCENT).bold()),
        Span::styled("HELP", fg(ACCENT).bold()),
    ])];
    for (key, action) in [
        ("↑ / ↓", "move selection"),
        ("Enter", "run action or restore selected backup"),
        ("Click", "run action, select backup, focus pane"),
        ("d", "delete selected backup"),
        ("Tab", "switch pane"),
        ("R", "refresh backups and status"),
        ("g / G", "top / bottom of output"),
        ("?  / Esc", "close this help"),
        ("↑ / ↓", "scroll this help"),
        ("Ctrl+C", "quit immediately"),
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

    let inner_height = popup.height.saturating_sub(2) as usize;
    let max_scroll = lines.len().saturating_sub(inner_height);
    let scroll = app.help_scroll.min(max_scroll);
    let visible: Vec<Line> = lines.into_iter().skip(scroll).take(inner_height).collect();
    let detail = (max_scroll > 0).then_some("↑↓ scroll");

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(visible)).block(pane_block("Help", detail, true, ACCENT)),
        popup,
    );
}

fn render_launch(frame: &mut Frame, area: Rect, app: &App) {
    let Some(argv) = app.pending.as_ref() else {
        return;
    };
    let progress = ease_out(app.launch_progress());
    let width = (30.0 + 30.0 * progress).round() as u16;
    let height = (5.0 + 2.0 * progress).round() as u16;
    let popup = centered(area, width, height);
    let spinner = app.spinner().unwrap_or("◐");

    let lines = vec![
        Line::from(vec![
            Span::styled("◆ ", fg(ACCENT).bold()),
            Span::styled("LAUNCHING", fg(ACCENT).bold()),
        ]),
        Line::from(vec![
            Span::styled(format!("{spinner} "), fg(ACCENT).bold()),
            Span::styled(
                format!("termux-sync {}", argv.join(" ")),
                fg(SUCCESS).bold(),
            ),
        ]),
        Line::from(Span::styled("Handing the terminal over…", fg(MUTED))),
    ];

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(pane_block("Launch", None, true, ACCENT)),
        popup,
    );
}

fn render_task(frame: &mut Frame, area: Rect, app: &App) {
    let Some(task) = &app.task else {
        return;
    };
    let progress = ease_out(app.task_progress());
    let max_width = area.width.saturating_sub(2).max(20) as f32;
    let max_height = area.height.saturating_sub(2).max(6) as f32;
    let width = (32.0 + 32.0 * progress).min(max_width) as u16;
    let height = (6.0 + 2.0 * progress).min(max_height) as u16;
    let popup = centered(area, width, height);

    let (title, marker, color, detail) = if task.done {
        ("DONE", "✓", SUCCESS, "Captured output is ready")
    } else {
        (
            "WORKING",
            app.spinner().unwrap_or("◐"),
            ACCENT,
            "Capturing command output…",
        )
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("◆ ", fg(color).bold()),
            Span::styled(title, fg(color).bold()),
        ]),
        Line::from(vec![
            Span::styled(format!("{marker} "), fg(color).bold()),
            Span::styled(task.label.clone(), fg(SUCCESS).bold()),
        ]),
        Line::from(Span::styled(detail, fg(MUTED))),
        sweep_bar(
            (width.saturating_sub(4) as usize).clamp(8, 24),
            app.tick,
            task.done,
        ),
    ];

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(pane_block(title, None, true, color)),
        popup,
    );
}

fn sweep_bar(width: usize, tick: u64, done: bool) -> Line<'static> {
    let (filled, empty, color) = if done {
        ("▰", "", SUCCESS)
    } else if no_color() {
        ("#", "-", SUCCESS)
    } else {
        ("▰", "▱", ACCENT)
    };
    if done {
        return Line::from(Span::styled(filled.repeat(width), fg(color)));
    }

    let cycle = (width + 6) as u64;
    let head = (tick % cycle) as usize;
    let mut bar = String::with_capacity(width);
    for index in 0..width {
        let lit = if head >= 6 {
            (head - 6..head).contains(&index)
        } else {
            index < head || index >= width.saturating_sub(6 - head)
        };
        bar.push_str(if lit { filled } else { empty });
    }
    Line::from(Span::styled(bar, fg(color)))
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
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

fn selection_style(color: Color) -> Style {
    if no_color() {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::new()
            .fg(Color::Black)
            .bg(color)
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

fn keycap(key: &str) -> Span<'static> {
    if no_color() {
        Span::styled(format!("[{key}]"), Style::new().bold())
    } else {
        Span::styled(
            format!(" {key} "),
            Style::new()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
    }
}

fn gradient_spans(text: &str) -> Vec<Span<'static>> {
    if no_color() {
        return vec![Span::styled(text.to_string(), Style::new().bold())];
    }
    let chars: Vec<char> = text.chars().collect();
    let last = chars.len().saturating_sub(1).max(1) as f32;
    chars
        .into_iter()
        .enumerate()
        .map(|(index, ch)| {
            let t = index as f32 / last;
            let color = lerp_color(GRADIENT_FROM, GRADIENT_TO, t);
            Span::styled(ch.to_string(), Style::new().fg(color).bold())
        })
        .collect()
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let (Color::Rgb(fr, fg, fb), Color::Rgb(tr, tg, tb)) = (from, to) else {
        return from;
    };
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color::Rgb(mix(fr, tr), mix(fg, tg), mix(fb, tb))
}

fn short_name(name: &str) -> &str {
    name.strip_prefix("termux_backup_").unwrap_or(name)
}

fn size_bar(size: u64, max: u64, width: usize) -> String {
    if max == 0 {
        return " ".repeat(width);
    }
    let filled = ((size as f64 / max as f64) * width as f64).round() as usize;
    let filled = filled.clamp(0, width);
    if no_color() {
        format!("{}{}", "#".repeat(filled), "-".repeat(width - filled))
    } else {
        format!("{}{}", "▰".repeat(filled), "▱".repeat(width - filled))
    }
}

fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

fn pane_block(title: &str, detail: Option<&str>, focused: bool, color: Color) -> Block<'static> {
    let style = if focused {
        fg(color).add_modifier(Modifier::BOLD)
    } else if no_color() {
        Style::new()
    } else {
        fg(color).add_modifier(Modifier::DIM)
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
    use ratatui::crossterm::event::KeyCode;

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
                modified: None,
            }],
            log: vec!["2026-01-01 12:00 [INFO] Backup completed".to_string()],
            warning: None,
        })
    }

    fn render_at(width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = app();
        for _ in 0..6 {
            app.on_tick();
        }
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
    fn wide_frame_shows_hero_stats_panes_and_detail() {
        let frame = render_at(100, 30);
        assert!(frame.contains("TERMUX SYNC"));
        assert!(frame.contains("BACKUPS"));
        assert!(frame.contains("STORED"));
        assert!(frame.contains("PACKAGES"));
        assert!(frame.contains("LOCAL"));
        assert!(frame.contains("Actions"));
        assert!(frame.contains("Output"));
        assert!(frame.contains("20260101_120000"));
        assert!(frame.contains("v1.3.9 GZ"));
        assert!(frame.contains("╭"));
        assert!(frame.contains("↑"));
        assert!(frame.contains("☰"));
    }

    #[test]
    fn narrow_frame_drops_cards_but_keeps_hero() {
        let frame = render_at(60, 20);
        assert!(frame.contains("TERMUX SYNC"));
        assert!(frame.contains("ACTIONS"));
        assert!(!frame.contains("STORED"));
        assert!(!frame.contains("Backups"));
    }

    #[test]
    fn output_pane_styles_log_lines() {
        let frame = render_at(100, 30);
        assert!(frame.contains("INFO"));
        assert!(frame.contains("Backup completed"));
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
        app.open_help();
        for _ in 0..5 {
            app.on_tick();
        }
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
        assert!(content.contains("HELP"));
        assert!(content.contains("switch pane"));
    }

    #[test]
    fn launch_overlay_animates_before_handoff() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = app();
        app.run(crate::app::Action::Backup);
        app.on_tick();
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
        assert!(content.contains("LAUNCHING"));
        assert!(content.contains("termux-sync backup"));
    }

    #[test]
    fn backup_names_drop_the_app_prefix() {
        assert_eq!(
            short_name("termux_backup_20260101_120000"),
            "20260101_120000"
        );
        assert_eq!(short_name("custom-name"), "custom-name");
    }

    #[test]
    fn task_overlay_renders_for_captured_commands() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = app();
        app.task = Some(crate::app::TaskOverlay {
            label: "termux-sync list".to_string(),
            started_at: 0,
            done: false,
            done_at: 0,
        });
        app.on_tick();
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
        assert!(content.contains("WORKING"));
        assert!(content.contains("termux-sync list"));
    }

    #[test]
    fn help_scrolls_at_the_minimum_size() {
        let backend = TestBackend::new(48, 12);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = app();
        app.open_help();
        for _ in 0..5 {
            app.on_tick();
        }
        for _ in 0..10 {
            app.handle_key(KeyCode::Down);
        }
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
        assert!(content.contains("clear cache"));
        assert!(content.contains("↑↓ scroll"));
    }

    #[test]
    fn loading_state_names_what_is_happening() {
        let mut app = app();
        app.snapshot.backups.clear();
        app.loading = true;
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
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
        assert!(content.contains("LOADING"));
        assert!(content.contains("Loading backups"));
    }

    #[test]
    fn standard_size_keeps_all_panes() {
        let frame = render_at(80, 24);
        assert!(frame.contains("TERMUX SYNC"));
        assert!(frame.contains("Actions"));
        assert!(frame.contains("Backups"));
        assert!(frame.contains("Output"));
        assert!(frame.contains("ACTIONS"));
    }

    #[test]
    fn size_bar_fills_proportionally() {
        assert_eq!(size_bar(0, 100, 4), "▱▱▱▱");
        assert_eq!(size_bar(50, 100, 4), "▰▰▱▱");
        assert_eq!(size_bar(100, 100, 4), "▰▰▰▰");
    }
}
