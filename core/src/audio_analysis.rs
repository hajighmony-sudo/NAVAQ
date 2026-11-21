//! Streaming PCM onset analysis for beat-reactive features.
//!
//! This deliberately reports generic energy onsets, not semantic kick/snare/vocal
//! labels. A host may map these events to a generic beat haptic, while richer
//! instrument classification requires spectral/source-separation analysis.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioAnalysisSettings {
    /// Analysis window in milliseconds.
    pub frame_ms: u32,
    /// Onset energy must exceed the adaptive floor by this multiplier.
    pub sensitivity: f32,
    /// Minimum absolute RMS to avoid triggering on silence/noise.
    pub minimum_rms: f32,
    /// Minimum spacing between emitted onsets.
    pub refractory_ms: u32,
}

impl Default for AudioAnalysisSettings {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            sensitivity: 2.2,
            minimum_rms: 0.025,
            refractory_ms: 120,
        }
    }
}

impl AudioAnalysisSettings {
    pub fn is_valid(&self) -> bool {
        (5..=100).contains(&self.frame_ms)
            && self.sensitivity.is_finite()
            && (1.1..=8.0).contains(&self.sensitivity)
            && self.minimum_rms.is_finite()
            && (0.0..=1.0).contains(&self.minimum_rms)
            && (40..=2_000).contains(&self.refractory_ms)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioOnset {
    /// End timestamp of the analysis frame, relative to analyzer creation/reset.
    pub position_ms: u64,
    /// RMS energy of the frame.
    pub rms: f32,
    /// Ratio between frame energy and the adaptive noise floor.
    pub strength: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioAnalysisError {
    InvalidFormat,
    InvalidSettings,
}

#[derive(Debug, Clone)]
pub struct AudioOnsetAnalyzer {
    sample_rate: u32,
    channels: usize,
    settings: AudioAnalysisSettings,
    frame_samples: usize,
    pending: Vec<f32>,
    processed_frames: u64,
    noise_floor: Option<f32>,
    last_onset_ms: Option<u64>,
}

impl AudioOnsetAnalyzer {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        settings: AudioAnalysisSettings,
    ) -> Result<Self, AudioAnalysisError> {
        if sample_rate < 8_000 || sample_rate > 192_000 || channels == 0 || channels > 8 {
            return Err(AudioAnalysisError::InvalidFormat);
        }
        if !settings.is_valid() {
            return Err(AudioAnalysisError::InvalidSettings);
        }
        let frame_samples = ((sample_rate as u64 * settings.frame_ms as u64) / 1_000)
            .max(1) as usize
            * channels as usize;
        Ok(Self {
            sample_rate,
            channels: channels as usize,
            settings,
            frame_samples,
            pending: Vec::with_capacity(frame_samples * 2),
            processed_frames: 0,
            noise_floor: None,
            last_onset_ms: None,
        })
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.processed_frames = 0;
        self.noise_floor = None;
        self.last_onset_ms = None;
    }

    /// Feed interleaved f32 PCM. Partial windows are retained between calls.
    /// Non-finite samples are treated as silence. Returned events are time ordered.
    pub fn process(&mut self, samples: &[f32]) -> Vec<AudioOnset> {
        if samples.len() % self.channels != 0 {
            return Vec::new();
        }
        self.pending.extend(
            samples
                .iter()
                .map(|sample| if sample.is_finite() { *sample } else { 0.0 }),
        );
        let mut events = Vec::new();
        while self.pending.len() >= self.frame_samples {
            let frame: Vec<f32> = self.pending.drain(..self.frame_samples).collect();
            let square_sum: f64 = frame
                .iter()
                .map(|sample| (*sample as f64) * (*sample as f64))
                .sum();
            let rms = (square_sum / frame.len() as f64).sqrt() as f32;
            let frame_count = (self.frame_samples / self.channels) as u64;
            self.processed_frames = self.processed_frames.saturating_add(frame_count);
            let position_ms = self.processed_frames.saturating_mul(1_000)
                / self.sample_rate as u64;

            let floor = self.noise_floor.unwrap_or(rms.max(0.0001));
            let strength = rms / floor.max(0.0001);
            let refractory_passed = self.last_onset_ms.map_or(true, |last| {
                position_ms.saturating_sub(last) >= self.settings.refractory_ms as u64
            });
            let onset = self.noise_floor.is_some()
                && rms >= self.settings.minimum_rms
                && strength >= self.settings.sensitivity
                && refractory_passed;

            if onset {
                self.last_onset_ms = Some(position_ms);
                events.push(AudioOnset {
                    position_ms,
                    rms,
                    strength,
                });
            }

            // Slow adaptive floor; suppress upward adaptation on transient frames.
            let target = if onset { floor.min(rms) } else { rms };
            let alpha = if target < floor { 0.08 } else { 0.015 };
            self.noise_floor = Some((floor + alpha * (target - floor)).clamp(0.0001, 1.0));
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_transient_after_quiet_baseline_and_preserves_partial_frames() {
        let mut analyzer = AudioOnsetAnalyzer::new(
            10_000,
            1,
            AudioAnalysisSettings {
                frame_ms: 10,
                sensitivity: 2.0,
                minimum_rms: 0.02,
                refractory_ms: 80,
            },
        )
        .unwrap();

        assert!(analyzer.process(&[0.001; 50]).is_empty());
        assert!(analyzer.process(&[0.001; 50]).is_empty());
        assert!(analyzer.process(&[0.8; 100]).len() == 1);
    }

    #[test]
    fn rejects_invalid_format_settings_and_misaligned_input() {
        assert_eq!(
            AudioOnsetAnalyzer::new(1_000, 2, AudioAnalysisSettings::default()).err(),
            Some(AudioAnalysisError::InvalidFormat)
        );
        let mut settings = AudioAnalysisSettings::default();
        settings.sensitivity = f32::NAN;
        assert_eq!(
            AudioOnsetAnalyzer::new(48_000, 2, settings).err(),
            Some(AudioAnalysisError::InvalidSettings)
        );
        let mut analyzer = AudioOnsetAnalyzer::new(
            48_000,
            2,
            AudioAnalysisSettings::default(),
        )
        .unwrap();
        assert!(analyzer.process(&[0.1]).is_empty());
    }

    #[test]
    fn refractory_window_limits_repeated_triggers_and_reset_restarts_clock() {
        let mut analyzer = AudioOnsetAnalyzer::new(
            10_000,
            1,
            AudioAnalysisSettings {
                frame_ms: 10,
                sensitivity: 1.5,
                minimum_rms: 0.02,
                refractory_ms: 100,
            },
        )
        .unwrap();
        analyzer.process(&[0.001; 100]);
        assert_eq!(analyzer.process(&[0.8; 10]).len(), 1);
        assert!(analyzer.process(&[0.001; 100]).is_empty());
        analyzer.reset();
        assert!(analyzer.process(&[0.001; 100]).is_empty());
    }
}
