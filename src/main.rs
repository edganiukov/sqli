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
mod format;
mod mysql;
mod postgres;
mod result_table;
mod session;
mod sql;
mod sqlite;
mod ssh_sqlite;
mod templates;
mod terminal;

use std::io;
use std::path::PathBuf;

use app::App;
use clap::Parser;
use controller::Controller;

use crossterm::event::{self, Event, KeyEventKind};
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
    /// Types: pg, my, cs, ch, sq (add 's' for TLS: pgs, mys, css, chs)
    /// Examples: pg://postgres@localhost/mydb, pgs://user@secure.host/db
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

    // Load config before entering raw mode so errors are visible. With
    // --connect the config is not used at all, so it is not read either.
    let connections = if cli_connection.is_some() {
        Vec::new()
    } else {
        match config::load_config(args.config) {
            Ok(connections) => connections,
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
    };

    terminal::install_panic_hook();
    terminal::enter()?;
    let result = Terminal::new(CrosstermBackend::new(io::stdout()))
        .and_then(|mut terminal| run(&mut terminal, connections, cli_connection));
    terminal::leave();
    result
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
                    // Coming back to this terminal (e.g. another tmux window
                    // was in front): make sure mouse reporting is still on.
                    Event::FocusGained => terminal::enable_input_modes(),
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
