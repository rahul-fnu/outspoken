use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::tray::{TrayRecordingState, TrayState};

const DEFAULT_HOTKEY: &str = "Ctrl+Shift+Space";

/// Stores the currently configured hotkey string.
pub struct HotkeyConfig {
    pub shortcut: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            shortcut: DEFAULT_HOTKEY.to_string(),
        }
    }
}

pub type HotkeyConfigState = Mutex<HotkeyConfig>;

/// Register the global shortcut for dictation toggle.
pub fn register_hotkey<R: Runtime>(app: &AppHandle<R>, shortcut_str: &str) -> Result<(), String> {
    let shortcut: Shortcut = shortcut_str
        .parse()
        .map_err(|e| format!("Invalid shortcut '{shortcut_str}': {e}"))?;

    let global_shortcut = app.global_shortcut();

    // Unregister all existing shortcuts first
    global_shortcut
        .unregister_all()
        .map_err(|e| format!("Failed to unregister shortcuts: {e}"))?;

    let app_handle = app.clone();
    global_shortcut
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let _ = toggle_recording_from_hotkey(&app_handle);
            }
        })
        .map_err(|e| format!("Failed to register shortcut '{shortcut_str}': {e}"))?;

    Ok(())
}

/// Unregister all global shortcuts.
pub fn unregister_all<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| format!("Failed to unregister shortcuts: {e}"))
}

/// Toggle recording, reusing the same tray event mechanism.
fn toggle_recording_from_hotkey<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let state = app.state::<TrayRecordingState>();
    let current = TrayState::from(state.load(std::sync::atomic::Ordering::Relaxed));

    match current {
        TrayState::Idle => {
            app.emit("tray-toggle-recording", "start")
                .map_err(|e| format!("Failed to emit start event: {e}"))?;
        }
        TrayState::Recording => {
            app.emit("tray-toggle-recording", "stop")
                .map_err(|e| format!("Failed to emit stop event: {e}"))?;
        }
        TrayState::Processing => {
            // Don't interrupt processing
        }
    }
    Ok(())
}
