use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use crate::platform::{AudioCapture, IndicatorState, StatusIndicator, TextInjector};
use crate::transcription::{TranscriptionConfig, TranscriptionService};
use crate::text_processing;
use crate::vad::VadSegmenter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonState {
    Idle,
    Recording,
    Processing,
}

/// The main daemon that ties hotkey → audio → transcription → text injection together.
pub struct Daemon {
    audio: Box<dyn AudioCapture>,
    transcriber: TranscriptionService,
    injector: Box<dyn TextInjector>,
    indicator: Box<dyn StatusIndicator>,
    hotkey_rx: mpsc::Receiver<()>,
    shutdown: Arc<AtomicBool>,
}

impl Daemon {
    pub fn new(
        audio: Box<dyn AudioCapture>,
        transcriber: TranscriptionService,
        injector: Box<dyn TextInjector>,
        indicator: Box<dyn StatusIndicator>,
        hotkey_rx: mpsc::Receiver<()>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            audio,
            transcriber,
            injector,
            indicator,
            hotkey_rx,
            shutdown,
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        let mut state = DaemonState::Idle;
        self.indicator.set_state(IndicatorState::Idle).ok();

        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                break;
            }

            // Poll for hotkey with timeout so we can check shutdown
            loop {
                if self.shutdown.load(Ordering::SeqCst) {
                    return Ok(());
                }
                match self.hotkey_rx.recv_timeout(std::time::Duration::from_millis(200)) {
                    Ok(()) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                }
            }

            match state {
                DaemonState::Idle => {
                    if let Err(e) = self.audio.start_recording() {
                        eprintln!("Failed to start recording: {e}");
                        continue;
                    }
                    state = DaemonState::Recording;
                    self.indicator.set_state(IndicatorState::Recording).ok();
                    eprintln!("Recording...");
                }
                DaemonState::Recording => {
                    state = DaemonState::Processing;
                    self.indicator.set_state(IndicatorState::Processing).ok();
                    eprintln!("Transcribing...");

                    let audio_data = match self.audio.stop_recording() {
                        Ok(data) => data,
                        Err(e) => {
                            eprintln!("Failed to stop recording: {e}");
                            state = DaemonState::Idle;
                            self.indicator.set_state(IndicatorState::Idle).ok();
                            continue;
                        }
                    };

                    if audio_data.is_empty() {
                        eprintln!("No audio captured.");
                        state = DaemonState::Idle;
                        self.indicator.set_state(IndicatorState::Idle).ok();
                        continue;
                    }

                    // Use VAD to strip silence (prevents Whisper hallucinations
                    // like "Thank you" on quiet segments)
                    let result = match VadSegmenter::new() {
                        Ok(mut vad) => {
                            match self.transcriber.transcribe_with_vad(&audio_data, &mut vad) {
                                Ok(r) if r.text.trim().is_empty() => {
                                    // VAD found no speech — fall back to direct transcription
                                    // in case VAD is too aggressive for this mic
                                    eprintln!("VAD found no speech, trying direct transcription...");
                                    self.transcriber.transcribe(&audio_data)
                                }
                                other => other,
                            }
                        }
                        Err(_) => self.transcriber.transcribe(&audio_data),
                    };

                    match result {
                        Ok(mut tr) => {
                            let dictionary = text_processing::list_entries().unwrap_or_default();
                            tr.text = text_processing::process_text(&tr.text, true, true, &dictionary);

                            if !tr.text.is_empty() {
                                eprintln!("Injecting: {}", tr.text);
                                if let Err(e) = self.injector.inject_text(&tr.text) {
                                    eprintln!("Failed to inject text: {e}");
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Transcription error: {e}");
                        }
                    }

                    state = DaemonState::Idle;
                    self.indicator.set_state(IndicatorState::Idle).ok();
                }
                DaemonState::Processing => {
                    // Ignore hotkey during processing
                }
            }
        }

        Ok(())
    }
}

// --- Mock implementations for testing ---

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
    fn start_recording(&mut self) -> crate::platform::Result<()> {
        Ok(())
    }
    fn stop_recording(&mut self) -> crate::platform::Result<Vec<f32>> {
        let data = self.audio_data.lock().map_err(|e| format!("Lock error: {e}"))?;
        Ok(data.clone())
    }
}

pub struct MockTextInjector {
    pub injected: Arc<Mutex<Vec<String>>>,
}

impl MockTextInjector {
    pub fn new() -> Self {
        Self {
            injected: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl TextInjector for MockTextInjector {
    fn inject_text(&self, text: &str) -> crate::platform::Result<()> {
        let mut v = self.injected.lock().map_err(|e| format!("Lock error: {e}"))?;
        v.push(text.to_string());
        Ok(())
    }
}

pub struct MockStatusIndicator;

impl StatusIndicator for MockStatusIndicator {
    fn set_state(&mut self, _state: IndicatorState) -> crate::platform::Result<()> {
        Ok(())
    }
}
