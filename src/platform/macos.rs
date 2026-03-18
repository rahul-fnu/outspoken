use super::{AudioCapture, IndicatorState, Result, StatusIndicator, TextInjector};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// --- Audio Capture (wraps cpal-based capture from audio.rs) ---

pub struct MacAudioCapture {
    recording: Mutex<Option<crate::audio::RecordingState>>,
}

impl MacAudioCapture {
    pub fn new() -> Self {
        Self {
            recording: Mutex::new(None),
        }
    }
}

impl AudioCapture for MacAudioCapture {
    fn start_recording(&mut self) -> Result<()> {
        let rec = crate::audio::start_capture(&None, None)
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        *self.recording.lock().unwrap() = Some(rec);
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<Vec<f32>> {
        let rec = self.recording.lock().unwrap().take();
        match rec {
            Some(r) => {
                r.is_recording.store(false, Ordering::Relaxed);
                let buffer = r.buffer.lock()
                    .map_err(|e| -> Box<dyn std::error::Error> { format!("Lock error: {e}").into() })?
                    .clone();
                Ok(buffer)
            }
            None => Err("Not recording".into()),
        }
    }
}

// --- Text Injector (CGEvent on macOS, types into focused app) ---

#[cfg(target_os = "macos")]
pub struct MacTextInjector;

#[cfg(target_os = "macos")]
impl MacTextInjector {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
impl TextInjector for MacTextInjector {
    fn inject_text(&self, text: &str) -> Result<()> {
        use arboard::Clipboard;
        use core_graphics::event::{CGEvent, CGEventFlags};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

        let mut clipboard = Clipboard::new()
            .map_err(|e| -> Box<dyn std::error::Error> { format!("Clipboard error: {e}").into() })?;

        clipboard.set_text(text.to_string())
            .map_err(|e| -> Box<dyn std::error::Error> { format!("Clipboard error: {e}").into() })?;

        // Small delay to ensure clipboard is ready
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Simulate Cmd+V paste
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| -> Box<dyn std::error::Error> { "Failed to create event source".into() })?;

        // keycode 9 = V
        let key_down = CGEvent::new_keyboard_event(source.clone(), 9, true)
            .map_err(|_| -> Box<dyn std::error::Error> { "Failed to create key event".into() })?;
        key_down.set_flags(CGEventFlags::CGEventFlagCommand);
        key_down.post(core_graphics::event::CGEventTapLocation::HID);

        let key_up = CGEvent::new_keyboard_event(source.clone(), 9, false)
            .map_err(|_| -> Box<dyn std::error::Error> { "Failed to create key event".into() })?;
        key_up.set_flags(CGEventFlags::CGEventFlagCommand);
        key_up.post(core_graphics::event::CGEventTapLocation::HID);

        Ok(())
    }
}

// Stub for non-macOS (uses Linux impl from linux.rs instead)
#[cfg(not(target_os = "macos"))]
pub struct MacTextInjector;

#[cfg(not(target_os = "macos"))]
impl MacTextInjector {
    pub fn new() -> Self { Self }
}

#[cfg(not(target_os = "macos"))]
impl TextInjector for MacTextInjector {
    fn inject_text(&self, _text: &str) -> Result<()> { Ok(()) }
}

// --- Status Indicator (prints to stderr for now, menu bar later) ---

pub struct MacStatusIndicator;

impl MacStatusIndicator {
    pub fn new() -> Self {
        Self
    }
}

impl StatusIndicator for MacStatusIndicator {
    fn set_state(&mut self, state: IndicatorState) -> Result<()> {
        match state {
            IndicatorState::Idle => eprintln!("● Idle"),
            IndicatorState::Recording => eprintln!("🔴 Recording"),
            IndicatorState::Processing => eprintln!("🟡 Processing"),
        }
        Ok(())
    }
}
