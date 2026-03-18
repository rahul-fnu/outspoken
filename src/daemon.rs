use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonState {
    Idle,
    Recording,
    Processing,
}

// --- Platform traits ---

pub trait HotkeyListener: Send {
    fn wait_for_hotkey(&self) -> Result<(), String>;
}

pub trait AudioCapture: Send {
    fn start(&self) -> Result<(), String>;
    fn stop(&self) -> Result<Vec<f32>, String>;
}

pub trait Transcriber: Send {
    fn transcribe(&self, audio: &[f32]) -> Result<String, String>;
}

pub trait TextInjector: Send {
    fn inject(&self, text: &str) -> Result<(), String>;
}

pub trait StatusIndicator: Send {
    fn set_state(&self, state: DaemonState);
}

// --- Mock implementations (used on Linux / in tests) ---

pub struct MockHotkeyListener {
    receiver: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl MockHotkeyListener {
    pub fn new() -> (Self, std::sync::mpsc::Sender<()>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (
            Self {
                receiver: Mutex::new(rx),
            },
            tx,
        )
    }
}

impl HotkeyListener for MockHotkeyListener {
    fn wait_for_hotkey(&self) -> Result<(), String> {
        let rx = self.receiver.lock().map_err(|e| format!("Lock error: {e}"))?;
        rx.recv().map_err(|e| format!("Hotkey channel closed: {e}"))
    }
}

pub struct MockAudioCapture {
    audio_data: Mutex<Vec<f32>>,
}

impl MockAudioCapture {
    pub fn new(audio_data: Vec<f32>) -> Self {
        Self {
            audio_data: Mutex::new(audio_data),
        }
    }
}

impl AudioCapture for MockAudioCapture {
    fn start(&self) -> Result<(), String> {
        Ok(())
    }

    fn stop(&self) -> Result<Vec<f32>, String> {
        let data = self.audio_data.lock().map_err(|e| format!("Lock error: {e}"))?;
        Ok(data.clone())
    }
}

pub struct MockTranscriber {
    result: String,
}

impl MockTranscriber {
    pub fn new(result: &str) -> Self {
        Self {
            result: result.to_string(),
        }
    }
}

impl Transcriber for MockTranscriber {
    fn transcribe(&self, _audio: &[f32]) -> Result<String, String> {
        Ok(self.result.clone())
    }
}

pub struct MockTextInjector {
    injected: Arc<Mutex<Vec<String>>>,
}

impl MockTextInjector {
    pub fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
        let injected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                injected: injected.clone(),
            },
            injected,
        )
    }
}

impl TextInjector for MockTextInjector {
    fn inject(&self, text: &str) -> Result<(), String> {
        let mut v = self.injected.lock().map_err(|e| format!("Lock error: {e}"))?;
        v.push(text.to_string());
        Ok(())
    }
}

pub struct MockStatusIndicator {
    states: Arc<Mutex<Vec<DaemonState>>>,
}

impl MockStatusIndicator {
    pub fn new() -> (Self, Arc<Mutex<Vec<DaemonState>>>) {
        let states: Arc<Mutex<Vec<DaemonState>>> = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                states: states.clone(),
            },
            states,
        )
    }
}

impl StatusIndicator for MockStatusIndicator {
    fn set_state(&self, state: DaemonState) {
        if let Ok(mut v) = self.states.lock() {
            v.push(state);
        }
    }
}

pub struct FailingTranscriber;

impl Transcriber for FailingTranscriber {
    fn transcribe(&self, _audio: &[f32]) -> Result<String, String> {
        Err("Transcription failed: model error".to_string())
    }
}

// --- Daemon run loop ---

pub struct Daemon {
    hotkey: Box<dyn HotkeyListener>,
    audio: Box<dyn AudioCapture>,
    transcriber: Box<dyn Transcriber>,
    injector: Box<dyn TextInjector>,
    indicator: Box<dyn StatusIndicator>,
    shutdown: Arc<AtomicBool>,
}

