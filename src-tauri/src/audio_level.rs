use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// Configuration for silence detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilenceConfig {
    /// Silence threshold in dB (e.g., -40.0). Levels below this are considered silence.
    pub threshold_db: f32,
    /// Duration in seconds of continuous silence before emitting `silence-detected`.
    pub duration_secs: f32,
}

impl Default for SilenceConfig {
    fn default() -> Self {
        Self {
            threshold_db: -40.0,
            duration_secs: 3.0,
        }
    }
}

/// Payload emitted for audio level events.
#[derive(Debug, Clone, Serialize)]
pub struct AudioLevelPayload {
    /// RMS level in dB (typically -60 to 0).
    pub level_db: f32,
    /// Raw RMS value (0.0 to 1.0).
    pub level_rms: f32,
}

/// Manages audio level monitoring and silence detection.
pub struct AudioLevelMonitor {
    app_handle: AppHandle,
    silence_config: SilenceConfig,
    silence_start: Option<Instant>,
    silence_emitted: bool,
    last_emit: Instant,
    emit_interval: Duration,
}

impl AudioLevelMonitor {
    pub fn new(app_handle: AppHandle, silence_config: SilenceConfig) -> Self {
        Self {
            app_handle,
            silence_config,
            silence_start: None,
            silence_emitted: false,
            last_emit: Instant::now() - Duration::from_millis(100),
            emit_interval: Duration::from_millis(50), // ~20fps
        }
    }

    /// Process a chunk of PCM f32 samples. Calculates RMS, emits level events
    /// at ~20fps, and detects silence.
    pub fn process_samples(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }

        let now = Instant::now();
        if now.duration_since(self.last_emit) < self.emit_interval {
            return;
        }
        self.last_emit = now;

        let rms = calculate_rms(samples);
        let db = rms_to_db(rms);

        // Emit audio level event.
        let _ = self.app_handle.emit(
            "audio-level",
            AudioLevelPayload {
                level_db: db,
                level_rms: rms,
            },
        );

        // Silence detection.
        if db < self.silence_config.threshold_db {
            match self.silence_start {
                None => {
                    self.silence_start = Some(now);
                }
                Some(start) => {
                    let silence_duration =
                        Duration::from_secs_f32(self.silence_config.duration_secs);
                    if !self.silence_emitted && now.duration_since(start) >= silence_duration {
                        let _ = self.app_handle.emit("silence-detected", ());
                        self.silence_emitted = true;
                    }
                }
            }
        } else {
            self.silence_start = None;
            self.silence_emitted = false;
        }
    }
}

/// Calculate Root Mean Square of PCM samples.
fn calculate_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Convert RMS value to decibels. Clamps to -60 dB minimum.
fn rms_to_db(rms: f32) -> f32 {
    if rms <= 0.0 {
        return -60.0;
    }
    let db = 20.0 * rms.log10();
    db.max(-60.0)
}
