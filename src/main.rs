mod app;
mod cassandra;
mod clickhouse;
mod config;
mod controller;
mod db;
mod mysql;
mod postgres;

use std::io;
use std::path::PathBuf;

use app::App;
use clap::Parser;
use controller::Controller;

use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyEventKind};
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
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, args.config);
    restore_terminal()?;
    result
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(io::stdout()))
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, config_path: Option<PathBuf>) -> io::Result<()> {
    let controller = Controller::new(config_path);
    let mut app = App::new(controller);

    loop {
        terminal.draw(|frame| app.draw(frame))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                app.handle_key(key);
            }
        }

        if app.quit() {
            break;
        }
    }

    Ok(())
}
