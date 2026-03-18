use super::{AudioCapture, HotkeyListener, IndicatorState, Result, StatusIndicator, TextInjector};

pub struct LinuxHotkeyListener;

impl HotkeyListener for LinuxHotkeyListener {
    fn start(&mut self, _callback: Box<dyn Fn()>) -> Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}

pub struct LinuxAudioCapture;

impl AudioCapture for LinuxAudioCapture {
    fn start_recording(&mut self) -> Result<()> {
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<Vec<f32>> {
        Ok(Vec::new())
    }
}

pub struct LinuxTextInjector;

impl TextInjector for LinuxTextInjector {
    fn inject_text(&self, _text: &str) -> Result<()> {
        Ok(())
    }
}

pub struct LinuxStatusIndicator;

impl StatusIndicator for LinuxStatusIndicator {
    fn set_state(&mut self, _state: IndicatorState) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_listener_start_stop() {
        let mut listener = LinuxHotkeyListener;
        assert!(listener.start(Box::new(|| {})).is_ok());
        assert!(listener.stop().is_ok());
    }

    #[test]
    fn audio_capture_returns_empty_buffer() {
        let mut capture = LinuxAudioCapture;
        assert!(capture.start_recording().is_ok());
        let samples = capture.stop_recording().unwrap();
        assert!(samples.is_empty());
    }

    #[test]
    fn text_injector_succeeds() {
        let injector = LinuxTextInjector;
        assert!(injector.inject_text("hello world").is_ok());
    }

    #[test]
    fn status_indicator_all_states() {
        let mut indicator = LinuxStatusIndicator;
        assert!(indicator.set_state(IndicatorState::Idle).is_ok());
        assert!(indicator.set_state(IndicatorState::Recording).is_ok());
        assert!(indicator.set_state(IndicatorState::Processing).is_ok());
    }

    #[test]
    fn indicator_state_equality() {
        assert_eq!(IndicatorState::Idle, IndicatorState::Idle);
        assert_ne!(IndicatorState::Idle, IndicatorState::Recording);
        assert_ne!(IndicatorState::Recording, IndicatorState::Processing);
    }
}
