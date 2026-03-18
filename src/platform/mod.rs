pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IndicatorState {
    Idle,
    Recording,
    Processing,
}

pub trait HotkeyListener {
    fn start(&mut self, callback: Box<dyn Fn()>) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
}

pub trait AudioCapture {
    fn start_recording(&mut self) -> Result<()>;
    fn stop_recording(&mut self) -> Result<Vec<f32>>;
}

pub trait TextInjector {
    fn inject_text(&self, text: &str) -> Result<()>;
}

pub trait StatusIndicator {
    fn set_state(&mut self, state: IndicatorState) -> Result<()>;
}

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;
