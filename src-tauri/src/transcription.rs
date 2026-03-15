use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

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
    /// If true, strip filler words from output.
    pub strip_filler_words: bool,
    /// Beam size for decoding. 1 = greedy (fastest on CPU), 5 = beam search (better on GPU).
    pub beam_size: i32,
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
            strip_filler_words: false,
            beam_size: 1,
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
        .map_err(|e| format!("Failed to load whisper model: {e}"))?;

        Ok(Self {
            ctx: Arc::new(ctx),
            config,
        })
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

        if self.config.strip_filler_words {
            full_text = strip_filler_words(&full_text);
            for seg in &mut segments {
                seg.text = strip_filler_words(&seg.text);
            }
            segments.retain(|s| !s.text.is_empty());
        }

        let detected_language = state
            .full_lang_id_from_state()
            .ok()
            .and_then(|id| whisper_rs::standalone::get_lang_str(id).map(|s| s.to_string()))
            .or_else(|| self.config.language.clone())
            .unwrap_or_else(|| "unknown".into());
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(TranscriptionResult {
            text: full_text,
            segments,
            language: detected_language,
            duration_ms,
        })
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

/// Returns the list of languages supported by Whisper, queried from whisper.cpp.
#[cfg(feature = "multilingual")]
pub fn supported_languages() -> Vec<SupportedLanguage> {
    let max_id = whisper_rs::standalone::get_lang_max_id();
    let mut languages = Vec::new();
    for id in 0..=max_id {
        if let (Some(code), Some(name)) = (
            whisper_rs::standalone::get_lang_str(id),
            whisper_rs::standalone::get_lang_str_full(id),
        ) {
            languages.push(SupportedLanguage {
                code: code.to_string(),
                name: name.to_string(),
            });
        }
    }
    languages
}

/// Remove common filler words using simple regex-based replacement.
fn strip_filler_words(text: &str) -> String {
    // Pattern matches common English filler words as whole words (case-insensitive).
    let fillers = [
        "um", "uh", "er", "ah", "like", "you know", "I mean", "so", "well", "actually",
        "basically", "literally", "right",
    ];

    let mut result = text.to_string();
    for filler in &fillers {
        // Match whole word boundaries with case-insensitive matching.
        let pattern = format!(r"(?i)\b{}\b", regex_lite::escape(filler));
        if let Ok(re) = regex_lite::Regex::new(&pattern) {
            result = re.replace_all(&result, "").to_string();
        }
    }

    // Clean up extra whitespace left by removals.
    let ws_re = regex_lite::Regex::new(r"\s{2,}").unwrap();
    ws_re.replace_all(result.trim(), " ").to_string()
}
