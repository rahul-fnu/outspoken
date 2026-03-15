use silero_vad_rust::load_silero_vad;
use silero_vad_rust::silero_vad::model::OnnxModel;
use silero_vad_rust::silero_vad::utils_vad::{VadParameters, get_speech_timestamps};

/// A segment of detected speech audio.
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    pub start_sample: usize,
    pub end_sample: usize,
    pub audio: Vec<f32>,
}

/// Wraps the Silero VAD model to segment audio into speech regions.
pub struct VadSegmenter {
    model: OnnxModel,
    params: VadParameters,
}

impl VadSegmenter {
    /// Create a new VadSegmenter using the bundled Silero VAD model.
    pub fn new() -> Result<Self, String> {
        let model = load_silero_vad()
            .map_err(|e| format!("Failed to load Silero VAD model: {e}"))?;

        let params = VadParameters {
            threshold: 0.5,
            min_speech_duration_ms: 250,
            min_silence_duration_ms: 100,
            speech_pad_ms: 300,
            return_seconds: false,
            ..Default::default()
        };

        Ok(Self { model, params })
    }

    /// Set the speech detection threshold (0.0 - 1.0, default 0.5).
    pub fn set_threshold(&mut self, threshold: f32) {
        self.params.threshold = threshold;
    }

    /// Segment audio into speech regions.
    /// Input: 16kHz mono f32 PCM samples.
    /// Returns speech segments with audio data and sample positions.
    pub fn segment(&mut self, audio: &[f32]) -> Result<Vec<SpeechSegment>, String> {
        self.model.reset_states();

        let timestamps = get_speech_timestamps(audio, &mut self.model, &self.params)
            .map_err(|e| format!("VAD processing failed: {e}"))?;

        let segments = timestamps
            .iter()
            .map(|ts| {
                let start = ts.start as usize;
                let end = (ts.end as usize).min(audio.len());
                SpeechSegment {
                    start_sample: start,
                    end_sample: end,
                    audio: audio[start..end].to_vec(),
                }
            })
            .collect();

        Ok(segments)
    }
}
