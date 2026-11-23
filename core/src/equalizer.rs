//! Platform-neutral equalizer settings. Native backends apply them via
//! `BackendAction::SetEqualizer`; the core only validates and stores them.

use serde::{Deserialize, Serialize};

pub const EQ_MIN_GAIN_DB: f32 = -12.0;
pub const EQ_MAX_GAIN_DB: f32 = 12.0;
pub const EQ_MAX_BANDS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqualizerBand {
    pub frequency_hz: u32,
    pub gain_db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqualizerPreset {
    Flat,
    BassBoost,
    TrebleBoost,
    Vocal,
    Rock,
    Classical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqualizerSettings {
    pub enabled: bool,
    pub bands: Vec<EqualizerBand>,
}

const DEFAULT_FREQUENCIES: [u32; 10] = [31, 62, 125, 250, 500, 1_000, 2_000, 4_000, 8_000, 16_000];

impl Default for EqualizerSettings {
    /// Ten flat bands, disabled.
    fn default() -> Self {
        Self::from_gains(false, [0.0; 10])
    }
}

impl EqualizerSettings {
    fn from_gains(enabled: bool, gains: [f32; 10]) -> Self {
        Self {
            enabled,
            bands: DEFAULT_FREQUENCIES
                .iter()
                .zip(gains)
                .map(|(&frequency_hz, gain_db)| EqualizerBand {
                    frequency_hz,
                    gain_db,
                })
                .collect(),
        }
    }

    /// Ready-made ten-band settings. `Flat` is returned enabled-but-neutral.
    pub fn preset(preset: EqualizerPreset) -> Self {
        let gains = match preset {
            EqualizerPreset::Flat => [0.0; 10],
            EqualizerPreset::BassBoost => [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            EqualizerPreset::TrebleBoost => [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 5.0, 6.0],
            EqualizerPreset::Vocal => [-2.0, -2.0, -1.0, 1.0, 3.0, 3.0, 2.0, 1.0, 0.0, -1.0],
            EqualizerPreset::Rock => [4.0, 3.0, 2.0, -1.0, -2.0, -1.0, 2.0, 3.0, 4.0, 4.0],
            EqualizerPreset::Classical => [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, -2.0, -2.0, -3.0],
        };
        Self::from_gains(true, gains)
    }

    /// Check that every band is usable by a native backend.
    pub fn is_valid(&self) -> bool {
        !self.bands.is_empty()
            && self.bands.len() <= EQ_MAX_BANDS
            && self.bands.iter().all(|band| {
                band.frequency_hz > 0
                    && band.gain_db.is_finite()
                    && (EQ_MIN_GAIN_DB..=EQ_MAX_GAIN_DB).contains(&band.gain_db)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_flat_disabled_and_valid() {
        let eq = EqualizerSettings::default();
        assert!(!eq.enabled);
        assert_eq!(eq.bands.len(), 10);
        assert!(eq.is_valid());
    }

    #[test]
    fn all_presets_are_valid_and_enabled() {
        for preset in [
            EqualizerPreset::Flat,
            EqualizerPreset::BassBoost,
            EqualizerPreset::TrebleBoost,
            EqualizerPreset::Vocal,
            EqualizerPreset::Rock,
            EqualizerPreset::Classical,
        ] {
            let eq = EqualizerSettings::preset(preset);
            assert!(eq.enabled && eq.is_valid(), "{preset:?}");
        }
    }

    #[test]
    fn rejects_out_of_range_nan_and_empty_bands() {
        let mut eq = EqualizerSettings::default();
        eq.bands[0].gain_db = 13.0;
        assert!(!eq.is_valid());
        eq.bands[0].gain_db = f32::NAN;
        assert!(!eq.is_valid());
        eq.bands[0].gain_db = 0.0;
        eq.bands[1].frequency_hz = 0;
        assert!(!eq.is_valid());
        eq.bands.clear();
        assert!(!eq.is_valid());
    }
}
