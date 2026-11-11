use std::env;
use std::fs;
use std::io;
use std::path::Path;
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
    let status = editor_command(&editor, &temp_file).status();

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
        Ok(exit_status) => {
            fs::remove_file(&temp_file).ok();
            Err(io::Error::other(format!(
                "Editor '{}' failed ({})",
                editor, exit_status
            )))
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

/// Build the command that opens `file` in `editor`.
///
/// `$EDITOR` is a command line, not a program name: it often carries
/// arguments (`code -w`, `nvim -u NONE`). Like git, run it through the shell
/// so those work, passing the file as a separate argument so its path is
/// never re-parsed.
fn editor_command(editor: &str, file: &Path) -> Command {
    if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command
            .arg("/C")
            .arg(format!("{} \"{}\"", editor, file.display()));
        command
    } else {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(format!("{} \"$1\"", editor))
            .arg("sh")
            .arg(file);
        command
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn editor_may_carry_arguments_and_the_path_may_contain_spaces() {
        let dir = env::temp_dir().join(format!("sqli editor test {}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("query one.sql");
        fs::write(&file, "select 1").unwrap();

        // An "editor" with an argument, which empties the file it is given.
        let status = editor_command("cp /dev/null", &file).status().unwrap();

        assert!(status.success());
        assert_eq!(fs::read_to_string(&file).unwrap(), "");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_editor_is_reported_as_a_failure() {
        let status = editor_command("sqli-no-such-editor", Path::new("/dev/null"))
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success());
    }
}
