use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct TranscriptionService {
    ctx: WhisperContext,
}

impl TranscriptionService {
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().ok_or("Invalid model path")?,
            params,
        )
        .map_err(|e| format!("Failed to load whisper model: {e}"))?;

        Ok(Self { ctx })
    }

    /// Transcribe PCM audio samples (16kHz, mono, f32).
    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, String> {
        let mut state = self.ctx.create_state().map_err(|e| format!("Failed to create state: {e}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, audio_data)
            .map_err(|e| format!("Transcription failed: {e}"))?;

        let num_segments = state.full_n_segments().map_err(|e| format!("Failed to get segments: {e}"))?;
        let mut result = String::new();
        for i in 0..num_segments {
            if let Ok(segment) = state.full_get_segment_text(i) {
                result.push_str(&segment);
            }
        }

        Ok(result.trim().to_string())
    }
}
