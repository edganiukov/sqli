//! Terminal mode management for the TUI.
//!
//! Everything that switches the terminal into or out of TUI mode goes through
//! here, so the main loop, the external editor hand-off and the panic hook all
//! agree on which modes are on.

use std::io;

use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

/// Switch the terminal into TUI mode.
pub fn enter() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_input_modes();
    Ok(())
}

/// Request the input reporting the UI relies on: mouse events, bracketed
/// paste and focus changes.
///
/// These are terminal-side switches that another program sharing the terminal
/// can turn off behind our back (an editor does so when it exits). Requesting
/// them again is harmless, so this is also called whenever the terminal may
/// have been used by something else.
pub fn enable_input_modes() {
    let mut out = io::stdout();
    // Each is best effort on its own: not every terminal supports all three.
    let _ = execute!(out, EnableMouseCapture);
    let _ = execute!(out, EnableBracketedPaste);
    let _ = execute!(out, EnableFocusChange);
}

/// Hand the terminal back in its normal state. Best effort: every step runs
/// even if an earlier one fails, so a write error cannot strand the terminal
/// in raw mode.
pub fn leave() {
    let mut out = io::stdout();
    let _ = execute!(out, DisableFocusChange);
    let _ = execute!(out, DisableBracketedPaste);
    let _ = execute!(out, DisableMouseCapture);
    let _ = execute!(out, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

/// Restore the terminal before a panic message is printed, so a crash does not
/// leave the shell in raw mode with mouse reporting on.
///
/// Only a panic on the calling (UI) thread ends the program. A panic in a
/// background task just fails that task, so the screen is left alone and the
/// message goes to the debug log instead of over the UI.
pub fn install_panic_hook() {
    let ui_thread = std::thread::current().id();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == ui_thread {
            leave();
            default_hook(info);
        } else {
            crate::debug_log!("panic in background task: {}", info);
        }
    }));
}
