//! Platform-independent PCM processing primitives for native audio backends.
//!
//! This module intentionally does not decode files or own an audio device. A backend
//! feeds interleaved floating-point PCM frames through `DspProcessor` and may use
//! `mix_equal_power_crossfade` when two decoded streams overlap.

use crate::equalizer::EqualizerSettings;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DspError {
    #[error("sample rate and channel count must be non-zero")]
    InvalidFormat,
    #[error("equalizer settings are invalid")]
    InvalidEqualizer,
    #[error("PCM buffer length must be a multiple of the channel count")]
    MisalignedBuffer,
    #[error("crossfade buffers must have equal non-zero lengths")]
    InvalidCrossfadeBuffers,
    #[error("echo parameters must be within their supported ranges")]
    InvalidEffectSettings,
}

#[derive(Debug, Clone, Copy)]
struct BiquadCoefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoefficients {
    /// RBJ peaking-EQ coefficients, normalized by a0.
    fn peaking(sample_rate: u32, frequency_hz: u32, gain_db: f32) -> Option<Self> {
        if gain_db.abs() < 1.0e-4 {
            return None;
        }
        let nyquist = sample_rate as f32 * 0.5;
        let frequency = (frequency_hz as f32).min(nyquist * 0.95);
        if frequency <= 0.0 || !frequency.is_finite() {
            return None;
        }

        let a = 10.0_f32.powf(gain_db / 40.0);
        let omega = 2.0 * std::f32::consts::PI * frequency / sample_rate as f32;
        let alpha = omega.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let cos = omega.cos();
        let a0 = 1.0 + alpha / a;
        if !a0.is_finite() || a0.abs() < f32::EPSILON {
            return None;
        }

        Some(Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, input: f32, c: BiquadCoefficients) -> f32 {
        // Direct Form II Transposed: two state values per channel and band.
        let output = c.b0 * input + self.z1;
        self.z1 = c.b1 * input - c.a1 * output + self.z2;
        self.z2 = c.b2 * input - c.a2 * output;
        output
    }
}

/// Stateful equalizer and gain stage for interleaved f32 PCM.
///
/// Each channel has independent filter history. Keep this processor alive between
/// buffers; constructing it for every callback would reset the filter and cause clicks.
#[derive(Debug, Clone)]
pub struct DspProcessor {
    channels: usize,
    linear_gain: f32,
    filters: Vec<(BiquadCoefficients, Vec<BiquadState>)>,
}

impl DspProcessor {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        equalizer: &EqualizerSettings,
        linear_gain: f32,
    ) -> Result<Self, DspError> {
        if sample_rate == 0 || sample_rate > 384_000 || channels == 0 || channels > 8 {
            return Err(DspError::InvalidFormat);
        }
        if !equalizer.is_valid() {
            return Err(DspError::InvalidEqualizer);
        }
        let channels = channels as usize;
        let mut filters = Vec::new();
        if equalizer.enabled {
            for band in &equalizer.bands {
                if let Some(coefficients) =
                    BiquadCoefficients::peaking(sample_rate, band.frequency_hz, band.gain_db)
                {
                    filters.push((coefficients, vec![BiquadState::default(); channels]));
                }
            }
        }

        Ok(Self {
            channels,
            linear_gain: if linear_gain.is_finite() {
                linear_gain.clamp(0.0, 4.0)
            } else {
                1.0
            },
            filters,
        })
    }

