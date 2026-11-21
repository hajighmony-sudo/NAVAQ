//! Personalized spatial audio settings.
//!
//! Models a virtual listening room (size, absorption, speaker layout) so the
//! host can render true spatial / binaural audio that matches the user's
//! real environment or preferred virtual space. Complements (does not replace)
//! system-level Atmos / spatializers.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Dimensions of the virtual room in metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoomDimensions {
    pub width: f32,
    pub depth: f32,
    pub height: f32,
}

impl RoomDimensions {
    pub fn is_valid(&self) -> bool {
        self.width > 0.5
            && self.width <= 30.0
            && self.depth > 0.5
            && self.depth <= 30.0
            && self.height > 1.5
            && self.height <= 10.0
    }

    pub fn volume(&self) -> f32 {
        self.width * self.depth * self.height
    }
}

/// Approximate acoustic absorption of surfaces (0.0 = reflective, 1.0 = fully absorbent).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Absorption {
    pub walls: f32,
    pub floor: f32,
    pub ceiling: f32,
}

impl Default for Absorption {
    fn default() -> Self {
        // Typical furnished living-room values.
        Self {
            walls: 0.3,
            floor: 0.4,
            ceiling: 0.25,
        }
    }
}

impl Absorption {
    pub fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.walls)
            && (0.0..=1.0).contains(&self.floor)
            && (0.0..=1.0).contains(&self.ceiling)
    }
}

/// Position of a virtual loudspeaker relative to the listener (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpeakerPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl SpeakerPosition {
    pub fn is_valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.z.is_finite()
            && self.x.abs() <= 30.0
            && self.y.abs() <= 10.0
            && self.z.abs() <= 30.0
    }
}

/// Common speaker layouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpeakerLayout {
    /// Classic stereo pair.
    Stereo {
        left: SpeakerPosition,
        right: SpeakerPosition,
    },
    /// 5.1-ish (no LFE position needed for core logic).
    Surround5_1 {
        front_left: SpeakerPosition,
        front_right: SpeakerPosition,
        center: SpeakerPosition,
        surround_left: SpeakerPosition,
        surround_right: SpeakerPosition,
    },
    /// Fully custom set of speakers.
    Custom { speakers: Vec<SpeakerPosition> },
}

impl SpeakerLayout {
    pub fn is_valid(&self) -> bool {
        match self {
            SpeakerLayout::Stereo { left, right } => left.is_valid() && right.is_valid(),
            SpeakerLayout::Surround5_1 {
                front_left,
                front_right,
                center,
                surround_left,
                surround_right,
            } => [
                front_left,
                front_right,
                center,
                surround_left,
                surround_right,
            ]
            .iter()
            .all(|p| p.is_valid()),
            SpeakerLayout::Custom { speakers } => {
                !speakers.is_empty()
                    && speakers.len() <= 32
                    && speakers.iter().all(|p| p.is_valid())
            }
        }
    }

    /// Sensible default stereo layout (listener at origin, speakers at ±30°).
    pub fn default_stereo() -> Self {
        SpeakerLayout::Stereo {
            left: SpeakerPosition {
                x: -1.5,
                y: 0.0,
                z: 2.0,
            },
            right: SpeakerPosition {
                x: 1.5,
                y: 0.0,
                z: 2.0,
            },
        }
    }
}

/// Complete spatial audio configuration for the current user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialSettings {
    pub enabled: bool,
    pub room: RoomDimensions,
    pub absorption: Absorption,
    pub layout: SpeakerLayout,
    /// Listener head position inside the room (usually near centre).
    pub listener: SpeakerPosition,
    /// Head-related transfer function profile id (host-specific, e.g. "default", "measured_01").
    pub hrtf_profile: Option<String>,
    /// Extra reverb wet level 0.0..=1.0 for the spatializer.
    pub reverb_mix: f32,
}

impl Default for SpatialSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            room: RoomDimensions {
                width: 4.5,
                depth: 5.0,
                height: 2.6,
            },
            absorption: Absorption::default(),
            layout: SpeakerLayout::default_stereo(),
            listener: SpeakerPosition {
                x: 0.0,
                y: 1.2,
                z: 0.0,
            },
            hrtf_profile: None,
            reverb_mix: 0.25,
        }
    }
}

impl SpatialSettings {
    /// Approximate reverberation time using the Sabine formula (seconds).
    pub fn estimated_rt60_seconds(&self) -> f32 {
        let w = self.room.width;
        let d = self.room.depth;
        let h = self.room.height;
        if !self.room.is_valid() || !self.absorption.is_valid() {
            return 0.0;
        }
        let area = 2.0
            * (w * d * self.absorption.floor
                + w * d * self.absorption.ceiling
                + w * h * self.absorption.walls
                + d * h * self.absorption.walls);
        if area <= f32::EPSILON {
            0.0
        } else {
            0.161 * self.room.volume() / area
        }
    }

    pub fn is_valid(&self) -> bool {
        self.room.is_valid()
            && self.absorption.is_valid()
            && self.layout.is_valid()
            && (0.0..=1.0).contains(&self.reverb_mix)
            && self.listener.is_valid()
            && self.listener.x.abs() <= self.room.width * 0.5
            && self.listener.y >= 0.0
            && self.listener.y <= self.room.height
            && self.listener.z.abs() <= self.room.depth * 0.5
            && self.hrtf_profile.as_ref().map_or(true, |profile| {
                !profile.trim().is_empty() && profile.len() <= 128
            })
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum SpatialError {
    #[error("invalid spatial settings")]
    Invalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid() {
        let s = SpatialSettings::default();
        assert!(s.is_valid());
        assert!(!s.enabled);
        assert!(s.room.volume() > 0.0);
    }

    #[test]
    fn invalid_room_rejected() {
        let mut s = SpatialSettings::default();
        s.room.width = 0.1;
        assert!(!s.is_valid());
    }

    #[test]
    fn spatial_validation_rejects_non_finite_speakers_and_out_of_room_listener() {
        let mut settings = SpatialSettings::default();
        if let SpeakerLayout::Stereo { left, .. } = &mut settings.layout {
            left.z = f32::NAN;
        }
        assert!(!settings.is_valid());
        settings = SpatialSettings::default();
        settings.listener.x = 100.0;
        assert!(!settings.is_valid());
    }

    #[test]
    fn estimated_rt60_is_finite_and_positive() {
        let settings = SpatialSettings::default();
        let rt60 = settings.estimated_rt60_seconds();
        assert!(rt60.is_finite() && rt60 > 0.0);
    }

    #[test]
    fn custom_layout_requires_speakers() {
        let layout = SpeakerLayout::Custom { speakers: vec![] };
        assert!(!layout.is_valid());
    }
}
