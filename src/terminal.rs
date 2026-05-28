use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

/// Poll for 's' key press. Returns true if 's' was pressed, false on timeout.
/// Returns false if raw mode isn't available (e.g., non-interactive terminal).
pub fn wait_for_stop_key(timeout: Duration) -> anyhow::Result<bool> {
    // Try to enable raw mode - if it fails, just return false (no key pressed)
    if enable_raw_mode().is_err() {
        std::thread::sleep(timeout);
        return Ok(false);
    }

    let result = if poll(timeout)? {
        if let Event::Key(key) = read()? {
            matches!(key.code, KeyCode::Char('s'))
        } else {
            false
        }
    } else {
        false
    };

    let _ = disable_raw_mode();
    Ok(result)
}

/// Print "Capturing, press s to stop." to stderr.
pub fn print_capturing() {
    eprintln!("Capturing, press s to stop.");
}

/// Print the saved output path to stderr.
pub fn print_saved(path: &std::path::Path) {
    eprintln!("Saved to {}", path.display());
}

/// Print a colored error message to stderr.
pub fn print_error(msg: &str) {
    eprintln!("\x1b[31merror\x1b[0m: {}", msg);
}
