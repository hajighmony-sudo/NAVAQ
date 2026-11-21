//! Advanced haptic feedback patterns driven by audio events.
//!
//! The core describes patterns (intensity, duration, sharpness) that the
//! host maps onto the device vibrator, game controller, or wearable.
//! Patterns can be tied to drum hits, bass pulses, vocal onsets, etc.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A single haptic pulse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HapticPulse {
    /// Intensity 0.0 (off) ..= 1.0 (maximum).
    pub intensity: f32,
    /// Duration of the pulse in milliseconds.
    pub duration_ms: u32,
    /// Sharpness / transient character 0.0 (soft) ..= 1.0 (clicky).
    pub sharpness: f32,
    /// Optional delay before this pulse fires (for sequences).
    pub delay_ms: u32,
}

impl HapticPulse {
    pub fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.intensity)
            && (0.0..=1.0).contains(&self.sharpness)
            && self.duration_ms > 0
            && self.duration_ms <= 5000
            && self.delay_ms <= 10_000
    }
}

/// A named pattern composed of one or more pulses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HapticPattern {
    pub name: String,
    pub pulses: Vec<HapticPulse>,
}

impl HapticPattern {
    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty()
            && self.name.len() <= 64
            && !self.pulses.is_empty()
            && self.pulses.len() <= 64
            && self.pulses.iter().all(|p| p.is_valid())
    }

    /// Total length of the pattern including delays.
    pub fn total_duration_ms(&self) -> u32 {
        self.pulses.iter().fold(0u32, |total, pulse| {
            total
                .saturating_add(pulse.delay_ms)
                .saturating_add(pulse.duration_ms)
        })
    }
}

/// Mapping from musical events to haptic patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HapticTrigger {
    /// Strong bass / kick drum.
    BassHit,
    /// Snare or clap.
    Snare,
    /// Hi-hat or transient high-frequency event.
    HiHat,
    /// Vocal onset / phrase start.
    VocalOnset,
    /// Beat grid (every beat).
    Beat,
    /// User-defined custom trigger.
    Custom,
}

/// User-configurable haptic settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HapticSettings {
    /// Master enable.
    pub enabled: bool,
    /// Global intensity multiplier 0.0..=1.0.
    pub master_intensity: f32,
    /// Which triggers are active.
    pub active_triggers: Vec<HapticTrigger>,
    /// Optional custom patterns keyed by trigger name.
    pub custom_patterns: Vec<(HapticTrigger, HapticPattern)>,
}

impl Default for HapticSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            master_intensity: 0.7,
            active_triggers: vec![HapticTrigger::BassHit, HapticTrigger::Snare],
            custom_patterns: Vec::new(),
        }
    }
}

