#[cfg(target_os = "macos")]
use std::thread;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
const KEYSTROKE_DELAY: Duration = Duration::from_millis(2);

pub trait TextInjector {
    fn inject_text(&mut self, text: &str) -> Result<(), String>;
}

#[cfg(target_os = "macos")]
pub struct MacOSTextInjector;

#[cfg(target_os = "macos")]
impl MacOSTextInjector {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
impl TextInjector for MacOSTextInjector {
    fn inject_text(&mut self, text: &str) -> Result<(), String> {
        use core_graphics::event::{CGEvent, CGEventTapLocation};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| "Failed to create CGEventSource".to_string())?;

        for ch in text.chars() {
            let mut utf16_buf = [0u16; 2];
            let unicode = ch.encode_utf16(&mut utf16_buf);

            let key_down = CGEvent::new_keyboard_event(source.clone(), 0, true)
                .map_err(|_| format!("Failed to create key down event for '{ch}'"))?;
            key_down.set_string_from_utf16_unchecked(unicode);

            let key_up = CGEvent::new_keyboard_event(source.clone(), 0, false)
                .map_err(|_| format!("Failed to create key up event for '{ch}'"))?;
            key_up.set_string_from_utf16_unchecked(unicode);

            key_down.post(CGEventTapLocation::HID);
            key_up.post(CGEventTapLocation::HID);

            thread::sleep(KEYSTROKE_DELAY);
        }

        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub struct MockTextInjector {
    pub injected: Vec<String>,
}

#[cfg(not(target_os = "macos"))]
impl MockTextInjector {
    pub fn new() -> Self {
        Self {
            injected: Vec::new(),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl TextInjector for MockTextInjector {
    fn inject_text(&mut self, text: &str) -> Result<(), String> {
        self.injected.push(text.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn mock_captures_injected_text() {
        let mut injector = MockTextInjector::new();
        injector.inject_text("hello world").unwrap();
        assert_eq!(injector.injected, vec!["hello world".to_string()]);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn mock_captures_multiple_injections() {
        let mut injector = MockTextInjector::new();
        injector.inject_text("first").unwrap();
        injector.inject_text("second").unwrap();
        injector.inject_text("third").unwrap();
        assert_eq!(injector.injected, vec!["first", "second", "third"]);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn mock_captures_unicode_text() {
        let mut injector = MockTextInjector::new();
        injector.inject_text("こんにちは 🌍 café").unwrap();
        assert_eq!(injector.injected, vec!["こんにちは 🌍 café"]);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn mock_starts_empty() {
        let injector = MockTextInjector::new();
        assert!(injector.injected.is_empty());
    }
}
