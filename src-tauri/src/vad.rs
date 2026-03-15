use silero_vad_rust::silero_vad::model::OnnxModel;
use silero_vad_rust::silero_vad::utils_vad::VadParameters;
use silero_vad_rust::{get_speech_timestamps, load_silero_vad};

/// A speech segment detected by VAD, expressed in sample indices.
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    /// Start sample index (inclusive).
    pub start: usize,
    /// End sample index (exclusive).
    pub end: usize,
}

/// Wrapper around Silero VAD that detects speech segments in audio.
pub struct VadSegmenter {
    model: OnnxModel,
}

impl VadSegmenter {
    /// Load the Silero VAD model (bundled ONNX, CPU).
    pub fn new() -> Result<Self, String> {
        let model = load_silero_vad().map_err(|e| format!("Failed to load VAD model: {e}"))?;
        Ok(Self { model })
    }

    /// Detect speech segments in 16 kHz mono f32 audio.
    /// Returns segments as sample-index ranges.
    pub fn segment(&mut self, audio: &[f32]) -> Result<Vec<SpeechSegment>, String> {
        let params = VadParameters {
            return_seconds: false,
            min_speech_duration_ms: 250,
            min_silence_duration_ms: 100,
            threshold: 0.5,
            ..Default::default()
        };

        let timestamps = get_speech_timestamps(audio, &mut self.model, &params)
            .map_err(|e| format!("VAD segmentation failed: {e}"))?;

        // Reset model state for next call
        self.model.reset_states();

        Ok(timestamps
            .into_iter()
            .map(|ts| SpeechSegment {
                start: ts.start as usize,
                end: ts.end as usize,
            })
            .collect())
    }
}
