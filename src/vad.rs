/// Energy-based Voice Activity Detection with spectral flux and zero-crossing rate.
///
/// Uses RMS energy, zero-crossing rate, and spectral flux with adaptive
/// thresholding to detect speech segments. Not as accurate as neural VAD
/// (Silero) but has zero dependencies and eliminates Whisper hallucinations
/// on silence effectively.

const SAMPLE_RATE: usize = 16000;
/// Frame size for energy calculation: 30ms at 16kHz
const FRAME_SIZE: usize = SAMPLE_RATE * 30 / 1000; // 480 samples
/// Minimum speech duration to keep (250ms)
const MIN_SPEECH_FRAMES: usize = 9; // ~270ms at 30ms/frame
/// Minimum silence duration to split segments (300ms)
const MIN_SILENCE_FRAMES: usize = 10; // ~300ms at 30ms/frame
/// Padding around speech segments in samples (300ms)
const PADDING_SAMPLES: usize = SAMPLE_RATE * 300 / 1000; // 4800

/// ZCR range for speech (per 30ms frame of 480 samples)
const ZCR_MIN: usize = 20;
const ZCR_MAX: usize = 200;

/// Minimum spectral flux (dB change between frames) for speech
const SPECTRAL_FLUX_THRESHOLD: f32 = 0.5;

/// A segment of detected speech audio.
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    pub start_sample: usize,
    pub end_sample: usize,
    pub audio: Vec<f32>,
}

/// Energy-based VAD segmenter.
pub struct VadSegmenter {
    /// Energy threshold in dB. Frames below this are silence.
    threshold_db: f32,
}

impl VadSegmenter {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            threshold_db: -35.0,
        })
    }

    pub fn set_threshold(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db;
    }

    /// Segment audio into speech regions based on energy.
    /// Input: 16kHz mono f32 PCM samples.
    pub fn segment(&mut self, audio: &[f32]) -> Result<Vec<SpeechSegment>, String> {
        if audio.is_empty() {
            return Ok(Vec::new());
        }

        let frames: Vec<&[f32]> = audio.chunks(FRAME_SIZE).collect();
        let num_frames = frames.len();

        // Calculate per-frame energy in dB
        let frame_energies: Vec<f32> = frames
            .iter()
            .map(|frame| {
                let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
                if rms > 0.0 {
                    20.0 * rms.log10()
                } else {
                    -80.0
                }
            })
            .collect();

        // Calculate per-frame zero-crossing rate
        let frame_zcr: Vec<usize> = frames
            .iter()
            .map(|frame| {
                frame
                    .windows(2)
                    .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
                    .count()
            })
            .collect();

        // Calculate spectral flux (absolute energy change between consecutive frames)
        let spectral_flux: Vec<f32> = (0..num_frames)
            .map(|i| {
                if i == 0 {
                    frame_energies[0].abs()
                } else {
                    (frame_energies[i] - frame_energies[i - 1]).abs()
                }
            })
            .collect();

        // Adaptive threshold: use the louder of fixed threshold or noise floor + 10dB
        let sorted_energies = {
            let mut e = frame_energies.clone();
            e.sort_by(|a, b| a.partial_cmp(b).unwrap());
            e
        };
        let noise_floor = sorted_energies[sorted_energies.len() / 10]; // 10th percentile
        let adaptive_threshold = self.threshold_db.max(noise_floor + 10.0);

        // Label each frame as speech when energy is above threshold and at least
        // one secondary indicator (ZCR or spectral flux) supports it. Requiring
        // all three was too aggressive for some microphones (e.g. webcam mics).
        let is_speech: Vec<bool> = (0..num_frames)
            .map(|i| {
                if frame_energies[i] <= adaptive_threshold {
                    return false;
                }
                let zcr_ok = frame_zcr[i] >= ZCR_MIN && frame_zcr[i] <= ZCR_MAX;
                let flux_ok = spectral_flux[i] > SPECTRAL_FLUX_THRESHOLD;
                zcr_ok || flux_ok
            })
            .collect();

        // Find speech regions (runs of speech frames)
        let mut regions: Vec<(usize, usize)> = Vec::new();
        let mut in_speech = false;
        let mut start = 0;
        let mut silence_count = 0;

        for (i, &speech) in is_speech.iter().enumerate() {
            if speech {
                if !in_speech {
                    start = i;
                    in_speech = true;
                }
                silence_count = 0;
            } else if in_speech {
                silence_count += 1;
                if silence_count >= MIN_SILENCE_FRAMES {
                    let end = i - silence_count + 1;
                    if end - start >= MIN_SPEECH_FRAMES {
                        regions.push((start, end));
                    }
                    in_speech = false;
                    silence_count = 0;
                }
            }
        }

        // Handle trailing speech
        if in_speech {
            let end = is_speech.len();
            if end - start >= MIN_SPEECH_FRAMES {
                regions.push((start, end));
            }
        }

        // Convert frame indices to sample indices with padding
        let segments = regions
            .iter()
            .map(|&(start_frame, end_frame)| {
                let start_sample = (start_frame * FRAME_SIZE).saturating_sub(PADDING_SAMPLES);
                let end_sample = ((end_frame * FRAME_SIZE) + PADDING_SAMPLES).min(audio.len());
                SpeechSegment {
                    start_sample,
                    end_sample,
                    audio: audio[start_sample..end_sample].to_vec(),
                }
            })
            .collect();

        Ok(segments)
    }
}
