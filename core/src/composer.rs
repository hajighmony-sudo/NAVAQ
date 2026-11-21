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
            && (1..=127).contains(&self.velocity)
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

    /// Export this arrangement as a Standard MIDI File (format 1).
    pub fn to_midi_bytes(&self) -> Result<Vec<u8>, ComposerError> {
        if !self.is_valid() {
            return Err(ComposerError::InvalidTrack);
        }
        if self.tracks.len() > 15 {
            return Err(ComposerError::TooManyMidiTracks);
        }
        let mut conductor = Vec::new();
        let micros_per_beat = (60_000_000.0 / self.tempo_bpm).round() as u32;
        conductor.extend_from_slice(&[
            0x00,
            0xFF,
            0x51,
            0x03,
            ((micros_per_beat >> 16) & 0xFF) as u8,
            ((micros_per_beat >> 8) & 0xFF) as u8,
            (micros_per_beat & 0xFF) as u8,
        ]);
        let denominator_power = self.denominator.trailing_zeros() as u8;
        conductor.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, self.numerator, denominator_power, 24, 8]);
        conductor.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        let mut chunks = Vec::with_capacity(self.tracks.len() + 1);
        chunks.push(midi_track_chunk(conductor));
        for (index, track) in self.tracks.iter().enumerate() {
            let channel = if track.instrument == Instrument::Drums {
                9
            } else if index >= 9 {
                index as u8 + 1
            } else {
                index as u8
            };
            let mut events: Vec<(u32, u8, [u8; 3], usize)> = Vec::new();
            events.push((0, 0, [0xC0 | channel, track.instrument.program(), 0], 2));
            events.push((
                0,
                1,
                [0xB0 | channel, 7, (track.volume * 127.0).round() as u8],
                3,
            ));
            events.push((
                0,
                2,
                [0xB0 | channel, 10, ((track.pan + 1.0) * 63.5).round() as u8],
                3,
            ));
            if !track.muted {
                for note in &track.notes {
                    let start = (note.start_beat * 480.0).round() as u32;
                    let end = (note.end_beat() * 480.0).round() as u32;
                    events.push((end, 0, [0x80 | channel, note.pitch, 0], 3));
                    events.push((start, 1, [0x90 | channel, note.pitch, note.velocity], 3));
                }
            }
            events.sort_by_key(|event| (event.0, event.1));
            let mut data = Vec::new();
            let mut previous_tick = 0u32;
            for (tick, _, bytes, length) in events {
                push_variable_length(&mut data, tick.saturating_sub(previous_tick));
                data.extend_from_slice(&bytes[..length]);
                previous_tick = tick;
            }
            data.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
            chunks.push(midi_track_chunk(data));
        }
        let mut output = Vec::new();
        output.extend_from_slice(b"MThd");
        output.extend_from_slice(&6u32.to_be_bytes());
        output.extend_from_slice(&1u16.to_be_bytes());
        output.extend_from_slice(&(chunks.len() as u16).to_be_bytes());
        output.extend_from_slice(&480u16.to_be_bytes());
        for chunk in chunks {
            output.extend_from_slice(&chunk);
        }
        Ok(output)
    }

    pub fn remove_track(&mut self, id: &str) -> Option<InstrumentTrack> {
        let index = self.tracks.iter().position(|track| track.id == id)?;
        Some(self.tracks.remove(index))
    }
}

fn midi_track_chunk(data: Vec<u8>) -> Vec<u8> {
    let mut chunk = Vec::with_capacity(data.len() + 8);
    chunk.extend_from_slice(b"MTrk");
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(&data);
    chunk
}

fn push_variable_length(output: &mut Vec<u8>, mut value: u32) {
    let mut buffer = [0u8; 4];
    let mut index = 3;
    buffer[index] = (value & 0x7F) as u8;
    while { value >>= 7; value != 0 } {
        index -= 1;
        buffer[index] = ((value & 0x7F) as u8) | 0x80;
    }
    output.extend_from_slice(&buffer[index..]);
}

impl Instrument {
    fn program(self) -> u8 {
        match self {
            Instrument::Piano => 0,
            Instrument::ElectricPiano => 4,
            Instrument::SynthLead => 80,
            Instrument::SynthPad => 89,
            Instrument::Bass => 38,
            Instrument::Drums => 0,
            Instrument::Strings => 48,
            Instrument::Guitar => 24,
            Instrument::Pluck => 10,
            Instrument::Flute => 73,
        }
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
    #[error("MIDI export supports at most 15 instrument tracks")]
    TooManyMidiTracks,
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
    fn exports_a_standard_midi_file() {
        let bytes = composition().to_midi_bytes().unwrap();
        assert_eq!(&bytes[..4], b"MThd");
        assert_eq!(&bytes[8..10], &[0, 1]);
        assert_eq!(&bytes[10..12], &[0, 2]);
        assert!(bytes.windows(4).any(|window| window == b"MTrk"));
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
