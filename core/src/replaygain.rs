//! ReplayGain-style loudness normalization math. The host reads the tags; the core turns
//! them into a linear gain that a backend can apply on top of the user's volume.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayGainMode {
    #[default]
    Off,
    Track,
    Album,
}

/// Gain values read from a file's tags (dB for gains, linear 0..=1+ for peaks).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReplayGainTags {
    pub track_gain_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayGainSettings {
    pub mode: ReplayGainMode,
    /// Added to the tagged gain.
    pub preamp_db: f32,
    /// Gain used for tracks that carry no usable tag.
    pub untagged_gain_db: f32,
    /// Never boost a track past full scale according to its peak tag.
    pub prevent_clipping: bool,
}

impl Default for ReplayGainSettings {
    fn default() -> Self {
        Self {
            mode: ReplayGainMode::Off,
            preamp_db: 0.0,
            untagged_gain_db: 0.0,
            prevent_clipping: true,
        }
    }
}

const MAX_LINEAR_GAIN: f32 = 4.0;

fn finite(value: Option<f32>) -> Option<f32> {
    value.filter(|v| v.is_finite())
}

/// Linear multiplier (1.0 = unchanged) for a track under `settings`.
/// Track mode falls back to album tags and vice versa.
pub fn linear_gain(tags: &ReplayGainTags, settings: &ReplayGainSettings) -> f32 {
    let (gain_db, peak) = match settings.mode {
        ReplayGainMode::Off => return 1.0,
        ReplayGainMode::Track => match finite(tags.track_gain_db) {
            Some(gain) => (Some(gain), finite(tags.track_peak)),
            None => (finite(tags.album_gain_db), finite(tags.album_peak)),
        },
        ReplayGainMode::Album => match finite(tags.album_gain_db) {
            Some(gain) => (Some(gain), finite(tags.album_peak)),
            None => (finite(tags.track_gain_db), finite(tags.track_peak)),
        },
    };

    let db = gain_db.unwrap_or(settings.untagged_gain_db) + settings.preamp_db;
    let mut linear = if db.is_finite() {
        10f32.powf(db / 20.0)
    } else {
        1.0
    };
    if settings.prevent_clipping {
        if let Some(peak) = peak.filter(|p| *p > 0.0) {
            linear = linear.min(1.0 / peak);
        }
    }
    linear.clamp(0.0, MAX_LINEAR_GAIN)
}

/// Volume to send to the backend: the user's volume scaled by the normalization gain.
pub fn effective_volume(user_volume: f32, linear_gain: f32) -> f32 {
    (user_volume * linear_gain).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mode: ReplayGainMode) -> ReplayGainSettings {
        ReplayGainSettings {
            mode,
            ..ReplayGainSettings::default()
        }
    }

    #[test]
    fn off_is_unity() {
        let tags = ReplayGainTags {
            track_gain_db: Some(-9.0),
            ..Default::default()
        };
        assert_eq!(linear_gain(&tags, &settings(ReplayGainMode::Off)), 1.0);
    }

    #[test]
    fn converts_db_to_linear() {
        let tags = ReplayGainTags {
            track_gain_db: Some(-6.0206),
            ..Default::default()
        };
        let gain = linear_gain(&tags, &settings(ReplayGainMode::Track));
        assert!((gain - 0.5).abs() < 1e-3);
    }

    #[test]
    fn falls_back_between_track_and_album_tags() {
        let tags = ReplayGainTags {
            album_gain_db: Some(-6.0206),
            ..Default::default()
        };
        let gain = linear_gain(&tags, &settings(ReplayGainMode::Track));
        assert!((gain - 0.5).abs() < 1e-3);
    }

    #[test]
    fn peak_prevents_clipping_but_can_be_disabled() {
        let tags = ReplayGainTags {
            track_gain_db: Some(6.0206),
            track_peak: Some(0.8),
            ..Default::default()
        };
        let limited = linear_gain(&tags, &settings(ReplayGainMode::Track));
        assert!((limited - 1.25).abs() < 1e-3);
        let mut free = settings(ReplayGainMode::Track);
        free.prevent_clipping = false;
        assert!((linear_gain(&tags, &free) - 2.0).abs() < 1e-2);
    }

    #[test]
    fn untagged_uses_fallback_plus_preamp_and_invalid_tags_are_ignored() {
        let mut s = settings(ReplayGainMode::Track);
        s.untagged_gain_db = -6.0206;
        s.preamp_db = 6.0206;
        let nan = ReplayGainTags {
            track_gain_db: Some(f32::NAN),
            ..Default::default()
        };
        assert!((linear_gain(&nan, &s) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn effective_volume_is_clamped() {
        assert_eq!(effective_volume(0.9, 2.0), 1.0);
        assert!((effective_volume(0.5, 0.5) - 0.25).abs() < 1e-6);
    }
}
