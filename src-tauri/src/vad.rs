use std::path::Path;

use ort::session::Session;
use ort::value::{Tensor, TensorRef};

const SAMPLE_RATE: i64 = 16000;
const CHUNK_SIZE: usize = 512; // 32ms at 16kHz
const STATE_DIM: usize = 128;

/// Silero VAD v5 wrapper using ONNX Runtime.
pub struct SileroVad {
    session: Session,
    /// Internal RNN state: shape (2, 1, 128)
    state: Vec<f32>,
    threshold: f32,
}

impl SileroVad {
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let session = Session::builder()
            .map_err(|e| format!("Failed to create session builder: {e}"))?
            .with_intra_threads(1)
            .map_err(|e| format!("Failed to set intra threads: {e}"))?
            .commit_from_file(model_path)
            .map_err(|e| format!("Failed to load VAD model: {e}"))?;

        Ok(Self {
            session,
            state: vec![0.0f32; 2 * 1 * STATE_DIM],
            threshold: 0.5,
        })
    }

    /// Set the speech detection threshold (default: 0.5).
    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold;
    }

    /// Process a chunk of 512 f32 samples at 16kHz.
    /// Returns speech probability between 0.0 and 1.0.
    pub fn process_chunk(&mut self, samples: &[f32]) -> Result<f32, String> {
        if samples.len() != CHUNK_SIZE {
            return Err(format!(
                "Expected {CHUNK_SIZE} samples, got {}",
                samples.len()
            ));
        }

        let input = TensorRef::from_array_view(([1i64, CHUNK_SIZE as i64], samples))
            .map_err(|e| format!("Failed to create input tensor: {e}"))?;

        let sr_data: [i64; 1] = [SAMPLE_RATE];
        let sr = TensorRef::from_array_view(([1i64], sr_data.as_slice()))
            .map_err(|e| format!("Failed to create sr tensor: {e}"))?;

        let state = TensorRef::from_array_view((
            [2i64, 1i64, STATE_DIM as i64],
            self.state.as_slice(),
        ))
        .map_err(|e| format!("Failed to create state tensor: {e}"))?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input" => input,
                "sr" => sr,
                "state" => state,
            ])
            .map_err(|e| format!("VAD inference failed: {e}"))?;

        // Extract speech probability
        let prob_output = &outputs[0];
        let (_, prob_data) = prob_output
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Failed to extract probability: {e}"))?;
        let probability = prob_data[0];

        // Update internal state
        let state_output = &outputs[1];
        let (_, new_state) = state_output
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Failed to extract state: {e}"))?;
        self.state.copy_from_slice(new_state);

        Ok(probability)
    }

    /// Reset internal state between utterances.
    pub fn reset(&mut self) {
        self.state.fill(0.0);
    }

    pub fn threshold(&self) -> f32 {
        self.threshold
    }
}

/// A segment of detected speech audio.
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    pub start_sample: usize,
    pub end_sample: usize,
    pub audio: Vec<f32>,
}

/// Wraps SileroVad to segment continuous audio into speech segments.
pub struct VadSegmenter {
    vad: SileroVad,
    /// Buffered samples not yet forming a full chunk
    buffer: Vec<f32>,
    /// Total samples processed (including buffered)
    total_samples: usize,
    /// Whether we're currently in a speech region
    in_speech: bool,
    /// Sample index where current speech started
    speech_start: usize,
    /// Sample index where speech last ended (for merging)
    last_speech_end: usize,
    /// Accumulated speech audio for current segment
    speech_audio: Vec<f32>,
    /// Completed segments
    segments: Vec<SpeechSegment>,
    /// All audio kept for padding extraction
    all_audio: Vec<f32>,
    /// Padding in samples (300ms at 16kHz = 4800 samples)
    padding_samples: usize,
    /// Merge gap in samples (500ms at 16kHz = 8000 samples)
    merge_gap_samples: usize,
}

impl VadSegmenter {
    pub fn new(vad: SileroVad) -> Self {
        let padding_samples = (SAMPLE_RATE as usize) * 300 / 1000; // 4800
        let merge_gap_samples = (SAMPLE_RATE as usize) * 500 / 1000; // 8000

        Self {
            vad,
            buffer: Vec::new(),
            total_samples: 0,
            in_speech: false,
            speech_start: 0,
            last_speech_end: 0,
            speech_audio: Vec::new(),
            segments: Vec::new(),
            all_audio: Vec::new(),
            padding_samples,
            merge_gap_samples,
        }
    }

    /// Feed audio samples and process them through VAD.
    pub fn process(&mut self, samples: &[f32]) -> Result<(), String> {
        self.all_audio.extend_from_slice(samples);
        self.buffer.extend_from_slice(samples);

        while self.buffer.len() >= CHUNK_SIZE {
            let chunk: Vec<f32> = self.buffer.drain(..CHUNK_SIZE).collect();
            let prob = self.vad.process_chunk(&chunk)?;
            let chunk_start = self.total_samples;
            self.total_samples += CHUNK_SIZE;

            if prob >= self.vad.threshold() {
                if !self.in_speech {
                    // Check if we should merge with previous segment
                    if !self.segments.is_empty()
                        && chunk_start.saturating_sub(self.last_speech_end) < self.merge_gap_samples
                    {
                        // Merge: pop last segment and continue from its start
                        let prev = self.segments.pop().unwrap();
                        self.speech_start = prev.start_sample;
                        self.speech_audio.clear();
                    } else {
                        self.speech_start = chunk_start;
                        self.speech_audio.clear();
                    }
                    self.in_speech = true;
                }
                self.speech_audio.extend_from_slice(&chunk);
            } else if self.in_speech {
                // End of speech
                self.in_speech = false;
                self.last_speech_end = self.total_samples;
                self.finalize_segment();
            }
        }

        Ok(())
    }

    /// Flush any remaining speech segment.
    pub fn flush(&mut self) -> Vec<SpeechSegment> {
        if self.in_speech {
            self.in_speech = false;
            self.last_speech_end = self.total_samples;
            self.finalize_segment();
        }
        self.vad.reset();
        std::mem::take(&mut self.segments)
    }

    fn finalize_segment(&mut self) {
        let padded_start = self.speech_start.saturating_sub(self.padding_samples);
        let padded_end = (self.last_speech_end + self.padding_samples).min(self.all_audio.len());

        if padded_end > padded_start {
            let audio = self.all_audio[padded_start..padded_end].to_vec();
            self.segments.push(SpeechSegment {
                start_sample: padded_start,
                end_sample: padded_end,
                audio,
            });
        }
        self.speech_audio.clear();
    }

    /// Reset the segmenter for a new recording.
    pub fn reset(&mut self) {
        self.vad.reset();
        self.buffer.clear();
        self.total_samples = 0;
        self.in_speech = false;
        self.speech_start = 0;
        self.last_speech_end = 0;
        self.speech_audio.clear();
        self.segments.clear();
        self.all_audio.clear();
    }
}

/// VAD model filename.
pub const VAD_MODEL_FILENAME: &str = "silero_vad.onnx";

/// VAD model download URL.
pub const VAD_MODEL_URL: &str =
    "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx";

/// VAD model approximate size in bytes (~2MB).
pub const VAD_MODEL_SIZE: u64 = 2_200_000;
