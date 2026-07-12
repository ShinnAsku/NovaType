//! Best-effort file logger for real-machine TSF debugging.
//!
//! Appends to `%TEMP%\novatype-tsf.log` inside whatever host process loaded
//! the DLL. Never panics and swallows all I/O errors: logging must not be able
//! to crash a host application.

use std::io::Write;

/// Appends one timestamped line to the debug log file.
pub fn log(message: &str) {
    let path = std::env::temp_dir().join("novatype-tsf.log");
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let pid = std::process::id();
    let _ = writeln!(
        file,
        "[{}.{:03}] [pid {pid}] {message}",
        now.as_secs(),
        now.subsec_millis()
    );
}