    /// Process samples in-place. The buffer must contain complete interleaved frames.
    pub fn process_interleaved(&mut self, samples: &mut [f32]) -> Result<(), DspError> {
        if samples.len() % self.channels != 0 {
            return Err(DspError::MisalignedBuffer);
        }

        for (frame_index, frame) in samples.chunks_exact_mut(self.channels).enumerate() {
            let _ = frame_index;
            for (channel, sample) in frame.iter_mut().enumerate() {
                let mut value = if sample.is_finite() { *sample } else { 0.0 };
                value *= self.linear_gain;
                for (coefficients, states) in &mut self.filters {
                    value = states[channel].process(value, *coefficients);
                }
                // Do not hard-clip here: preserve headroom for the output device/limiter.
                *sample = if value.is_finite() { value } else { 0.0 };
            }
        }
        Ok(())
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn active_band_count(&self) -> usize {
        self.filters.len()
    }
}

/// Stateful feedback delay for an echo effect on interleaved PCM.
#[derive(Debug, Clone)]
pub struct EchoProcessor {
    channels: usize,
    delay_frames: usize,
    write_frame: usize,
    feedback: f32,
    mix: f32,
    delay: Vec<f32>,
}

impl EchoProcessor {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        delay_ms: u32,
        feedback: f32,
        mix: f32,
    ) -> Result<Self, DspError> {
        if sample_rate == 0 || sample_rate > 384_000 || channels == 0 || channels > 8 {
            return Err(DspError::InvalidFormat);
        }
        if delay_ms == 0
            || delay_ms > 2000
            || !feedback.is_finite()
            || !(0.0..=0.95).contains(&feedback)
            || !mix.is_finite()
            || !(0.0..=1.0).contains(&mix)
        {
            return Err(DspError::InvalidEffectSettings);
        }
        let channels = channels as usize;
        let delay_frames = ((sample_rate as u64 * delay_ms as u64 + 999) / 1000) as usize;
        Ok(Self {
            channels,
            delay_frames,
            write_frame: 0,
            feedback,
            mix,
            delay: vec![0.0; delay_frames * channels],
        })
    }

    /// Process complete interleaved frames in place; state persists across callbacks.
    pub fn process_interleaved(&mut self, samples: &mut [f32]) -> Result<(), DspError> {
        if samples.len() % self.channels != 0 {
            return Err(DspError::MisalignedBuffer);
        }
        for frame in samples.chunks_exact_mut(self.channels) {
            let base = self.write_frame * self.channels;
            for (channel, sample) in frame.iter_mut().enumerate() {
                let dry = if sample.is_finite() { *sample } else { 0.0 };
                let delayed = self.delay[base + channel];
                self.delay[base + channel] = (dry + delayed * self.feedback).clamp(-4.0, 4.0);
                let wet = dry * (1.0 - self.mix) + delayed * self.mix;
                *sample = if wet.is_finite() { wet } else { 0.0 };
            }
            self.write_frame = (self.write_frame + 1) % self.delay_frames;
        }
        Ok(())
    }
}

