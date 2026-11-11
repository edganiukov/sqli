use std::env;
use std::fs;
use std::io;
use std::process::Command;

use crate::terminal;

/// Open content in an external editor and return the edited content.
/// Uses $EDITOR environment variable, falling back to vim.
pub fn edit_in_external_editor(content: &str, file_extension: &str) -> io::Result<String> {
    let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());

    // Create temp file with the content
    let temp_dir = env::temp_dir();
    let temp_file = temp_dir.join(format!(
        "sqli_edit_{}.{}",
        std::process::id(),
        file_extension
    ));

    fs::write(&temp_file, content)?;

    // Hand the terminal to the editor in its normal state: with mouse
    // reporting still on, an editor that does not use the mouse would receive
    // every click as typed garbage.
    terminal::leave();

    // Run the editor
    let status = Command::new(&editor).arg(&temp_file).status();

    // Take the terminal back. Editors switch mouse reporting and bracketed
    // paste off when they exit, so these have to be requested again here; the
    // caller forces a full redraw.
    terminal::enter()?;

    // Check editor status and read result
    match status {
        Ok(exit_status) if exit_status.success() => {
            let result = fs::read_to_string(&temp_file)?;
            fs::remove_file(&temp_file).ok(); // Clean up
            Ok(result)
        }
        Ok(_) => {
            fs::remove_file(&temp_file).ok();
            Err(io::Error::other("Editor exited with error"))
        }
        Err(e) => {
            fs::remove_file(&temp_file).ok();
            Err(io::Error::other(format!(
                "Failed to run editor '{}': {}",
                editor, e
            )))
        }
    }
}
