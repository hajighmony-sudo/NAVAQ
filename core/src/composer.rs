//! Manual, editable music composition primitives.
//!
//! This module stores a deterministic note/clip arrangement. It does not synthesize
//! audio itself; a platform renderer can turn the arrangement into PCM or MIDI.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Instrument {
    Piano,
    ElectricPiano,
    SynthLead,
    SynthPad,
    Bass,
    Drums,
    Strings,
    Guitar,
    Pluck,
    Flute,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteEvent {
    /// MIDI note number, 0..=127.
    pub pitch: u8,
    /// Start position in quarter-note beats from the start of the arrangement.
    pub start_beat: f32,
    /// Duration in quarter-note beats.
    pub duration_beats: f32,
    /// Velocity, 0..=127.
    pub velocity: u8,
}

impl NoteEvent {
    pub fn is_valid(&self) -> bool {
        self.pitch <= 127
            && self.start_beat.is_finite()
            && self.start_beat >= 0.0
            && self.duration_beats.is_finite()
            && self.duration_beats > 0.0
            && self.duration_beats <= 64.0
    }

    pub fn end_beat(&self) -> f32 {
        self.start_beat + self.duration_beats
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentTrack {
    pub id: String,
    pub name: String,
    pub instrument: Instrument,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub notes: Vec<NoteEvent>,
}

impl InstrumentTrack {
    pub fn is_valid(&self) -> bool {
        !self.id.trim().is_empty()
            && !self.name.trim().is_empty()
            && self.volume.is_finite()
            && (0.0..=1.0).contains(&self.volume)
            && self.pan.is_finite()
            && (-1.0..=1.0).contains(&self.pan)
            && self.notes.len() <= 100_000
            && self.notes.iter().all(NoteEvent::is_valid)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Composition {
    pub title: String,
    pub tempo_bpm: f32,
    pub numerator: u8,
    pub denominator: u8,
    pub key: String,
    pub bars: u16,
    pub tracks: Vec<InstrumentTrack>,
}

impl Composition {
    pub fn is_valid(&self) -> bool {
        !self.title.trim().is_empty()
            && self.tempo_bpm.is_finite()
            && (30.0..=300.0).contains(&self.tempo_bpm)
            && (1..=16).contains(&self.numerator)
            && matches!(self.denominator, 2 | 4 | 8 | 16)
            && !self.key.trim().is_empty()
            && (1..=512).contains(&self.bars)
            && !self.tracks.is_empty()
            && self.tracks.len() <= 64
            && self.tracks.iter().all(InstrumentTrack::is_valid)
            && self
                .tracks
                .iter()
                .flat_map(|t| &t.notes)
                .all(|note| note.end_beat() <= self.total_beats() + 1.0e-4)
    }

    /// Arrangement length in quarter-note beats.
    pub fn total_beats(&self) -> f32 {
        self.bars as f32 * self.numerator as f32 * (4.0 / self.denominator as f32)
    }

    pub fn duration_ms(&self) -> u64 {
        ((self.total_beats() as f64 * 60_000.0) / self.tempo_bpm as f64).round() as u64
    }

    pub fn add_track(&mut self, track: InstrumentTrack) -> Result<(), ComposerError> {
        if !track.is_valid() {
            return Err(ComposerError::InvalidTrack);
        }
        if self.tracks.iter().any(|existing| existing.id == track.id) {
            return Err(ComposerError::DuplicateTrack);
        }
        if self.tracks.len() >= 64 {
            return Err(ComposerError::TooManyTracks);
        }
        self.tracks.push(track);
        Ok(())
    }

    pub fn remove_track(&mut self, id: &str) -> Option<InstrumentTrack> {
        let index = self.tracks.iter().position(|track| track.id == id)?;
        Some(self.tracks.remove(index))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ComposerError {
    #[error("instrument track is invalid")]
    InvalidTrack,
    #[error("track id already exists")]
    DuplicateTrack,
    #[error("composition has reached the track limit")]
    TooManyTracks,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composition() -> Composition {
        Composition {
            title: "Midnight".into(),
            tempo_bpm: 120.0,
            numerator: 4,
            denominator: 4,
            key: "A minor".into(),
            bars: 8,
            tracks: vec![InstrumentTrack {
                id: "piano".into(),
                name: "Piano".into(),
                instrument: Instrument::Piano,
                volume: 0.8,
                pan: 0.0,
                muted: false,
                notes: vec![NoteEvent {
                    pitch: 60,
                    start_beat: 0.0,
                    duration_beats: 1.0,
                    velocity: 100,
                }],
            }],
        }
    }

    #[test]
    fn validates_manual_arrangement_and_duration() {
        let arrangement = composition();
        assert!(arrangement.is_valid());
        assert_eq!(arrangement.total_beats(), 32.0);
        assert_eq!(arrangement.duration_ms(), 16_000);
    }

    #[test]
    fn rejects_notes_outside_arrangement_and_invalid_values() {
        let mut arrangement = composition();
        arrangement.tracks[0].notes[0].start_beat = f32::NAN;
        assert!(!arrangement.is_valid());
        arrangement.tracks[0].notes[0].start_beat = 32.0;
        assert!(!arrangement.is_valid());
    }

    #[test]
    fn track_management_checks_ids_and_limits() {
        let mut arrangement = composition();
        let duplicate = arrangement.tracks[0].clone();
        assert_eq!(
            arrangement.add_track(duplicate),
            Err(ComposerError::DuplicateTrack)
        );
        assert!(arrangement.remove_track("piano").is_some());
        assert!(arrangement.tracks.is_empty());
    }
}