impl HapticSettings {
    pub fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.master_intensity)
            && self.active_triggers.len() <= 16
            && self.custom_patterns.len() <= 16
            && self.custom_patterns.iter().all(|(_, p)| {
                p.name.len() <= 64 && p.pulses.len() <= 64 && p.is_valid()
            })
    }

    /// Return the active pattern with intensity scaled for the current device.
    pub fn pulses_for(&self, trigger: HapticTrigger) -> Vec<HapticPulse> {
        if !self.is_valid()
            || !self.enabled
            || !self.active_triggers.contains(&trigger)
            || self.master_intensity <= 0.0
        {
            return Vec::new();
        }
        let pattern = self.pattern_for(trigger);
        if !pattern.is_valid() {
            return Vec::new();
        }
        pattern
            .pulses
            .into_iter()
            .map(|mut pulse| {
                pulse.intensity = (pulse.intensity * self.master_intensity).clamp(0.0, 1.0);
                pulse
            })
            .collect()
    }

    /// Built-in sensible defaults for common triggers.
    pub fn builtin_pattern(trigger: HapticTrigger) -> HapticPattern {
        match trigger {
            HapticTrigger::BassHit => HapticPattern {
                name: "bass_hit".into(),
                pulses: vec![HapticPulse {
                    intensity: 0.9,
                    duration_ms: 80,
                    sharpness: 0.3,
                    delay_ms: 0,
                }],
            },
            HapticTrigger::Snare => HapticPattern {
                name: "snare".into(),
                pulses: vec![HapticPulse {
                    intensity: 0.7,
                    duration_ms: 40,
                    sharpness: 0.85,
                    delay_ms: 0,
                }],
            },
            HapticTrigger::HiHat => HapticPattern {
                name: "hihat".into(),
                pulses: vec![HapticPulse {
                    intensity: 0.35,
                    duration_ms: 25,
                    sharpness: 0.95,
                    delay_ms: 0,
                }],
            },
            HapticTrigger::VocalOnset => HapticPattern {
                name: "vocal".into(),
                pulses: vec![HapticPulse {
                    intensity: 0.5,
                    duration_ms: 60,
                    sharpness: 0.4,
                    delay_ms: 0,
                }],
            },
            HapticTrigger::Beat => HapticPattern {
                name: "beat".into(),
                pulses: vec![HapticPulse {
                    intensity: 0.45,
                    duration_ms: 30,
                    sharpness: 0.6,
                    delay_ms: 0,
                }],
            },
            HapticTrigger::Custom => HapticPattern {
                name: "custom".into(),
                pulses: vec![HapticPulse {
                    intensity: 0.6,
                    duration_ms: 50,
                    sharpness: 0.5,
                    delay_ms: 0,
                }],
            },
        }
    }

    pub fn pattern_for(&self, trigger: HapticTrigger) -> HapticPattern {
        self.custom_patterns
            .iter()
            .find(|(t, _)| *t == trigger)
            .map(|(_, p)| p.clone())
            .unwrap_or_else(|| Self::builtin_pattern(trigger))
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum HapticError {
    #[error("invalid haptic settings or pattern")]
    Invalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_valid() {
        let s = HapticSettings::default();
        assert!(s.is_valid());
        assert!(!s.enabled);
    }

    #[test]
    fn builtin_patterns_are_valid() {
        for t in [
            HapticTrigger::BassHit,
            HapticTrigger::Snare,
            HapticTrigger::HiHat,
            HapticTrigger::VocalOnset,
            HapticTrigger::Beat,
        ] {
            let p = HapticSettings::builtin_pattern(t);
            assert!(p.is_valid(), "{:?} invalid", t);
            assert!(p.total_duration_ms() > 0);
        }
    }

    #[test]
    fn disabled_or_inactive_trigger_emits_no_haptics_and_enabled_scales_intensity() {
        let mut settings = HapticSettings::default();
        assert!(settings.pulses_for(HapticTrigger::BassHit).is_empty());
        settings.enabled = true;
        settings.master_intensity = 0.5;
        let pulses = settings.pulses_for(HapticTrigger::BassHit);
        assert_eq!(pulses.len(), 1);
        assert!((pulses[0].intensity - 0.45).abs() < 1e-6);
        assert!(settings.pulses_for(HapticTrigger::HiHat).is_empty());
    }
    #[test]
    fn invalid_or_oversized_custom_patterns_are_rejected() {
        let mut settings = HapticSettings::default();
        settings.custom_patterns.push((
            HapticTrigger::BassHit,
            HapticPattern {
                name: "x".repeat(65),
                pulses: vec![HapticPulse {
                    intensity: 1.0,
                    duration_ms: 10,
                    sharpness: 0.5,
                    delay_ms: 0,
                }],
            },
        ));
        assert!(!settings.is_valid());
        assert!(settings.pulses_for(HapticTrigger::BassHit).is_empty());
    }

    #[test]
    fn custom_pattern_overrides() {
        let mut s = HapticSettings::default();
        let custom = HapticPattern {
            name: "heavy_bass".into(),
            pulses: vec![HapticPulse {
                intensity: 1.0,
                duration_ms: 120,
                sharpness: 0.2,
                delay_ms: 0,
            }],
        };
        s.custom_patterns
            .push((HapticTrigger::BassHit, custom.clone()));
        assert_eq!(s.pattern_for(HapticTrigger::BassHit), custom);
    }
}
