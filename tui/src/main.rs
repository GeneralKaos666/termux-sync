mod app;
mod data;
mod ui;

use std::io::{self, IsTerminal, Write};
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::app::App;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("termux-sync-tui {VERSION}");
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }
    if !io::stdout().is_terminal() {
        eprintln!("termux-sync-tui: refusing to start: stdout is not a terminal");
        std::process::exit(2);
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();

    loop {
        app.on_tick();
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key.code);
        }

        app.poll_worker();

        if let Some(argv) = app.pending.take() {
            handoff(terminal, &mut app, &argv)?;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handoff(terminal: &mut DefaultTerminal, app: &mut App, argv: &[String]) -> io::Result<()> {
    ratatui::restore();

    let result = data::cli_command(argv).status();

    print!("\nPress Enter to return to the dashboard…");
    io::stdout().flush()?;
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);

    *terminal = ratatui::init();
    terminal.clear()?;

    app.status = match result {
        Ok(status) if status.success() => format!("termux-sync {} finished", argv.join(" ")),
        Ok(status) => format!("termux-sync {} exited with {status}", argv.join(" ")),
        Err(err) => format!("cannot run termux-sync: {err}"),
    };
    app.refresh();
    Ok(())
}

fn print_help() {
    println!(
        "termux-sync-tui {VERSION}
Ratatui dashboard wrapper for the termux-sync CLI (spike).

Usage: termux-sync-tui

Keys:
  ↑ / ↓      move selection
  Enter      run action or restore selected backup
  d          delete selected backup
  Tab        switch pane
  R          refresh backups and status
  ?          help
  q          quit

Environment:
  TERMUX_SYNC_BIN   termux-sync executable to run (default: termux-sync)"
    );
}