/// Mix two equally sized interleaved PCM buffers with an equal-power crossfade.
///
/// `progress` is clamped to 0..=1. At the midpoint both streams are attenuated
/// by approximately 3 dB, avoiding the perceived dip of a linear crossfade.
pub fn mix_equal_power_crossfade(
    outgoing: &[f32],
    incoming: &[f32],
    progress: f32,
    output: &mut [f32],
) -> Result<(), DspError> {
    if outgoing.is_empty() || outgoing.len() != incoming.len() || outgoing.len() != output.len() {
        return Err(DspError::InvalidCrossfadeBuffers);
    }
    let progress = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let angle = progress * std::f32::consts::FRAC_PI_2;
    let outgoing_gain = angle.cos();
    let incoming_gain = angle.sin();

    for ((out, incoming), mixed) in outgoing.iter().zip(incoming).zip(output) {
        let a = if out.is_finite() { *out } else { 0.0 };
        let b = if incoming.is_finite() { *incoming } else { 0.0 };
        *mixed = a * outgoing_gain + b * incoming_gain;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equalizer::EqualizerPreset;

    #[test]
    fn flat_disabled_equalizer_preserves_pcm_and_channels() {
        let mut processor =
            DspProcessor::new(48_000, 2, &EqualizerSettings::default(), 1.0).unwrap();
        let mut samples = [0.25, -0.5, 0.75, -1.0];
        processor.process_interleaved(&mut samples).unwrap();
        assert_eq!(samples, [0.25, -0.5, 0.75, -1.0]);
        assert_eq!(processor.channels(), 2);
        assert_eq!(processor.active_band_count(), 0);
    }

    #[test]
    fn enabled_equalizer_builds_only_non_neutral_filters() {
        let settings = EqualizerSettings::preset(EqualizerPreset::BassBoost);
        let processor = DspProcessor::new(48_000, 2, &settings, 1.0).unwrap();
        assert!(processor.active_band_count() > 0);
        assert!(processor.active_band_count() <= settings.bands.len());
    }

    #[test]
    fn processor_keeps_filter_state_between_buffers() {
        let settings = EqualizerSettings::preset(EqualizerPreset::BassBoost);
        let mut processor = DspProcessor::new(48_000, 1, &settings, 1.0).unwrap();
        let mut first = [1.0, 0.0, 0.0, 0.0];
        let mut second = [0.0, 0.0, 0.0, 0.0];
        processor.process_interleaved(&mut first).unwrap();
        processor.process_interleaved(&mut second).unwrap();
        assert!(second.iter().any(|sample| sample.abs() > 1.0e-6));
    }

    #[test]
    fn applies_gain_and_sanitizes_non_finite_samples() {
        let mut processor =
            DspProcessor::new(44_100, 1, &EqualizerSettings::default(), 0.5).unwrap();
        let mut samples = [1.0, f32::NAN, f32::INFINITY, -1.0];
        processor.process_interleaved(&mut samples).unwrap();
        assert_eq!(samples, [0.5, 0.0, 0.0, -0.5]);
    }

    #[test]
    fn rejects_unsupported_device_channel_counts_and_sample_rates() {
        assert_eq!(
            DspProcessor::new(768_000, 2, &EqualizerSettings::default(), 1.0).unwrap_err(),
            DspError::InvalidFormat
        );
        assert_eq!(
            DspProcessor::new(48_000, 9, &EqualizerSettings::default(), 1.0).unwrap_err(),
            DspError::InvalidFormat
        );
    }

    #[test]
    fn rejects_invalid_formats_and_misaligned_buffers() {
        assert_eq!(
            DspProcessor::new(0, 2, &EqualizerSettings::default(), 1.0).unwrap_err(),
            DspError::InvalidFormat
        );
        let mut processor =
            DspProcessor::new(48_000, 2, &EqualizerSettings::default(), 1.0).unwrap();
        assert_eq!(
            processor.process_interleaved(&mut [0.0]),
            Err(DspError::MisalignedBuffer)
        );
    }

    #[test]
    fn echo_delays_audio_and_preserves_state_across_buffers() {
        let mut echo = EchoProcessor::new(1_000, 1, 2, 0.5, 1.0).unwrap();
        let mut first = [1.0, 0.0];
        let mut second = [0.0, 0.0];
        echo.process_interleaved(&mut first).unwrap();
        echo.process_interleaved(&mut second).unwrap();
        assert!((second[0] - 1.0).abs() < 1e-6);
        let mut third = [0.0, 0.0];
        echo.process_interleaved(&mut third).unwrap();
        assert!((third[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn echo_rejects_invalid_settings_and_misaligned_pcm() {
        assert_eq!(
            EchoProcessor::new(48_000, 2, 0, 0.5, 0.5).unwrap_err(),
            DspError::InvalidEffectSettings
        );
        let mut echo = EchoProcessor::new(48_000, 2, 250, 0.5, 0.5).unwrap();
        assert_eq!(
            echo.process_interleaved(&mut [0.0]),
            Err(DspError::MisalignedBuffer)
        );
    }

    #[test]
    fn equal_power_crossfade_has_correct_endpoints_and_midpoint() {
        let outgoing = [1.0, 1.0];
        let incoming = [0.0, 0.0];
        let mut output = [0.0; 2];
        mix_equal_power_crossfade(&outgoing, &incoming, 0.0, &mut output).unwrap();
        assert!((output[0] - 1.0).abs() < 1.0e-6);
        mix_equal_power_crossfade(&outgoing, &incoming, 1.0, &mut output).unwrap();
        assert!(output[0].abs() < 1.0e-6);
        mix_equal_power_crossfade(&outgoing, &incoming, 0.5, &mut output).unwrap();
        assert!((output[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-6);
    }

    #[test]
    fn crossfade_rejects_mismatched_buffers_and_clamps_progress() {
        let mut output = [0.0; 2];
        assert_eq!(
            mix_equal_power_crossfade(&[1.0], &[1.0, 0.0], 0.5, &mut output),
            Err(DspError::InvalidCrossfadeBuffers)
        );
        mix_equal_power_crossfade(&[1.0, 1.0], &[0.0, 0.0], 2.0, &mut output).unwrap();
        assert!(output.iter().all(|sample| sample.abs() < 1.0e-6));
    }
}
