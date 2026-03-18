use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const SAMPLE_RATE: usize = 16000;

use crate::audio_preprocess::normalize_gain;
use crate::vad::VadSegmenter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub segments: Vec<Segment>,
    pub language: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionConfig {
    /// Language code (e.g. "en") or None for auto-detect.
    pub language: Option<String>,
    /// If true, translate to English.
    pub translate: bool,
    /// Number of threads for whisper inference.
    pub thread_count: i32,
    /// Beam size for decoding. 1 = greedy (fastest on CPU), 5 = beam search (better on GPU).
    pub beam_size: i32,
    /// If true (default), use VAD to segment audio before transcription.
    #[serde(default = "default_use_vad")]
    pub use_vad: bool,
}

fn default_use_vad() -> bool {
    true
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        let thread_count = std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4);
        Self {
            language: Some("en".into()),
            translate: false,
            thread_count,
            beam_size: 1,
            use_vad: true,
        }
    }
}

#[derive(Clone)]
pub struct TranscriptionService {
    ctx: Arc<WhisperContext>,
    config: TranscriptionConfig,
}

impl TranscriptionService {
    pub fn new(model_path: &Path, config: TranscriptionConfig) -> Result<Self, String> {
        let params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().ok_or("Invalid model path")?,
            params,
        )
        .map_err(|e| format!("Failed to load model: {e}. The file may be corrupted. Try: `outspoken config download large-v3-turbo-q5_0` to re-download."))?;

