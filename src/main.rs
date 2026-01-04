mod app;
mod cassandra;
mod clickhouse;
mod config;
mod controller;
mod db;
mod postgres;

use std::io;

use app::App;
use controller::Controller;

use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;

fn main() -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal);
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

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let controller = Controller::new();
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
