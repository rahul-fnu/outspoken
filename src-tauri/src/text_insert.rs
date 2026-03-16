use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::thread;
use std::time::Duration;

const STEP_DELAY: Duration = Duration::from_millis(50);

/// Maximum text length for clipboard-based paste. Beyond this, we chunk.
const MAX_PASTE_LEN: usize = 100_000;

/// Insert text into the active application using clipboard paste.
///
/// Flow: set clipboard → simulate Cmd/Ctrl+V.
/// Falls back to character-by-character typing if paste simulation fails.
pub fn insert_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    // Truncate extremely long text to avoid clipboard issues
    let text = if text.len() > MAX_PASTE_LEN {
        &text[..MAX_PASTE_LEN]
    } else {
        text
    };

    match insert_via_paste(text) {
        Ok(()) => Ok(()),
        Err(paste_err) => {
            eprintln!("Paste insertion failed ({paste_err}), falling back to typing");
            insert_via_typing(text)
        }
    }
}

/// Insert text by setting clipboard and simulating Ctrl/Cmd+V.
fn insert_via_paste(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard init failed: {e}"))?;

    clipboard
        .set_text(text)
        .map_err(|e| format!("Failed to set clipboard: {e}"))?;

    thread::sleep(STEP_DELAY);

    let result = simulate_paste();

    thread::sleep(STEP_DELAY);

    result
}

/// Simulate Ctrl+V / Cmd+V keystroke.
fn simulate_paste() -> Result<(), String> {
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("Enigo init failed: {e}"))?;

    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };

    enigo
        .key(modifier, Direction::Press)
        .map_err(|e| format!("Key press failed: {e}"))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| format!("Key click failed: {e}"))?;
    enigo
        .key(modifier, Direction::Release)
        .map_err(|e| format!("Key release failed: {e}"))?;

    Ok(())
}

/// Fallback: type text character-by-character using enigo.
/// Slower but works in apps that don't respond to paste.
fn insert_via_typing(text: &str) -> Result<(), String> {
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("Enigo init failed: {e}"))?;

    enigo
        .text(text)
        .map_err(|e| format!("Text input failed: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_noop() {
        assert!(insert_text("").is_ok());
    }
}
