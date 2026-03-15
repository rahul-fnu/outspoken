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

#[cfg(test)]
mod tests {
    use super::*;

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
