use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;

static DEBUG_LOG: Mutex<Option<File>> = Mutex::new(None);
static DEBUG_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn init(enabled: bool) -> Option<PathBuf> {
    DEBUG_ENABLED.store(enabled, std::sync::atomic::Ordering::SeqCst);

    if !enabled {
        return None;
    }

    let log_path = dirs::config_dir()
        .map(|p| p.join("sqli").join("debug.log"))
        .unwrap_or_else(|| PathBuf::from("sqli-debug.log"));

    // Ensure parent directory exists
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(mut file) => {
            let _ = writeln!(
                file,
                "\n=== sqli debug session started at {} ===",
                Local::now().format("%Y-%m-%d %H:%M:%S")
            );
            *DEBUG_LOG.lock().unwrap() = Some(file);
            Some(log_path)
        }
        Err(e) => {
            eprintln!("[debug] Failed to open log file {:?}: {}", log_path, e);
            None
        }
    }
}

pub fn log(message: &str) {
    if !DEBUG_ENABLED.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    if let Ok(mut guard) = DEBUG_LOG.lock() {
        if let Some(ref mut file) = *guard {
            let timestamp = Local::now().format("%H:%M:%S%.3f");
            let _ = writeln!(file, "[{}] {}", timestamp, message);
            let _ = file.flush();
        }
    }
}

#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        $crate::debug::log(&format!($($arg)*))
    };
}
