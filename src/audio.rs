use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, StreamConfig};
use rubato::{SincFixedIn, SincInterpolationParameters, SincInterpolationType, Resampler, WindowFunction};
use serde::{Deserialize, Serialize};

/// Callback type for receiving processed mono samples (used for audio level monitoring).
pub type SampleCallback = Arc<dyn Fn(&[f32]) + Send + Sync>;

const TARGET_SAMPLE_RATE: u32 = 16_000;
const TARGET_CHANNELS: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

pub fn list_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let host = cpal::default_host();
    let default_device_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());

    let devices = host
        .input_devices()
        .map_err(|e| format!("Failed to enumerate input devices: {e}"))?;

    let mut result = Vec::new();
    for device in devices {
        if let Ok(name) = device.name() {
            let is_default = default_device_name.as_deref() == Some(&name);
            result.push(AudioDeviceInfo { name, is_default });
        }
    }

    Ok(result)
}

fn find_device(device_name: &Option<String>) -> Result<Device, String> {
    let host = cpal::default_host();

    match device_name {
        Some(name) => {
            let devices = host
                .input_devices()
                .map_err(|e| format!("Failed to enumerate devices: {e}"))?;
            for device in devices {
                if device.name().ok().as_deref() == Some(name.as_str()) {
                    return Ok(device);
                }
            }
            Err(format!("Audio device not found: {name}"))
        }
        None => host
            .default_input_device()
            .ok_or_else(|| "No microphone found. Connect a microphone and try again. On Linux, ensure PulseAudio/PipeWire is running.".to_string()),
    }
}

/// Shared state for an active recording session.
pub struct RecordingState {
    pub buffer: Arc<Mutex<Vec<f32>>>,
    pub is_recording: Arc<AtomicBool>,
    // Keep the stream alive; dropping it stops capture.
    _stream: cpal::Stream,
}

// SAFETY: RecordingState is only accessed behind a std::sync::Mutex which
// guarantees exclusive access. cpal::Stream itself is thread-safe in practice
// (it manages its own audio thread internally), but does not implement Send
// because it holds a raw pointer internally. We ensure the stream is never
// moved across threads without synchronisation.
unsafe impl Send for RecordingState {}

