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
