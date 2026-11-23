//! Manual composition: write instrumentals note by note (or with a drum step sequencer),
//! then render them to audio. Everything is deterministic and runs offline in the core.
//!
//! A `Composition` is a tempo plus `Part`s (one instrument each). It can be rendered to
//! stereo PCM / a 16-bit WAV file with a small built-in synthesizer, or exported as a
//! Standard MIDI File. The host saves the bytes and plays the file like any other track.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MIN_TEMPO_BPM: f32 = 30.0;
pub const MAX_TEMPO_BPM: f32 = 300.0;
pub const MAX_NOTES: usize = 50_000;
/// Longest composition that can be rendered (memory guard).
pub const MAX_DURATION_SECS: f32 = 300.0;
const RELEASE_SECS: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Instrument {
    Sine,
    Square,
    Saw,
    Triangle,
    Pluck,
    Pad,
    Kick,
    Snare,
    HiHat,
}

impl Instrument {
    pub fn is_drum(self) -> bool {
        matches!(
            self,
            Instrument::Kick | Instrument::Snare | Instrument::HiHat
        )
    }

    /// General MIDI program number used when exporting.
    pub fn midi_program(self) -> u8 {
        match self {
            Instrument::Sine => 80,     // square lead (closest simple tone)
            Instrument::Square => 80,   // lead 1 (square)
            Instrument::Saw => 81,      // lead 2 (sawtooth)
            Instrument::Triangle => 73, // flute
            Instrument::Pluck => 24,    // nylon guitar
            Instrument::Pad => 89,      // warm pad
            _ => 0,
        }
    }

