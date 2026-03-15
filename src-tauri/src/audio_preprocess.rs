

```rust
/// Audio preprocessing for improving transcription accuracy.
/// Normalizes gain and trims silence before whisper inference.

/// Convert a linear amplitude to decibels (dBFS).
fn linear_to_db(linear: f32) -> f32 {
    20.0 * linear.log10()
}

/// Convert decibels (dBFS) to a linear amplitude.
fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Calculate the RMS (root mean square) of audio samples.
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Normalize audio gain so that the RMS matches `target_db` (in dBFS).
///
/// Scales all samples proportionally and clamps to [-1.0, 1.0] to prevent clipping.
/// Default target is -20 dBFS, which matches Whisper's training data range.
pub fn normalize_gain_rms(samples: &mut [f32], target_db: f32) {
    if samples.is_empty() {
        return;
    }

    let current_rms = rms(samples);
    if current_rms < 1e-10 {
        // Essentially silence — nothing to normalize.
        return;
    }

    let current_db = linear_to_db(current_rms);
    let gain_db = target_db - current_db;
    let gain = db_to_linear(gain_db);

    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

/// Normalize audio gain to a target peak level.
///
/// Scales the audio so the peak amplitude reaches `target_peak` (default 0.95).
/// Returns the original slice unchanged if the audio is silent (peak below `silence_threshold`).
pub fn normalize_gain(audio: &[f32]) -> Vec<f32> {
    normalize_gain_with_params(audio, 0.95, 1e-6)
}

fn normalize_gain_with_params(audio: &[f32], target_peak: f32, silence_threshold: f32) -> Vec<f32> {
    if audio.is_empty() {
        return Vec::new();
    }

    let peak = audio
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);

    if peak < silence_threshold {
        return audio.to_vec();
    }

    let gain = target_peak / peak;
    audio.iter().map(|s| (s * gain).clamp(-1.0, 1.0)).collect()
}

/// Trim leading and trailing silence below `threshold_db` (in dBFS).
///
/// Uses a small window (10ms at 16kHz = 160 samples) to detect silence.
/// Returns a sub-slice of the input with silence removed from both ends.
pub fn trim_silence(samples: &[f32], threshold_db: f32) -> &[f32] {
    if samples.is_empty() {
        return samples;
    }

    let threshold_linear = db_to_linear(threshold_db);
    let window_size = 160; // ~10ms at 16kHz

    // Find first non-silent window from the start.
    let start = find_voice_edge(samples, threshold_linear, window_size, false);

    // Find first non-silent window from the end.
    let end = find_voice_edge(samples, threshold_linear, window_size, true);

    if start >= end {
        // All silence — return empty slice.
        return &samples[0..0];
    }

    &samples[start..end]
}

/// Find the sample index where audio exceeds the threshold.
/// If `from_end` is true, searches backwards and returns the end index (exclusive).
fn find_voice_edge(samples: &[f32], threshold: f32, window_size: usize, from_end: bool) -> usize {
    let len = samples.len();
    let step = window_size;

    if from_end {
        let mut pos = len;
        while pos > 0 {
            let window_start = pos.saturating_sub(step);
            let window = &samples[window_start..pos];
            if rms(window) >= threshold {
                return pos;
            }
            pos = window_start;
        }
        0
    } else {
        let mut pos = 0;
        while pos < len {
            let window_end = (pos + step).min(len);
            let window = &samples[pos..window_end];
            if rms(window) >= threshold {
                return pos;
            }
            pos = window_end;
        }
        len
    }
}

/// Apply all preprocessing steps to audio samples before transcription.
/// Returns a new Vec with normalized, trimmed audio.
pub fn preprocess_audio(samples: &[f32]) -> Vec<f32> {
    // First trim silence at -40 dBFS.
    let trimmed = trim_silence(samples, -40.0);

    if trimmed.is_empty() {
        return Vec::new();
    }

    // Then normalize gain to -20 dBFS.
    let mut normalized = trimmed.to_vec();
    normalize_gain_rms(&mut normalized, -20.0);

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_quiet_audio() {
        // Generate a quiet sine wave at roughly -40 dBFS RMS
        let target_rms = db_to_linear(-40.0);
        let mut samples: Vec<f32> = (0..16000)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16000.0).sin() * target_rms * std::f32::consts::SQRT_2)
            .collect();

        let rms_before = rms(&samples);
        assert!((linear_to_db(rms_before) - (-40.0)).abs() < 1.0);

        normalize_gain_rms(&mut samples, -20.0);

        let rms_after = rms(&samples);
        assert!((linear_to_db(rms_after) - (-20.0)).abs() < 1.0);

        // All samples should be within [-1, 1]
        assert!(samples.iter().all(|&s| s >= -1.0 && s <= 1.0));
    }

    #[test]
    fn test_normalize_loud_audio() {
        // Generate a loud signal near 0 dBFS
        let mut samples: Vec<f32> = (0..16000)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16000.0).sin() * 0.95)
            .collect();

        normalize_gain_rms(&mut samples, -20.0);

        // Should be scaled down, all within range
        assert!(samples.iter().all(|&s| s >= -1.0 && s <= 1.0));

        let rms_after = rms(&samples);
        assert!((linear_to_db(rms_after) - (-20.0)).abs() < 1.0);
    }

    #[test]
    fn test_trim_silence() {
        let mut samples = vec![0.0f32; 16000]; // 1 second of silence
        // Insert 0.5 seconds of audio in the middle
        for i in 4000..12000 {
            samples[i] = (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16000.0).sin() * 0.5;
        }

        let trimmed = trim_silence(&samples, -40.0);
        // Should be shorter than original
        assert!(trimmed.len() < samples.len());
        // Should still contain the audio content
        assert!(!trimmed.is_empty());
    }

    #[test]
    fn test_all_silence() {
        let samples = vec![0.0f32; 1600];
        let trimmed = trim_silence(&samples, -40.0);
        assert!(trimmed.is_empty());
    }

    #[test]
    fn test_empty_input() {
        let mut empty: Vec<f32> = vec![];
        normalize_gain_rms(&mut empty, -20.0);
        assert!(empty.is_empty());

        let trimmed = trim_silence(&empty, -40.0);
        assert!(trimmed.is_empty());
    }

    #[test]
    fn test_clamp_prevents_clipping() {
        // Very quiet audio normalized aggressively should still be clamped
        let mut samples = vec![0.001f32; 100];
        samples[50] = 0.5; // One louder sample

        normalize_gain_rms(&mut samples, -3.0);

        assert!(samples.iter().all(|&s| s >= -1.0 && s <= 1.0));
    }

    #[test]
    fn test_normalize_gain_silent() {
        let silent = vec![0.0f32; 100];
        let result = normalize_gain(&silent);
        assert_eq!(result, silent);
    }

    #[test]
    fn test_normalize_gain_scales_up() {
        let audio = vec![0.1, -0.2, 0.15, -0.05];
        let result = normalize_gain(&audio);
        let peak = result.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!((peak - 0.95).abs() < 0.01);
    }

    #[test]
    fn test_normalize_gain_empty() {
        let result = normalize_gain(&[]);
        assert!(result.is_empty());
    }
}
```
