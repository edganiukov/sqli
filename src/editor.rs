use crossterm::ExecutableCommand;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::Command;

/// Open content in an external editor and return the edited content.
/// Uses $EDITOR environment variable, falling back to vim.
pub fn edit_in_external_editor(content: &str, file_extension: &str) -> io::Result<String> {
    let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());

    // Create temp file with the content
    let temp_dir = env::temp_dir();
    let temp_file = temp_dir.join(format!("sqli_edit_{}.{}", std::process::id(), file_extension));

    fs::write(&temp_file, content)?;

    // Leave TUI mode
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    // Run the editor
    let status = Command::new(&editor)
        .arg(&temp_file)
        .status();

    // Restore TUI mode
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    // Force a redraw by clearing
    print!("\x1B[2J\x1B[H");
    io::stdout().flush()?;

    // Check editor status and read result
    match status {
        Ok(exit_status) if exit_status.success() => {
            let result = fs::read_to_string(&temp_file)?;
            fs::remove_file(&temp_file).ok(); // Clean up
            Ok(result)
        }
        Ok(_) => {
            fs::remove_file(&temp_file).ok();
            Err(io::Error::new(io::ErrorKind::Other, "Editor exited with error"))
        }
        Err(e) => {
            fs::remove_file(&temp_file).ok();
            Err(io::Error::new(io::ErrorKind::Other, format!("Failed to run editor '{}': {}", editor, e)))
        }
    }
}
