use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

const TRAY_ID: &str = "outspoken-tray";

/// Recording state for the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrayState {
    Idle = 0,
    Recording = 1,
    Processing = 2,
}

impl From<u8> for TrayState {
    fn from(v: u8) -> Self {
        match v {
            1 => TrayState::Recording,
            2 => TrayState::Processing,
            _ => TrayState::Idle,
        }
    }
}

/// Shared tray state accessible from commands.
pub type TrayRecordingState = Arc<AtomicU8>;

/// Generate a 32x32 RGBA icon for the given tray state.
fn generate_icon(state: TrayState) -> Vec<u8> {
    let size: usize = 32;
    let mut pixels = vec![0u8; size * size * 4];

    match state {
        TrayState::Idle => {
            // Mic outline: draw a simple mic shape in white/gray
            for y in 0..size {
                for x in 0..size {
                    let idx = (y * size + x) * 4;
                    let cx = x as f32 - 15.5;
                    let cy = y as f32 - 15.5;

                    // Mic body: ellipse in upper portion
                    let in_mic_body = {
                        let ey = (cy + 4.0) / 8.0;
                        let ex = cx / 5.0;
                        ex * ex + ey * ey <= 1.0 && cy < 4.0
                    };

                    // Mic stand: vertical line below body
                    let in_stand = cx.abs() < 1.5 && cy >= 4.0 && cy < 10.0;

                    // Mic base: horizontal line at bottom
                    let in_base = cx.abs() < 5.0 && cy >= 10.0 && cy < 12.0;

                    // Cup around mic (U shape)
                    let dist_from_center = (cx * cx + (cy - 2.0) * (cy - 2.0)).sqrt();
                    let in_cup =
                        dist_from_center >= 7.0 && dist_from_center <= 9.0 && cy > 0.0 && cy < 8.0;

                    if in_mic_body || in_stand || in_base || in_cup {
                        pixels[idx] = 200; // R
                        pixels[idx + 1] = 200; // G
                        pixels[idx + 2] = 200; // B
                        pixels[idx + 3] = 255; // A
                    }
                }
            }
        }
        TrayState::Recording => {
            // Red filled circle (recording indicator)
            for y in 0..size {
                for x in 0..size {
                    let idx = (y * size + x) * 4;
                    let cx = x as f32 - 15.5;
                    let cy = y as f32 - 15.5;
                    let dist = (cx * cx + cy * cy).sqrt();

                    if dist <= 12.0 {
                        pixels[idx] = 220; // R
                        pixels[idx + 1] = 40; // G
                        pixels[idx + 2] = 40; // B
                        pixels[idx + 3] = 255; // A
                    }
                }
            }
        }
        TrayState::Processing => {
            // Orange/amber pulsing circle (processing indicator)
            for y in 0..size {
                for x in 0..size {
                    let idx = (y * size + x) * 4;
                    let cx = x as f32 - 15.5;
                    let cy = y as f32 - 15.5;
                    let dist = (cx * cx + cy * cy).sqrt();

                    if dist <= 12.0 && dist >= 6.0 {
                        pixels[idx] = 255; // R
                        pixels[idx + 1] = 165; // G
                        pixels[idx + 2] = 0; // B
                        pixels[idx + 3] = 255; // A
                    }
                }
            }
        }
    }

    pixels
}

/// Create an Image from a tray state.
fn icon_for_state(state: TrayState) -> Image<'static> {
    let pixels = generate_icon(state);
    Image::new_owned(pixels, 32, 32)
}

/// Set up the system tray icon with menu and event handlers.
pub fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let toggle_item = MenuItem::with_id(app, "toggle", "Start Dictation", true, None::<&str>)?;
    let open_item = MenuItem::with_id(app, "open", "Open Window", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&toggle_item, &open_item, &settings_item, &quit_item])?;

    let app_handle = app.clone();
    let app_handle2 = app.clone();

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon_for_state(TrayState::Idle))
        .tooltip("Outspoken - Idle")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "toggle" => {
                let _ = toggle_recording(&app_handle);
            }
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                let _ = toggle_recording(tray.app_handle());
            }
            TrayIconEvent::DoubleClick { .. } => {
                if let Some(window) = app_handle2.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Toggle recording from the tray. Emits an event to the frontend.
fn toggle_recording<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let state = app.state::<TrayRecordingState>();
    let current = TrayState::from(state.load(Ordering::Relaxed));

    match current {
        TrayState::Idle => {
            // Tell the frontend to start recording
            app.emit("tray-toggle-recording", "start")?;
        }
        TrayState::Recording => {
            // Tell the frontend to stop recording
            app.emit("tray-toggle-recording", "stop")?;
        }
        TrayState::Processing => {
            // Don't interrupt processing
        }
    }
    Ok(())
}

/// Update the tray icon and tooltip to reflect the current state.
/// Called from Tauri commands when recording state changes.
pub fn update_tray_state<R: Runtime>(app: &AppHandle<R>, new_state: TrayState) {
    let state = app.state::<TrayRecordingState>();
    state.store(new_state as u8, Ordering::Relaxed);

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon_for_state(new_state)));
        let tooltip = match new_state {
            TrayState::Idle => "Outspoken - Idle",
            TrayState::Recording => "Outspoken - Recording",
            TrayState::Processing => "Outspoken - Processing",
        };
        let _ = tray.set_tooltip(Some(tooltip));

        // Update the toggle menu text
        if let Some(window) = app.get_webview_window("main") {
            let _ = app.emit_to(window.label(), "tray-state-changed", new_state as u8);
        }
    }

    // Update the menu toggle text
    update_toggle_menu_text(app, new_state);
}

fn update_toggle_menu_text<R: Runtime>(app: &AppHandle<R>, state: TrayState) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let toggle_text = match state {
            TrayState::Idle => "Start Dictation",
            TrayState::Recording => "Stop Dictation",
            TrayState::Processing => "Processing...",
        };
        // Rebuild menu with updated text
        if let Ok(toggle_item) = MenuItem::with_id(
            app,
            "toggle",
            toggle_text,
            state != TrayState::Processing,
            None::<&str>,
        ) {
            if let Ok(open_item) = MenuItem::with_id(app, "open", "Open Window", true, None::<&str>)
            {
                if let Ok(settings_item) =
                    MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)
                {
                    if let Ok(quit_item) =
                        MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
                    {
                        if let Ok(menu) = Menu::with_items(
                            app,
                            &[&toggle_item, &open_item, &settings_item, &quit_item],
                        ) {
                            let _ = tray.set_menu(Some(menu));
                        }
                    }
                }
            }
        }
    }
}
