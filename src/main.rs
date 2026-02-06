mod app;
mod cassandra;
mod clickhouse;
mod completion;
mod config;
mod controller;
mod db;
mod debug;
mod editor;
mod error;
mod mysql;
mod postgres;
mod sqlite;
mod templates;

use std::io;
use std::path::PathBuf;

use app::App;
use clap::Parser;
use controller::Controller;

use crossterm::ExecutableCommand;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;

#[derive(Parser)]
#[command(name = "sqli")]
#[command(about = "Interactive SQL client for multiple databases")]
struct Args {
    /// Path to config file
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Print debug information during startup
    #[arg(short, long)]
    debug: bool,

    /// Connect using a connection string: <type>://user:pass@host:port/db
    /// Types: pg (PostgreSQL), my (MySQL), cs (Cassandra/ScyllaDB), ch (ClickHouse), sq (SQLite)
    /// Examples:
    ///   pg://postgres:secret@localhost:5432/mydb
    ///   my://root@localhost:3306
    ///   sq:///path/to/database.db
    #[arg(long, value_name = "URL")]
    connect: Option<String>,
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    // Initialize debug logging before anything else
    debug::init(args.debug);

    // Parse --connect URL if provided
    let cli_connection = if let Some(ref url) = args.connect {
        match config::parse_connection_string(url) {
            Ok(conn) => Some(conn),
            Err(e) => {
                eprintln!("Error parsing connection string: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    // Load config before entering raw mode so errors are visible
    let connections = config::load_config(args.config);
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, connections, cli_connection);
    restore_terminal()?;
    result
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    io::stdout()
        .execute(EnterAlternateScreen)?
        .execute(EnableMouseCapture)?;
    // Bracketed paste may not be supported in all environments (e.g. some tmux configs)
    let _ = io::stdout().execute(EnableBracketedPaste);
    Terminal::new(CrosstermBackend::new(io::stdout()))
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    let _ = io::stdout().execute(DisableBracketedPaste);
    io::stdout()
        .execute(DisableMouseCapture)?
        .execute(LeaveAlternateScreen)?;
    Ok(())
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    connections: Vec<controller::DatabaseConn>,
    cli_connection: Option<controller::DatabaseConn>,
) -> io::Result<()> {
    let controller = Controller::with_connections(connections, cli_connection);
    let mut app = App::new(controller);

    loop {
        // Check if a full redraw is needed (e.g., after external editor)
        if app.needs_redraw() {
            terminal.clear()?;
        }

        terminal.draw(|frame| app.draw(frame))?;
        // Poll pending async operations
        app.poll_pending();
        // Drain all available events before redrawing
        if event::poll(std::time::Duration::from_millis(100))? {
            loop {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        app.handle_key(key);
                    }
                    Event::Mouse(mouse) => {
                        app.handle_mouse(mouse);
                    }
                    Event::Paste(text) => {
                        app.handle_paste(text);
                    }
                    Event::Resize(_, _) => {
                        app.handle_resize();
                    }
                    _ => {}
                }
                // Process remaining queued events without blocking
                if !event::poll(std::time::Duration::ZERO)? {
                    break;
                }
            }
        }

        // Tick spinner
        app.tick_spinner();
        if app.quit() {
            break;
        }
    }

    Ok(())
}
