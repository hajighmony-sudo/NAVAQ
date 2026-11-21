//! Stateful, platform-independent PCM processing pipeline.
//!
//! A native decoder feeds interleaved f32 PCM into this pipeline before sending
//! samples to the output device. It combines ReplayGain/master gain, the
//! parametric equalizer, optional echo, and an optional peak ceiling. It does
//! not decode files, own an audio device, or implement binaural spatial audio.

use crate::dsp::{DspError, DspProcessor, EchoProcessor};
use crate::equalizer::EqualizerSettings;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoSettings {
    pub delay_ms: u32,
    pub feedback: f32,
    pub mix: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PcmPipelineSettings {
    pub equalizer: EqualizerSettings,
    /// Linear ReplayGain multiplier. Use 1.0 when tags are unavailable.
    pub replay_gain: f32,
    /// User master-gain multiplier.
    pub master_gain: f32,
    pub echo: Option<EchoSettings>,
    /// Optional output peak ceiling. None preserves headroom without limiting.
    pub peak_ceiling: Option<f32>,
}

impl Default for PcmPipelineSettings {
    fn default() -> Self {
        Self {
            equalizer: EqualizerSettings::default(),
            replay_gain: 1.0,
            master_gain: 1.0,
            echo: None,
            peak_ceiling: Some(1.0),
        }
    }
}

impl PcmPipelineSettings {
    pub fn is_valid(&self) -> bool {
        self.equalizer.is_valid()
            && self.replay_gain.is_finite()
            && (0.0..=4.0).contains(&self.replay_gain)
            && self.master_gain.is_finite()
            && (0.0..=4.0).contains(&self.master_gain)
            && self.replay_gain * self.master_gain <= 4.0
            && self
                .peak_ceiling
                .map_or(true, |v| v.is_finite() && (0.1..=1.0).contains(&v))
            && self.echo.map_or(true, |echo| {
                (1..=2_000).contains(&echo.delay_ms)
                    && echo.feedback.is_finite()
                    && (0.0..=0.95).contains(&echo.feedback)
                    && echo.mix.is_finite()
                    && (0.0..=1.0).contains(&echo.mix)
            })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PcmProcessStats {
    /// Peak absolute sample after processing and optional limiting.
    pub peak: f32,
    /// Root-mean-square level after processing.
    pub rms: f32,
    /// Number of samples reduced by the configured peak ceiling.
    pub limited_samples: usize,
}

#[derive(Debug, Error, PartialEq)]
pub enum PcmPipelineError {
    #[error("PCM format is invalid")]
    InvalidFormat,
    #[error("PCM pipeline settings are invalid")]
    InvalidSettings,
    #[error(transparent)]
    Dsp(#[from] DspError),
}

/// Reusable PCM chain. Keep one instance alive across decoder callbacks so
/// equalizer and echo histories remain continuous and do not click at buffer edges.
#[derive(Debug, Clone)]
pub struct PcmAudioPipeline {
    format: PcmFormat,
    settings: PcmPipelineSettings,
    equalizer: DspProcessor,
    echo: Option<EchoProcessor>,
}

impl PcmAudioPipeline {
    pub fn new(format: PcmFormat, settings: PcmPipelineSettings) -> Result<Self, PcmPipelineError> {
        if format.sample_rate == 0
            || format.sample_rate > 384_000
            || format.channels == 0
            || format.channels > 8
        {
            return Err(PcmPipelineError::InvalidFormat);
        }
        if !settings.is_valid() {
            return Err(PcmPipelineError::InvalidSettings);
        }
        let gain = settings.replay_gain * settings.master_gain;
        let equalizer = DspProcessor::new(
            format.sample_rate,
            format.channels,
            &settings.equalizer,
            gain,
        )?;
        let echo = settings
            .echo
            .map(|value| {
                EchoProcessor::new(
                    format.sample_rate,
                    format.channels,
                    value.delay_ms,
                    value.feedback,
                    value.mix,
                )
            })
            .transpose()?;

        Ok(Self {
            format,
            settings,
            equalizer,
            echo,
        })
    }

    pub fn format(&self) -> PcmFormat {
        self.format
    }

    pub fn settings(&self) -> &PcmPipelineSettings {
        &self.settings
    }

    /// Replace settings. Rebuilding the filters resets their delay histories;
    /// hosts should apply this between PCM buffers, never midway through a buffer.
    pub fn set_settings(&mut self, settings: PcmPipelineSettings) -> Result<(), PcmPipelineError> {
        if !settings.is_valid() {
            return Err(PcmPipelineError::InvalidSettings);
        }
        let gain = settings.replay_gain * settings.master_gain;
        let equalizer = DspProcessor::new(
            self.format.sample_rate,
            self.format.channels,
            &settings.equalizer,
            gain,
        )?;
        let echo = settings
            .echo
            .map(|value| {
                EchoProcessor::new(
                    self.format.sample_rate,
                    self.format.channels,
                    value.delay_ms,
                    value.feedback,
                    value.mix,
                )
            })
            .transpose()?;

        self.settings = settings;
        self.equalizer = equalizer;
        self.echo = echo;
        Ok(())
    }

    /// Process one interleaved f32 buffer and return meter/limiter statistics.
    pub fn process_interleaved(
        &mut self,
        samples: &mut [f32],
    ) -> Result<PcmProcessStats, PcmPipelineError> {
        let channels = self.format.channels as usize;
        if samples.len() % channels != 0 {
            return Err(PcmPipelineError::Dsp(DspError::MisalignedBuffer));
        }

        self.equalizer.process_interleaved(samples)?;
        if let Some(echo) = &mut self.echo {
            echo.process_interleaved(samples)?;
        }

        let mut peak = 0.0_f32;
        let mut square_sum = 0.0_f64;
        let mut limited_samples = 0;
        let ceiling = self.settings.peak_ceiling;

        for sample in samples.iter_mut() {
            let finite = if sample.is_finite() { *sample } else { 0.0 };
            let output = if let Some(limit) = ceiling {
                let limited = finite.clamp(-limit, limit);
                if limited != finite {
                    limited_samples += 1;
                }
                limited
            } else {
                finite
            };
            *sample = output;
            peak = peak.max(output.abs());
            square_sum += (output as f64) * (output as f64);
        }

        let rms = if samples.is_empty() {
            0.0
        } else {
            (square_sum / samples.len() as f64).sqrt() as f32
        };

        Ok(PcmProcessStats {
            peak,
            rms,
            limited_samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equalizer::EqualizerPreset;

    fn format() -> PcmFormat {
        PcmFormat {
            sample_rate: 48_000,
            channels: 2,
        }
    }

    #[test]
    fn default_pipeline_preserves_quiet_pcm_and_reports_meters() {
        let mut pipeline = PcmAudioPipeline::new(format(), PcmPipelineSettings::default()).unwrap();
        let mut pcm = [0.25, -0.5, 0.75, -1.0];
        let stats = pipeline.process_interleaved(&mut pcm).unwrap();
        assert_eq!(pcm, [0.25, -0.5, 0.75, -1.0]);
        assert!((stats.peak - 1.0).abs() < 1e-6);
        assert!(stats.rms > 0.0);
        assert_eq!(stats.limited_samples, 0);
    }

    #[test]
    fn combines_replay_gain_and_master_gain_before_equalization() {
        let settings = PcmPipelineSettings {
            replay_gain: 0.5,
            master_gain: 0.5,
            peak_ceiling: None,
            ..PcmPipelineSettings::default()
        };
        let mut pipeline = PcmAudioPipeline::new(format(), settings).unwrap();
        let mut pcm = [1.0, -1.0];
        pipeline.process_interleaved(&mut pcm).unwrap();
        assert_eq!(pcm, [0.25, -0.25]);
    }

    #[test]
    fn peak_ceiling_prevents_over_range_samples() {
        let mut pipeline = PcmAudioPipeline::new(format(), PcmPipelineSettings::default()).unwrap();
        let mut pcm = [2.0, -1.5, 0.5, -0.5];
        let stats = pipeline.process_interleaved(&mut pcm).unwrap();
        assert_eq!(pcm, [1.0, -1.0, 0.5, -0.5]);
        assert_eq!(stats.limited_samples, 2);
        assert_eq!(stats.peak, 1.0);
    }

    #[test]
    fn echo_and_equalizer_can_be_combined() {
        let settings = PcmPipelineSettings {
            equalizer: EqualizerSettings::preset(EqualizerPreset::BassBoost),
            echo: Some(EchoSettings {
                delay_ms: 2,
                feedback: 0.4,
                mix: 0.2,
            }),
            ..PcmPipelineSettings::default()
        };
        let mut pipeline = PcmAudioPipeline::new(
            PcmFormat {
                sample_rate: 1_000,
                channels: 1,
            },
            settings,
        )
        .unwrap();
        let mut first = [0.5, 0.0];
        let mut second = [0.0, 0.0];
        pipeline.process_interleaved(&mut first).unwrap();
        pipeline.process_interleaved(&mut second).unwrap();
        assert!(first.iter().chain(second.iter()).all(|v| v.is_finite()));
    }

    #[test]
    fn rejects_invalid_settings_and_misaligned_pcm() {
        let invalid = PcmPipelineSettings {
            master_gain: f32::NAN,
            ..PcmPipelineSettings::default()
        };
        assert_eq!(
            PcmAudioPipeline::new(format(), invalid).unwrap_err(),
            PcmPipelineError::InvalidSettings
        );

        let mut pipeline = PcmAudioPipeline::new(format(), PcmPipelineSettings::default()).unwrap();
        assert_eq!(
            pipeline.process_interleaved(&mut [0.0]),
            Err(PcmPipelineError::Dsp(DspError::MisalignedBuffer))
        );
    }

    #[test]
    fn settings_update_is_applied_and_preserves_pipeline_format() {
        let mut pipeline = PcmAudioPipeline::new(format(), PcmPipelineSettings::default()).unwrap();
        let settings = PcmPipelineSettings {
            master_gain: 0.25,
            ..PcmPipelineSettings::default()
        };
        pipeline.set_settings(settings).unwrap();
        let mut pcm = [1.0, -1.0];
        pipeline.process_interleaved(&mut pcm).unwrap();
        assert_eq!(pcm, [0.25, -0.25]);
        assert_eq!(pipeline.format(), format());
    }
}