impl Daemon {
    pub fn new(
        hotkey: Box<dyn HotkeyListener>,
        audio: Box<dyn AudioCapture>,
        transcriber: Box<dyn Transcriber>,
        injector: Box<dyn TextInjector>,
        indicator: Box<dyn StatusIndicator>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            hotkey,
            audio,
            transcriber,
            injector,
            indicator,
            shutdown,
        }
    }

    pub fn run(&self) -> Result<(), String> {
        let mut state = DaemonState::Idle;
        self.indicator.set_state(state);

        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                break;
            }

            // Wait for hotkey (blocks until pressed or channel closes)
            if self.hotkey.wait_for_hotkey().is_err() {
                // Channel closed — treat as shutdown
                break;
            }

            if self.shutdown.load(Ordering::SeqCst) {
                break;
            }

            match state {
                DaemonState::Idle => {
                    // Start recording
                    if let Err(e) = self.audio.start() {
                        eprintln!("Failed to start audio capture: {e}");
                        continue;
                    }
                    state = DaemonState::Recording;
                    self.indicator.set_state(state);
                }
                DaemonState::Recording => {
                    // Stop recording, process
                    state = DaemonState::Processing;
                    self.indicator.set_state(state);

                    let audio_data = match self.audio.stop() {
                        Ok(data) => data,
                        Err(e) => {
                            eprintln!("Failed to stop audio capture: {e}");
                            state = DaemonState::Idle;
                            self.indicator.set_state(state);
                            continue;
                        }
                    };

                    match self.transcriber.transcribe(&audio_data) {
                        Ok(text) => {
                            if !text.is_empty() {
                                if let Err(e) = self.injector.inject(&text) {
                                    eprintln!("Failed to inject text: {e}");
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Transcription error: {e}");
                        }
                    }

                    state = DaemonState::Idle;
                    self.indicator.set_state(state);
                }
                DaemonState::Processing => {
                    // Ignore hotkey during processing
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_transitions_full_pipeline() {
        let (hotkey_listener, hotkey_tx) = MockHotkeyListener::new();
        let audio_capture = MockAudioCapture::new(vec![0.1, 0.2, 0.3]);
        let transcriber = MockTranscriber::new("hello world");
        let (injector, injected_texts) = MockTextInjector::new();
        let (indicator, indicator_states) = MockStatusIndicator::new();
        let shutdown = Arc::new(AtomicBool::new(false));

        let daemon = Daemon::new(
            Box::new(hotkey_listener),
            Box::new(audio_capture),
            Box::new(transcriber),
            Box::new(injector),
            Box::new(indicator),
            shutdown.clone(),
        );

        let hotkey_tx_clone = hotkey_tx.clone();
        let shutdown_clone = shutdown.clone();
        let handle = std::thread::spawn(move || {
            daemon.run()
        });

        // First hotkey: Idle -> Recording
        hotkey_tx_clone.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Second hotkey: Recording -> Processing -> Idle
        hotkey_tx_clone.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Shutdown
        shutdown_clone.store(true, Ordering::SeqCst);
        drop(hotkey_tx_clone);
        drop(hotkey_tx);

        handle.join().unwrap().unwrap();

        // Verify injected text
        let texts = injected_texts.lock().unwrap();
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0], "hello world");

        // Verify state transitions: Idle, Recording, Processing, Idle
        let states = indicator_states.lock().unwrap();
        assert_eq!(
            *states,
            vec![
                DaemonState::Idle,
                DaemonState::Recording,
                DaemonState::Processing,
                DaemonState::Idle,
            ]
        );
    }

    #[test]
    fn test_transcription_error_returns_to_idle() {
        let (hotkey_listener, hotkey_tx) = MockHotkeyListener::new();
        let audio_capture = MockAudioCapture::new(vec![0.1, 0.2]);
        let transcriber = FailingTranscriber;
        let (injector, injected_texts) = MockTextInjector::new();
        let (indicator, indicator_states) = MockStatusIndicator::new();
        let shutdown = Arc::new(AtomicBool::new(false));

        let daemon = Daemon::new(
            Box::new(hotkey_listener),
            Box::new(audio_capture),
            Box::new(transcriber),
            Box::new(injector),
            Box::new(indicator),
            shutdown.clone(),
        );

        let shutdown_clone = shutdown.clone();
        let handle = std::thread::spawn(move || daemon.run());

        // Idle -> Recording
        hotkey_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Recording -> Processing -> error -> Idle
        hotkey_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Shutdown
        shutdown_clone.store(true, Ordering::SeqCst);
        drop(hotkey_tx);

        handle.join().unwrap().unwrap();

        // No text should have been injected
        let texts = injected_texts.lock().unwrap();
        assert!(texts.is_empty());

        // State should have gone: Idle, Recording, Processing, Idle
        let states = indicator_states.lock().unwrap();
        assert_eq!(
            *states,
            vec![
                DaemonState::Idle,
                DaemonState::Recording,
                DaemonState::Processing,
                DaemonState::Idle,
            ]
        );
    }

    #[test]
    fn test_multiple_cycles() {
        let (hotkey_listener, hotkey_tx) = MockHotkeyListener::new();
        let audio_capture = MockAudioCapture::new(vec![0.5]);
        let transcriber = MockTranscriber::new("test");
        let (injector, injected_texts) = MockTextInjector::new();
        let (indicator, indicator_states) = MockStatusIndicator::new();
        let shutdown = Arc::new(AtomicBool::new(false));

        let daemon = Daemon::new(
            Box::new(hotkey_listener),
            Box::new(audio_capture),
            Box::new(transcriber),
            Box::new(injector),
            Box::new(indicator),
            shutdown.clone(),
        );

        let shutdown_clone = shutdown.clone();
        let handle = std::thread::spawn(move || daemon.run());

        // Cycle 1
        hotkey_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        hotkey_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Cycle 2
        hotkey_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        hotkey_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        shutdown_clone.store(true, Ordering::SeqCst);
        drop(hotkey_tx);

        handle.join().unwrap().unwrap();

        let texts = injected_texts.lock().unwrap();
        assert_eq!(texts.len(), 2);
        assert_eq!(texts[0], "test");
        assert_eq!(texts[1], "test");

        let states = indicator_states.lock().unwrap();
        assert_eq!(
            *states,
            vec![
                DaemonState::Idle,
                DaemonState::Recording,
                DaemonState::Processing,
                DaemonState::Idle,
                DaemonState::Recording,
                DaemonState::Processing,
                DaemonState::Idle,
            ]
        );
    }

    #[test]
    fn test_graceful_shutdown_while_idle() {
        let (hotkey_listener, hotkey_tx) = MockHotkeyListener::new();
        let audio_capture = MockAudioCapture::new(vec![]);
        let transcriber = MockTranscriber::new("");
        let (injector, _injected_texts) = MockTextInjector::new();
        let (indicator, indicator_states) = MockStatusIndicator::new();
        let shutdown = Arc::new(AtomicBool::new(false));

        let daemon = Daemon::new(
            Box::new(hotkey_listener),
            Box::new(audio_capture),
            Box::new(transcriber),
            Box::new(injector),
            Box::new(indicator),
            shutdown.clone(),
        );

        let shutdown_clone = shutdown.clone();
        let handle = std::thread::spawn(move || daemon.run());

        // Immediately shutdown by closing channel
        shutdown_clone.store(true, Ordering::SeqCst);
        drop(hotkey_tx);

        handle.join().unwrap().unwrap();

        let states = indicator_states.lock().unwrap();
        assert_eq!(states[0], DaemonState::Idle);
    }
}