    /// General MIDI percussion key for drum instruments.
    pub fn drum_note(self) -> Option<u8> {
        match self {
            Instrument::Kick => Some(36),
            Instrument::Snare => Some(38),
            Instrument::HiHat => Some(42),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Start position in beats from the beginning.
    pub start_beats: f32,
    pub duration_beats: f32,
    /// MIDI note number (60 = middle C). Ignored for drum instruments.
    pub pitch: u8,
    /// 1..=127.
    pub velocity: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub name: String,
    pub instrument: Instrument,
    /// 0.0..=1.0
    pub volume: f32,
    /// -1.0 (left) ..= 1.0 (right)
    pub pan: f32,
    pub notes: Vec<Note>,
}

impl Part {
    pub fn new(name: &str, instrument: Instrument) -> Self {
        Self {
            name: name.to_owned(),
            instrument,
            volume: 0.8,
            pan: 0.0,
            notes: Vec::new(),
        }
    }

    pub fn add_note(&mut self, start_beats: f32, duration_beats: f32, pitch: u8, velocity: u8) {
        self.notes.push(Note {
            start_beats,
            duration_beats,
            pitch,
            velocity,
        });
    }

    pub fn add_chord(
        &mut self,
        start_beats: f32,
        duration_beats: f32,
        pitches: &[u8],
        velocity: u8,
    ) {
        for &pitch in pitches {
            self.add_note(start_beats, duration_beats, pitch, velocity);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Composition {
    pub title: String,
    pub tempo_bpm: f32,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CompositionError {
    #[error("tempo must be between 30 and 300 BPM")]
    InvalidTempo,
    #[error("part '{0}' has an invalid volume or pan")]
    InvalidPart(String),
    #[error("note {index} of part '{part}' is invalid")]
    InvalidNote { part: String, index: usize },
    #[error("too many notes (limit 50000)")]
    TooManyNotes,
    #[error("composition is empty or longer than 300 seconds")]
    BadLength,
    #[error("sample rate must be between 8000 and 48000 Hz")]
    InvalidSampleRate,
    #[error("invalid drum pattern")]
    InvalidPattern,
}

impl Composition {
    pub fn new(title: &str, tempo_bpm: f32) -> Self {
        Self {
            title: title.to_owned(),
            tempo_bpm,
            parts: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), CompositionError> {
        if !self.tempo_bpm.is_finite() || !(MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&self.tempo_bpm)
        {
            return Err(CompositionError::InvalidTempo);
        }
        let mut total = 0usize;
        for part in &self.parts {
            if !part.volume.is_finite()
                || !(0.0..=1.0).contains(&part.volume)
                || !part.pan.is_finite()
                || !(-1.0..=1.0).contains(&part.pan)
            {
                return Err(CompositionError::InvalidPart(part.name.clone()));
            }
            total += part.notes.len();
            if total > MAX_NOTES {
                return Err(CompositionError::TooManyNotes);
            }
            for (index, note) in part.notes.iter().enumerate() {
                let ok = note.start_beats.is_finite()
                    && note.start_beats >= 0.0
                    && note.duration_beats.is_finite()
                    && note.duration_beats > 0.0
                    && note.duration_beats <= 64.0
                    && note.pitch <= 127
                    && (1..=127).contains(&note.velocity);
                if !ok {
                    return Err(CompositionError::InvalidNote {
                        part: part.name.clone(),
                        index,
                    });
                }
            }
        }
        Ok(())
    }

    /// Length in beats (end of the last note).
    pub fn duration_beats(&self) -> f32 {
        self.parts
            .iter()
            .flat_map(|p| &p.notes)
            .map(|n| n.start_beats + n.duration_beats)
            .fold(0.0, f32::max)
    }

    pub fn duration_secs(&self) -> f32 {
        self.duration_beats() * 60.0 / self.tempo_bpm
    }

    pub fn duration_ms(&self) -> u64 {
        (self.duration_secs() * 1000.0).round() as u64
    }

    /// Shift every pitched note by `semitones` (clamped to the MIDI range). Drums are untouched.
    pub fn transpose(&mut self, semitones: i32) {
        for part in self.parts.iter_mut().filter(|p| !p.instrument.is_drum()) {
            for note in &mut part.notes {
                note.pitch = (i32::from(note.pitch) + semitones).clamp(0, 127) as u8;
            }
        }
    }

    /// Render to interleaved stereo f32 at `sample_rate_hz` (8000..=48000).
    pub fn render_pcm(&self, sample_rate_hz: u32) -> Result<Vec<f32>, CompositionError> {
        self.validate()?;
        if !(8_000..=48_000).contains(&sample_rate_hz) {
            return Err(CompositionError::InvalidSampleRate);
        }
        let secs = self.duration_secs();
        if secs <= 0.0 || secs > MAX_DURATION_SECS {
            return Err(CompositionError::BadLength);
        }
        let sr = sample_rate_hz as f32;
        let frames = ((secs + RELEASE_SECS + 0.35) * sr) as usize;
        let mut mix = vec![0.0f32; frames * 2];
        let seconds_per_beat = 60.0 / self.tempo_bpm;

        for part in &self.parts {
            let angle = (part.pan + 1.0) * std::f32::consts::FRAC_PI_4;
            let (left, right) = (angle.cos() * part.volume, angle.sin() * part.volume);
            for note in &part.notes {
                let start = (note.start_beats * seconds_per_beat * sr) as usize;
                let samples = synth_note(
                    part.instrument,
                    note.pitch,
                    note.duration_beats * seconds_per_beat,
                    note.velocity,
                    sr,
                    start as u32,
                );
                for (i, s) in samples.iter().enumerate() {
                    let frame = start + i;
                    if frame >= frames {
                        break;
                    }
                    mix[frame * 2] += s * left;
                    mix[frame * 2 + 1] += s * right;
                }
            }
        }

        let peak = mix.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        if peak > 0.95 {
            let scale = 0.95 / peak;
            mix.iter_mut().for_each(|s| *s *= scale);
        }
        Ok(mix)
    }

    /// Render to a 16-bit stereo PCM WAV file.
    pub fn to_wav_bytes(&self, sample_rate_hz: u32) -> Result<Vec<u8>, CompositionError> {
        let pcm = self.render_pcm(sample_rate_hz)?;
        let data_len = (pcm.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&2u16.to_le_bytes()); // channels
        out.extend_from_slice(&sample_rate_hz.to_le_bytes());
        out.extend_from_slice(&(sample_rate_hz * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for s in pcm {
            out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes());
        }
        Ok(out)
    }

    /// Export as a Standard MIDI File (format 1, 480 ticks per beat). Drums go to channel 10.
    pub fn to_midi_bytes(&self) -> Result<Vec<u8>, CompositionError> {
        self.validate()?;
        const TPB: f32 = 480.0;
        let mut tracks: Vec<Vec<u8>> = Vec::new();

        // Conductor track: name, tempo, 4/4.
        let mut conductor = Vec::new();
        write_meta(&mut conductor, 0, 0x03, self.title.as_bytes());
        let micros = (60_000_000.0 / self.tempo_bpm).round() as u32;
        write_meta(&mut conductor, 0, 0x51, &micros.to_be_bytes()[1..]);
        write_meta(&mut conductor, 0, 0x58, &[4, 2, 24, 8]);
        write_meta(&mut conductor, 0, 0x2F, &[]);
        tracks.push(conductor);

        let mut next_channel = 0u8;
        for part in &self.parts {
            let channel = if part.instrument.is_drum() {
                9
            } else {
                let c = next_channel;
                next_channel = (next_channel + 1) % 16;
                if next_channel == 9 {
                    next_channel = 10;
                }
                c
            };
            // (tick, order, status, data1, data2): note-offs sort before note-ons at a tick.
            let mut events: Vec<(u32, u8, u8, u8, u8)> = Vec::new();
            for note in &part.notes {
                let key = part.instrument.drum_note().unwrap_or(note.pitch);
                let on = (note.start_beats * TPB).round() as u32;
                let off = on + ((note.duration_beats * TPB).round() as u32).max(1);
                events.push((on, 1, 0x90 | channel, key, note.velocity));
                events.push((off, 0, 0x80 | channel, key, 0));
            }
            events.sort_by_key(|e| (e.0, e.1));

            let mut data = Vec::new();
            write_meta(&mut data, 0, 0x03, part.name.as_bytes());
            if !part.instrument.is_drum() {
                write_vlq(&mut data, 0);
                data.extend_from_slice(&[0xC0 | channel, part.instrument.midi_program()]);
            }
            let mut tick = 0u32;
            for (at, _, status, d1, d2) in events {
                write_vlq(&mut data, at - tick);
                tick = at;
                data.extend_from_slice(&[status, d1, d2]);
            }
            write_meta(&mut data, 0, 0x2F, &[]);
            tracks.push(data);
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"MThd");
        out.extend_from_slice(&6u32.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
        out.extend_from_slice(&480u16.to_be_bytes());
        for track in tracks {
            out.extend_from_slice(b"MTrk");
            out.extend_from_slice(&(track.len() as u32).to_be_bytes());
            out.extend_from_slice(&track);
        }
        Ok(out)
    }
}

fn write_vlq(out: &mut Vec<u8>, mut value: u32) {
    let mut bytes = vec![(value & 0x7F) as u8];
    value >>= 7;
    while value > 0 {
        bytes.push((value & 0x7F) as u8 | 0x80);
        value >>= 7;
    }
    bytes.reverse();
    out.extend(bytes);
}

fn write_meta(out: &mut Vec<u8>, delta: u32, kind: u8, data: &[u8]) {
    write_vlq(out, delta);
    out.extend_from_slice(&[0xFF, kind]);
    write_vlq(out, data.len() as u32);
    out.extend_from_slice(data);
}

fn midi_to_hz(pitch: u8) -> f32 {
    440.0 * 2f32.powf((f32::from(pitch) - 69.0) / 12.0)
}

/// Deterministic xorshift noise in -1..1.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Render one note (including its release tail) to mono samples.
fn synth_note(
    instrument: Instrument,
    pitch: u8,
    duration_secs: f32,
    velocity: u8,
    sr: f32,
    seed: u32,
) -> Vec<f32> {
    use std::f32::consts::TAU;
    let level = f32::from(velocity) / 127.0 * 0.35;
    let mut noise = Noise(seed.wrapping_mul(2_654_435_761) | 1);

    if instrument.is_drum() {
        let secs = match instrument {
            Instrument::Kick => 0.30,
            Instrument::Snare => 0.20,
            _ => 0.06,
        };
        let mut phase = 0.0f32;
        let mut previous = 0.0f32;
        return (0..(secs * sr) as usize)
            .map(|i| {
                let t = i as f32 / sr;
                let value = match instrument {
                    Instrument::Kick => {
                        phase += TAU * (45.0 + 105.0 * (-t * 30.0).exp()) / sr;
                        phase.sin() * (-t * 10.0).exp()
                    }
                    Instrument::Snare => {
                        noise.next() * (-t * 20.0).exp() * 0.8
                            + (TAU * 180.0 * t).sin() * (-t * 25.0).exp() * 0.5
                    }
                    _ => {
                        let n = noise.next();
                        let high = n - previous;
                        previous = n;
                        high * 0.5 * (-t * 70.0).exp()
                    }
                };
                value * level * 2.0
            })
            .collect();
    }

    let freq = midi_to_hz(pitch);
    let attack = if instrument == Instrument::Pad {
        0.08
    } else {
        0.005
    };
    let total = ((duration_secs + RELEASE_SECS) * sr) as usize;
    let mut phase = 0.0f32;
    let step = freq / sr;
    (0..total)
        .map(|i| {
            let t = i as f32 / sr;
            let held = |t: f32| -> f32 {
                let a = (t / attack).min(1.0);
                if instrument == Instrument::Pluck {
                    a * (-t * 5.0).exp()
                } else {
                    a * (0.7 + 0.3 * (-(t - attack).max(0.0) * 12.0).exp())
                }
            };
            let env = if t < duration_secs {
                held(t)
            } else {
                held(duration_secs) * (1.0 - (t - duration_secs) / RELEASE_SECS).max(0.0)
            };
            phase = (phase + step).fract();
            let wave = match instrument {
                Instrument::Sine => (TAU * phase).sin(),
                Instrument::Square => {
                    if phase < 0.5 {
                        0.6
                    } else {
                        -0.6
                    }
                }
                Instrument::Saw => (2.0 * phase - 1.0) * 0.6,
                Instrument::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
                Instrument::Pluck => {
                    (4.0 * (phase - 0.5).abs() - 1.0) * 0.8 + (2.0 * phase - 1.0) * 0.3
                }
                _ => {
                    // Pad: soft stack of sine partials.
                    (TAU * phase).sin() * 0.6
                        + (TAU * 2.0 * phase).sin() * 0.25
                        + (TAU * 0.5 * phase).sin() * 0.15
                }
            };
            wave * env * level
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordQuality {
    Major,
    Minor,
    Diminished,
    Sus4,
    Dominant7,
    Major7,
    Minor7,
}

/// MIDI pitches of a chord built on `root` (notes above 127 are dropped).
pub fn chord_pitches(root: u8, quality: ChordQuality) -> Vec<u8> {
    let intervals: &[u8] = match quality {
        ChordQuality::Major => &[0, 4, 7],
        ChordQuality::Minor => &[0, 3, 7],
        ChordQuality::Diminished => &[0, 3, 6],
        ChordQuality::Sus4 => &[0, 5, 7],
        ChordQuality::Dominant7 => &[0, 4, 7, 10],
        ChordQuality::Major7 => &[0, 4, 7, 11],
        ChordQuality::Minor7 => &[0, 3, 7, 10],
    };
    intervals
        .iter()
        .filter_map(|i| root.checked_add(*i))
        .filter(|p| *p <= 127)
        .collect()
}

/// A drum step sequencer: one row per drum, `x` = hit, `X` = accent, `.` = rest.
#[derive(Debug, Clone, PartialEq)]
pub struct DrumPattern {
    steps: usize,
    lanes: Vec<(Instrument, Vec<u8>)>,
}

impl DrumPattern {
    /// Rows must all have the same length (1..=64 steps) and use only kick, snare or hi-hat.
    pub fn from_rows(rows: &[(Instrument, &str)]) -> Result<Self, CompositionError> {
        let steps = rows.first().map(|(_, r)| r.chars().count()).unwrap_or(0);
        if steps == 0 || steps > 64 {
            return Err(CompositionError::InvalidPattern);
        }
        let mut lanes = Vec::new();
        for (instrument, row) in rows {
            if !instrument.is_drum() || row.chars().count() != steps {
                return Err(CompositionError::InvalidPattern);
            }
            let velocities = row
                .chars()
                .map(|c| match c {
                    'x' => Ok(100),
                    'X' => Ok(127),
                    '.' | '-' => Ok(0),
                    _ => Err(CompositionError::InvalidPattern),
                })
                .collect::<Result<Vec<u8>, _>>()?;
            lanes.push((*instrument, velocities));
        }
        Ok(Self { steps, lanes })
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    /// Repeat the pattern `repeats` times (each step is `beats_per_step` beats long) and add
    /// one part per drum to the composition, starting at `start_beats`.
    pub fn append_to(
        &self,
        composition: &mut Composition,
        start_beats: f32,
        repeats: u32,
        beats_per_step: f32,
    ) {
        for (instrument, velocities) in &self.lanes {
            let name = format!("{instrument:?}");
            let index = composition
                .parts
                .iter()
                .position(|p| p.instrument == *instrument && p.name == name);
            let part = match index {
                Some(i) => &mut composition.parts[i],
                None => {
                    composition.parts.push(Part::new(&name, *instrument));
                    composition.parts.last_mut().expect("just pushed")
                }
            };
            for repeat in 0..repeats {
                for (step, velocity) in velocities.iter().enumerate() {
                    if *velocity > 0 {
                        let at = start_beats
                            + (repeat as usize * self.steps + step) as f32 * beats_per_step;
                        part.add_note(at, beats_per_step.max(0.05), 60, *velocity);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple() -> Composition {
        let mut c = Composition::new("Demo", 120.0);
        let mut lead = Part::new("Lead", Instrument::Sine);
        lead.add_note(0.0, 1.0, 69, 100); // A4
        lead.add_note(1.0, 1.0, 72, 100);
        c.parts.push(lead);
        c
    }

    #[test]
    fn duration_follows_tempo() {
        let c = simple();
        assert_eq!(c.duration_beats(), 2.0);
        assert!((c.duration_secs() - 1.0).abs() < 1e-6);
        assert_eq!(c.duration_ms(), 1_000);
    }

    #[test]
    fn validation_catches_bad_input() {
        let mut c = simple();
        c.tempo_bpm = 10.0;
        assert_eq!(c.validate(), Err(CompositionError::InvalidTempo));
        let mut c = simple();
        c.parts[0].notes[1].velocity = 0;
        assert!(matches!(
            c.validate(),
            Err(CompositionError::InvalidNote { index: 1, .. })
        ));
        let mut c = simple();
        c.parts[0].pan = 2.0;
        assert!(matches!(
            c.validate(),
            Err(CompositionError::InvalidPart(_))
        ));
        let mut c = simple();
        c.parts[0].notes[0].duration_beats = f32::NAN;
        assert!(c.validate().is_err());
        assert_eq!(
            Composition::new("x", 120.0).render_pcm(44_100),
            Err(CompositionError::BadLength)
        );
    }

    #[test]
    fn rendered_audio_has_the_right_length_pitch_and_headroom() {
        let c = simple();
        let sr = 44_100u32;
        let pcm = c.render_pcm(sr).unwrap();
        assert_eq!(pcm.len() % 2, 0);
        let peak = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05 && peak <= 0.95, "{peak}");

        // Count rising zero crossings in the first note (A4 = 440 Hz) -> about 440 in 1 s.
        let first_second: Vec<f32> = pcm
            .iter()
            .step_by(2)
            .take(sr as usize / 2)
            .cloned()
            .collect();
        let crossings = first_second
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        assert!((crossings as i32 - 220).abs() <= 3, "{crossings}");
    }

    #[test]
    fn rendering_is_deterministic_and_mix_is_normalised() {
        let mut c = Composition::new("Loud", 120.0);
        for i in 0..8 {
            let mut p = Part::new(&format!("p{i}"), Instrument::Saw);
            p.volume = 1.0;
            p.add_chord(0.0, 2.0, &[60, 64, 67], 127);
            c.parts.push(p);
        }
        let a = c.render_pcm(22_050).unwrap();
        assert_eq!(a, c.render_pcm(22_050).unwrap());
        assert!(a.iter().all(|s| s.abs() <= 0.951));
    }

    #[test]
    fn panning_moves_sound_between_channels() {
        let mut c = simple();
        c.parts[0].pan = -1.0;
        let pcm = c.render_pcm(22_050).unwrap();
        let left: f32 = pcm.iter().step_by(2).map(|s| s.abs()).sum();
        let right: f32 = pcm.iter().skip(1).step_by(2).map(|s| s.abs()).sum();
        assert!(left > 100.0 * right.max(1e-6));
    }

    #[test]
    fn wav_header_is_valid() {
        let wav = simple().to_wav_bytes(22_050).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 2);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            22_050
        );
        let data_len = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
        assert_eq!(wav.len(), 44 + data_len);
        assert_eq!(
            u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]) as usize,
            36 + data_len
        );
        assert_eq!(
            simple().to_wav_bytes(100),
            Err(CompositionError::InvalidSampleRate)
        );
    }

    fn read_vlq(data: &[u8], pos: &mut usize) -> u32 {
        let mut value = 0u32;
        loop {
            let b = data[*pos];
            *pos += 1;
            value = (value << 7) | u32::from(b & 0x7F);
            if b & 0x80 == 0 {
                return value;
            }
        }
    }

    /// Returns (tick, channel, key) of every note-on in a Standard MIDI File.
    fn note_ons(midi: &[u8]) -> Vec<(u32, u8, u8)> {
        assert_eq!(&midi[0..4], b"MThd");
        let tracks = u16::from_be_bytes([midi[10], midi[11]]) as usize;
        let mut pos = 14;
        let mut out = Vec::new();
        for _ in 0..tracks {
            assert_eq!(&midi[pos..pos + 4], b"MTrk");
            let len =
                u32::from_be_bytes([midi[pos + 4], midi[pos + 5], midi[pos + 6], midi[pos + 7]])
                    as usize;
            pos += 8;
            let end = pos + len;
            let mut tick = 0;
            while pos < end {
                tick += read_vlq(midi, &mut pos);
                let status = midi[pos];
                pos += 1;
                match status {
                    0xFF => {
                        pos += 1;
                        let l = read_vlq(midi, &mut pos) as usize;
                        pos += l;
                    }
                    s if s & 0xF0 == 0x90 => {
                        if midi[pos + 1] > 0 {
                            out.push((tick, s & 0x0F, midi[pos]));
                        }
                        pos += 2;
                    }
                    s if s & 0xF0 == 0x80 => pos += 2,
                    s if s & 0xF0 == 0xC0 => pos += 1,
                    other => panic!("unexpected status {other:#x}"),
                }
            }
        }
        out
    }

    #[test]
    fn midi_export_round_trips_notes_tempo_and_drum_channel() {
        let mut c = simple();
        c.tempo_bpm = 100.0;
        let pattern = DrumPattern::from_rows(&[(Instrument::Kick, "x...x...")]).unwrap();
        pattern.append_to(&mut c, 0.0, 1, 0.5);
        let midi = c.to_midi_bytes().unwrap();
        // 3 tracks: conductor, lead, kick.
        assert_eq!(u16::from_be_bytes([midi[10], midi[11]]), 3);
        // Tempo meta: 60_000_000 / 100 = 600_000 us per beat.
        let tempo = midi
            .windows(6)
            .position(|w| w == [0xFF, 0x51, 0x03, 0x09, 0x27, 0xC0]);
        assert!(tempo.is_some());

        let ons = note_ons(&midi);
        let lead: Vec<_> = ons.iter().filter(|n| n.1 == 0).collect();
        assert_eq!(lead.len(), 2);
        assert_eq!((lead[0].0, lead[0].2), (0, 69));
        assert_eq!((lead[1].0, lead[1].2), (480, 72));
        let drums: Vec<_> = ons.iter().filter(|n| n.1 == 9).collect();
        assert_eq!(drums.len(), 2);
        assert!(drums.iter().all(|n| n.2 == 36));
        assert_eq!(drums[1].0, 960);
    }

    #[test]
    fn drum_pattern_parsing_and_repeats() {
        assert!(DrumPattern::from_rows(&[]).is_err());
        assert!(
            DrumPattern::from_rows(&[(Instrument::Kick, "x..."), (Instrument::Snare, "x.")])
                .is_err()
        );
        assert!(DrumPattern::from_rows(&[(Instrument::Sine, "x...")]).is_err());
        assert!(DrumPattern::from_rows(&[(Instrument::Kick, "x?..")]).is_err());

        let p = DrumPattern::from_rows(&[(Instrument::Kick, "x..."), (Instrument::Snare, "..X.")])
            .unwrap();
        assert_eq!(p.steps(), 4);
        let mut c = Composition::new("Beat", 120.0);
        p.append_to(&mut c, 0.0, 2, 1.0);
        assert_eq!(c.parts.len(), 2);
        assert_eq!(c.parts[0].notes.len(), 2);
        assert_eq!(c.parts[1].notes[0].velocity, 127);
        assert_eq!(c.parts[1].notes[1].start_beats, 6.0);
        // Appending again reuses the same parts.
        p.append_to(&mut c, 8.0, 1, 1.0);
        assert_eq!(c.parts.len(), 2);
        assert_eq!(c.parts[0].notes.len(), 3);
        assert!(c.render_pcm(22_050).is_ok());
    }

    #[test]
    fn transpose_skips_drums_and_clamps() {
        let mut c = simple();
        let mut d = Part::new("Kick", Instrument::Kick);
        d.add_note(0.0, 0.5, 36, 100);
        c.parts.push(d);
        c.transpose(12);
        assert_eq!(c.parts[0].notes[0].pitch, 81);
        assert_eq!(c.parts[1].notes[0].pitch, 36);
        c.transpose(100);
        assert_eq!(c.parts[0].notes[0].pitch, 127);
        c.transpose(-300);
        assert_eq!(c.parts[0].notes[0].pitch, 0);
    }

    #[test]
    fn chord_pitches_follow_the_quality() {
        assert_eq!(chord_pitches(60, ChordQuality::Major), [60, 64, 67]);
        assert_eq!(chord_pitches(57, ChordQuality::Minor7), [57, 60, 64, 67]);
        assert_eq!(chord_pitches(126, ChordQuality::Major), [126]);
    }

    #[test]
    fn midi_vlq_encoding_matches_the_spec_examples() {
        let enc = |v| {
            let mut o = Vec::new();
            write_vlq(&mut o, v);
            o
        };
        assert_eq!(enc(0), [0x00]);
        assert_eq!(enc(0x7F), [0x7F]);
        assert_eq!(enc(0x80), [0x81, 0x00]);
        assert_eq!(enc(0x3FFF), [0xFF, 0x7F]);
        assert_eq!(enc(0x4000), [0x81, 0x80, 0x00]);
    }
}
