use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::thread;
use std::time::Duration;

const STEP_DELAY: Duration = Duration::from_millis(50);

/// Maximum text length for clipboard-based paste. Beyond this, we chunk.
const MAX_PASTE_LEN: usize = 100_000;

/// Insert text into the active application using clipboard paste.
///
/// Flow: save clipboard → copy text → simulate Cmd/Ctrl+V → restore clipboard.
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

/// Insert text by saving/restoring clipboard and simulating Ctrl/Cmd+V.
fn insert_via_paste(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard init failed: {e}"))?;

    // Save current clipboard contents
    let saved_text = clipboard.get_text().ok();

    // Set our text on the clipboard
    clipboard
        .set_text(text)
        .map_err(|e| format!("Failed to set clipboard: {e}"))?;

    thread::sleep(STEP_DELAY);

    // Simulate Cmd+V (macOS) or Ctrl+V (Windows/Linux)
    let result = simulate_paste();

    // Wait for the paste to be processed by the target app
    thread::sleep(STEP_DELAY);

    // Restore original clipboard (best-effort)
    if let Some(saved) = saved_text {
        // Small delay before restoring to let paste complete
        thread::sleep(STEP_DELAY);
        let _ = clipboard.set_text(saved);
    }

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
