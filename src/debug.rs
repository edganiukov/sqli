use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;

static DEBUG_LOG: Mutex<Option<File>> = Mutex::new(None);
static DEBUG_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn init(enabled: bool) {
    DEBUG_ENABLED.store(enabled, std::sync::atomic::Ordering::SeqCst);

    if !enabled {
        return;
    }

    let log_path = PathBuf::from("/tmp/sqli.log");

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
        }
        Err(_) => {}
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