        Ok(Self {
            ctx: Arc::new(ctx),
            config,
        })
    }

    pub fn config(&self) -> &TranscriptionConfig {
        &self.config
    }

    pub fn transcribe(&self, audio_data: &[f32]) -> Result<TranscriptionResult, String> {
        let start = std::time::Instant::now();

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("Failed to create state: {e}"))?;

        let strategy = if self.config.beam_size > 1 {
            SamplingStrategy::BeamSearch {
                beam_size: self.config.beam_size,
                patience: -1.0,
            }
        } else {
            SamplingStrategy::Greedy { best_of: 1 }
        };
        let mut params = FullParams::new(strategy);
        params.set_language(self.config.language.as_deref());
        params.set_translate(self.config.translate);
        params.set_n_threads(self.config.thread_count);
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_token_timestamps(true);

        // Anti-hallucination: set initial prompt to ground the model
        params.set_initial_prompt("This is a voice dictation transcript. The speaker is dictating text naturally.");

        // Suppress non-speech: if a segment has high no-speech probability, skip it
        params.set_no_speech_thold(0.6);

        // Limit single-segment hallucinations by setting max segment length
        params.set_max_len(0); // 0 = no limit on segment length (prevents splitting mid-sentence)

        // Suppress specific hallucinated tokens
        params.set_suppress_non_speech_tokens(true);

        state
            .full(params, audio_data)
            .map_err(|e| format!("Transcription failed: {e}"))?;

        let num_segments = state
            .full_n_segments()
            .map_err(|e| format!("Failed to get segments: {e}"))?;

        let mut segments = Vec::new();
        let mut full_text = String::new();

        for i in 0..num_segments {
            let text = state
                .full_get_segment_text(i)
                .map_err(|e| format!("Failed to get segment text: {e}"))?;
            let start_ts = state
                .full_get_segment_t0(i)
                .map_err(|e| format!("Failed to get segment start: {e}"))?;
            let end_ts = state
                .full_get_segment_t1(i)
                .map_err(|e| format!("Failed to get segment end: {e}"))?;

            // whisper timestamps are in centiseconds (10ms units)
            let start_ms = start_ts as i64 * 10;
            let end_ms = end_ts as i64 * 10;

            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                if !full_text.is_empty() {
                    full_text.push(' ');
                }
                full_text.push_str(&trimmed);
                segments.push(Segment {
                    start_ms,
                    end_ms,
                    text: trimmed,
                });
            }
        }

        let detected_language = self
            .config
            .language
            .clone()
            .unwrap_or_else(|| "en".into());
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(TranscriptionResult {
            text: full_text,
            segments,
            language: detected_language,
            duration_ms,
        })
    }

    /// Transcribe audio using VAD to segment speech regions first.
    ///
    /// For each speech segment detected by VAD:
    /// 1. Extract the audio slice
    /// 2. Apply gain normalization
    /// 3. Transcribe with Whisper
    /// 4. Offset timestamps to be relative to original audio
    ///
    /// If VAD finds no speech, returns an empty result (no hallucination).
    pub fn transcribe_with_vad(
        &self,
        audio: &[f32],
        vad: &mut VadSegmenter,
    ) -> Result<TranscriptionResult, String> {
        let start = std::time::Instant::now();

        // Run VAD segmentation
        let speech_segments = vad.segment(audio)?;

        // No speech detected — return empty result
        if speech_segments.is_empty() {
            return Ok(TranscriptionResult {
                text: String::new(),
                segments: Vec::new(),
                language: self
                    .config
                    .language
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        let sample_rate = 16000.0f64; // Whisper expects 16kHz
        let mut all_segments = Vec::new();
        let mut all_text = String::new();
        let mut detected_language = None;

        for speech in &speech_segments {
            // Apply gain normalization to the segment
            let mut normalized = normalize_gain(&speech.audio);

            // Whisper requires at least 1s of audio; pad to 1.5s for framing headroom
            let min_samples = (sample_rate * 1.5) as usize; // 24000 samples = 1.5s
            if normalized.len() < min_samples {
                normalized.resize(min_samples, 0.0);
            }

            // Transcribe the normalized segment
            let seg_result = self.transcribe(&normalized)?;

            // Store detected language from first segment with speech
            if detected_language.is_none() && !seg_result.text.is_empty() {
                detected_language = Some(seg_result.language.clone());
            }

            // Offset timestamps to be relative to original audio
            let offset_ms = (speech.start_sample as f64 / sample_rate * 1000.0) as i64;

            for seg in seg_result.segments {
                let adjusted = Segment {
                    start_ms: seg.start_ms + offset_ms,
                    end_ms: seg.end_ms + offset_ms,
                    text: seg.text.clone(),
                };

                if !adjusted.text.is_empty() {
                    if !all_text.is_empty() {
                        all_text.push(' ');
                    }
                    all_text.push_str(&adjusted.text);
                    all_segments.push(adjusted);
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(TranscriptionResult {
            text: all_text,
            segments: all_segments,
            language: detected_language
                .or_else(|| self.config.language.clone())
                .unwrap_or_else(|| "unknown".into()),
            duration_ms,
        })
    }

    pub fn transcribe_streaming(
        &self,
        buffer: &mut Vec<f32>,
        audio_chunk: &[f32],
        is_final: bool,
    ) -> Result<TranscriptionResult, String> {
        buffer.extend_from_slice(audio_chunk);

        if !is_final && buffer.len() < SAMPLE_RATE {
            return Ok(TranscriptionResult {
                text: String::new(),
                segments: Vec::new(),
                language: self
                    .config
                    .language
                    .clone()
                    .unwrap_or_else(|| "en".into()),
                duration_ms: 0,
            });
        }

        let result = self.transcribe(buffer)?;

        if is_final {
            buffer.clear();
        }

        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportedLanguage {
    pub code: String,
    pub name: String,
}

/// Returns the list of supported languages.
/// When the `multilingual` feature is disabled, only English is supported.
#[cfg(not(feature = "multilingual"))]
pub fn supported_languages() -> Vec<SupportedLanguage> {
    vec![SupportedLanguage {
        code: "en".to_string(),
        name: "English".to_string(),
    }]
}

/// Returns the list of languages supported by Whisper.
/// Note: whisper_rs::standalone is no longer public in newer versions,
/// so we return a curated list of common Whisper-supported languages.
#[cfg(feature = "multilingual")]
pub fn supported_languages() -> Vec<SupportedLanguage> {
    let languages = [
        ("en", "English"), ("zh", "Chinese"), ("de", "German"), ("es", "Spanish"),
        ("ru", "Russian"), ("ko", "Korean"), ("fr", "French"), ("ja", "Japanese"),
        ("pt", "Portuguese"), ("tr", "Turkish"), ("pl", "Polish"), ("ca", "Catalan"),
        ("nl", "Dutch"), ("ar", "Arabic"), ("sv", "Swedish"), ("it", "Italian"),
        ("id", "Indonesian"), ("hi", "Hindi"), ("fi", "Finnish"), ("vi", "Vietnamese"),
        ("he", "Hebrew"), ("uk", "Ukrainian"), ("el", "Greek"), ("ms", "Malay"),
        ("cs", "Czech"), ("ro", "Romanian"), ("da", "Danish"), ("hu", "Hungarian"),
        ("ta", "Tamil"), ("no", "Norwegian"), ("th", "Thai"), ("ur", "Urdu"),
        ("hr", "Croatian"), ("bg", "Bulgarian"), ("lt", "Lithuanian"), ("la", "Latin"),
        ("mi", "Maori"), ("ml", "Malayalam"), ("cy", "Welsh"), ("sk", "Slovak"),
        ("te", "Telugu"), ("fa", "Persian"), ("lv", "Latvian"), ("bn", "Bengali"),
        ("sr", "Serbian"), ("az", "Azerbaijani"), ("sl", "Slovenian"), ("kn", "Kannada"),
        ("et", "Estonian"), ("mk", "Macedonian"),
    ];
    languages
        .iter()
        .map(|(code, name)| SupportedLanguage {
            code: code.to_string(),
            name: name.to_string(),
        })
        .collect()
}