/// Start capturing audio from the specified device (or default).
/// Returns a `RecordingState` that accumulates 16kHz mono f32 samples.
/// If `sample_callback` is provided, it will be called with each chunk of
/// processed mono samples (before resampling) for audio level monitoring.
pub fn start_capture(
    device_name: &Option<String>,
    sample_callback: Option<SampleCallback>,
) -> Result<RecordingState, String> {
    let device = find_device(device_name)?;
    let supported_config = device
        .default_input_config()
        .map_err(|e| format!("Failed to get default input config: {e}"))?;

    let sample_rate = supported_config.sample_rate().0;
    let channels = supported_config.channels();
    let sample_format = supported_config.sample_format();

    let config: StreamConfig = supported_config.into();

    let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let is_recording = Arc::new(AtomicBool::new(true));

    let buf_clone = Arc::clone(&buffer);
    let recording_flag = Arc::clone(&is_recording);

    // Build a resampler if the device sample rate differs from 16kHz.
    let needs_resample = sample_rate != TARGET_SAMPLE_RATE;
    let resampler: Option<Mutex<SincFixedIn<f32>>> = if needs_resample {
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: WindowFunction::BlackmanHarris2,
        };
        let resampler = SincFixedIn::new(
            TARGET_SAMPLE_RATE as f64 / sample_rate as f64,
            2.0,
            params,
            1024,
            1,
        )
        .map_err(|e| format!("Failed to create resampler: {e}"))?;
        Some(Mutex::new(resampler))
    } else {
        None
    };

    let resampler = Arc::new(resampler);
    let sample_cb = sample_callback.clone();

    let data_callback = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        if !recording_flag.load(Ordering::Relaxed) {
            return;
        }

        // Convert to mono by averaging channels.
        let mono: Vec<f32> = if channels > 1 {
            data.chunks(channels as usize)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                .collect()
        } else {
            data.to_vec()
        };

        // Notify level monitor with raw mono samples.
        if let Some(ref cb) = sample_cb {
            cb(&mono);
        }

        let samples = if let Some(ref resampler_mutex) = *resampler {
            if let Ok(mut resampler) = resampler_mutex.lock() {
                // Process in chunks matching resampler's expected input size.
                let chunk_size = resampler.input_frames_max();
                let mut resampled_all = Vec::new();

                for chunk in mono.chunks(chunk_size) {
                    if chunk.len() < resampler.input_frames_next() {
                        // Pad short final chunk.
                        let mut padded = chunk.to_vec();
                        padded.resize(resampler.input_frames_next(), 0.0);
                        if let Ok(output) = resampler.process(&[&padded], None) {
                            if let Some(channel) = output.first() {
                                resampled_all.extend_from_slice(channel);
                            }
                        }
                    } else if let Ok(output) = resampler.process(&[chunk], None) {
                        if let Some(channel) = output.first() {
                            resampled_all.extend_from_slice(channel);
                        }
                    }
                }
                resampled_all
            } else {
                mono
            }
        } else {
            mono
        };

        if let Ok(mut buf) = buf_clone.lock() {
            buf.extend_from_slice(&samples);
        }
    };

    let err_callback = |err: cpal::StreamError| {
        eprintln!("Audio stream error: {err}");
    };

    let stream = match sample_format {
        SampleFormat::F32 => device
            .build_input_stream(&config, data_callback, err_callback, None)
            .map_err(|e| format!("Microphone access failed: {e}. On macOS, check System Settings → Privacy → Microphone. On Linux, check `arecord -l`."))?,
        SampleFormat::I16 => {
            let buf_clone = Arc::clone(&buffer);
            let recording_flag = Arc::clone(&is_recording);
            let sample_cb_i16 = sample_callback;
            let needs_resample = sample_rate != TARGET_SAMPLE_RATE;
            let resampler_i16: Option<Mutex<SincFixedIn<f32>>> = if needs_resample {
                let params = SincInterpolationParameters {
                    sinc_len: 256,
                    f_cutoff: 0.95,
                    interpolation: SincInterpolationType::Linear,
                    oversampling_factor: 256,
                    window: WindowFunction::BlackmanHarris2,
                };
                SincFixedIn::new(
                    TARGET_SAMPLE_RATE as f64 / sample_rate as f64,
                    2.0,
                    params,
                    1024,
                    1,
                )
                .ok()
                .map(Mutex::new)
            } else {
                None
            };
            let resampler_i16 = Arc::new(resampler_i16);

            device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if !recording_flag.load(Ordering::Relaxed) {
                            return;
                        }
                        let float_data: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();

                        let mono: Vec<f32> = if channels > 1 {
                            float_data
                                .chunks(channels as usize)
                                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                                .collect()
                        } else {
                            float_data
                        };

                        // Notify level monitor with raw mono samples.
                        if let Some(ref cb) = sample_cb_i16 {
                            cb(&mono);
                        }

                        let samples = if let Some(ref resampler_mutex) = *resampler_i16 {
                            if let Ok(mut resampler) = resampler_mutex.lock() {
                                let chunk_size = resampler.input_frames_max();
                                let mut resampled_all = Vec::new();
                                for chunk in mono.chunks(chunk_size) {
                                    if chunk.len() < resampler.input_frames_next() {
                                        let mut padded = chunk.to_vec();
                                        padded.resize(resampler.input_frames_next(), 0.0);
                                        if let Ok(output) = resampler.process(&[&padded], None) {
                                            if let Some(ch) = output.first() {
                                                resampled_all.extend_from_slice(ch);
                                            }
                                        }
                                    } else if let Ok(output) = resampler.process(&[chunk], None) {
                                        if let Some(ch) = output.first() {
                                            resampled_all.extend_from_slice(ch);
                                        }
                                    }
                                }
                                resampled_all
                            } else {
                                mono
                            }
                        } else {
                            mono
                        };

                        if let Ok(mut buf) = buf_clone.lock() {
                            buf.extend_from_slice(&samples);
                        }
                    },
                    err_callback,
                    None,
                )
                .map_err(|e| format!("Microphone access failed: {e}. On macOS, check System Settings → Privacy → Microphone. On Linux, check `arecord -l`."))?
        }
        format => return Err(format!("Unsupported sample format: {format:?}")),
    };

    stream
        .play()
        .map_err(|e| format!("Failed to start audio stream: {e}"))?;

    Ok(RecordingState {
        buffer,
        is_recording,
        _stream: stream,
    })
}

/// Trait abstracting audio capture for the daemon.
/// Implementations provide 16kHz mono Float32 PCM audio.
pub trait AudioCapture: Send {
    fn start(&mut self) -> Result<(), String>;
    fn stop(&mut self) -> Result<Vec<f32>, String>;
    fn is_recording(&self) -> bool;
}

/// Real audio capture using cpal, wrapping the existing `start_capture` function.
pub struct CpalAudioCapture {
    device_name: Option<String>,
    state: Option<RecordingState>,
}

impl CpalAudioCapture {
    pub fn new(device_name: Option<String>) -> Self {
        Self {
            device_name,
            state: None,
        }
    }
}

impl AudioCapture for CpalAudioCapture {
    fn start(&mut self) -> Result<(), String> {
        if self.state.is_some() {
            return Err("Already recording".into());
        }
        let recording = start_capture(&self.device_name, None)?;
        self.state = Some(recording);
        Ok(())
    }

    fn stop(&mut self) -> Result<Vec<f32>, String> {
        let recording = self.state.take().ok_or("Not currently recording")?;
        recording.is_recording.store(false, Ordering::Relaxed);
        let buffer = recording
            .buffer
            .lock()
            .map_err(|_| "Failed to lock audio buffer".to_string())?
            .clone();
        Ok(buffer)
    }

    fn is_recording(&self) -> bool {
        self.state
            .as_ref()
            .map(|s| s.is_recording.load(Ordering::Relaxed))
            .unwrap_or(false)
    }
}

/// Mock audio capture that returns a pre-filled sine wave buffer.
/// Useful for testing the full pipeline on Linux or in CI without a real microphone.
pub struct MockAudioCapture {
    recording: bool,
    duration_secs: f32,
    frequency_hz: f32,
}

impl MockAudioCapture {
    pub fn new(duration_secs: f32, frequency_hz: f32) -> Self {
        Self {
            recording: false,
            duration_secs,
            frequency_hz,
        }
    }

    fn generate_sine_wave(&self) -> Vec<f32> {
        let num_samples = (TARGET_SAMPLE_RATE as f32 * self.duration_secs) as usize;
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / TARGET_SAMPLE_RATE as f32;
                (2.0 * std::f32::consts::PI * self.frequency_hz * t).sin()
            })
            .collect()
    }
}

impl AudioCapture for MockAudioCapture {
    fn start(&mut self) -> Result<(), String> {
        if self.recording {
            return Err("Already recording".into());
        }
        self.recording = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<Vec<f32>, String> {
        if !self.recording {
            return Err("Not currently recording".into());
        }
        self.recording = false;
        Ok(self.generate_sine_wave())
    }

    fn is_recording(&self) -> bool {
        self.recording
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_capture_returns_valid_buffer() {
        let mut capture = MockAudioCapture::new(1.0, 440.0);
        assert!(!capture.is_recording());

        capture.start().unwrap();
        assert!(capture.is_recording());

        let buffer = capture.stop().unwrap();
        assert!(!capture.is_recording());

        // 1 second at 16kHz = 16000 samples
        assert_eq!(buffer.len(), 16000);
    }

    #[test]
    fn mock_capture_buffer_format() {
        let mut capture = MockAudioCapture::new(0.5, 440.0);
        capture.start().unwrap();
        let buffer = capture.stop().unwrap();

        // 0.5 seconds at 16kHz = 8000 samples
        assert_eq!(buffer.len(), 8000);

        // All samples should be in [-1.0, 1.0] range (valid Float32 PCM)
        for &sample in &buffer {
            assert!(sample >= -1.0 && sample <= 1.0, "Sample {sample} out of range");
        }
    }

    #[test]
    fn mock_capture_sine_wave_is_not_silence() {
        let mut capture = MockAudioCapture::new(0.1, 440.0);
        capture.start().unwrap();
        let buffer = capture.stop().unwrap();

        // Should have non-zero samples (not silence)
        let rms: f32 = (buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len() as f32).sqrt();
        assert!(rms > 0.3, "RMS {rms} too low — expected audible sine wave");
    }

    #[test]
    fn mock_capture_errors_on_double_start() {
        let mut capture = MockAudioCapture::new(1.0, 440.0);
        capture.start().unwrap();
        assert!(capture.start().is_err());
    }

    #[test]
    fn mock_capture_errors_on_stop_without_start() {
        let mut capture = MockAudioCapture::new(1.0, 440.0);
        assert!(capture.stop().is_err());
    }
}
