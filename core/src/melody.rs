//! Offline lyric-to-melody generator: from lyrics and a style it composes a melody, chords,
//! bass and drums as a `Composition` that can be rendered to WAV, exported as MIDI, or
//! edited by hand. Output is fully deterministic for a given lyric, options and seed.
//!
//! This is a rule-based generator (scales, progressions, syllable-aware rhythm, phrase
//! endings), so it works without a network or model. A host may still send the same
//! `LyricToMelodyRequest` to an AI service and play its result instead.

use crate::composition::{Composition, CompositionError, DrumPattern, Instrument, Part};
use crate::creative::LyricToMelodyRequest;
use crate::text::normalize_for_search;
use thiserror::Error;

pub const MAX_LINES: usize = 32;
pub const MAX_LYRIC_CHARS: usize = 8_000;
const BEATS_PER_LINE: f32 = 8.0; // two 4/4 bars per lyric line

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MelodyStyle {
    Pop,
    Sad,
    Rock,
    Lofi,
    Jazz,
    Persian,
    Cinematic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Major,
    Minor,
    Dorian,
    /// Hijaz-like colour (1 b2 3 4 5 b6 b7), used for the Persian style.
    PhrygianDominant,
}

impl Mode {
    fn scale(self) -> [i32; 7] {
        match self {
            Mode::Major => [0, 2, 4, 5, 7, 9, 11],
            Mode::Minor => [0, 2, 3, 5, 7, 8, 10],
            Mode::Dorian => [0, 2, 3, 5, 7, 9, 10],
            Mode::PhrygianDominant => [0, 1, 4, 5, 7, 8, 10],
        }
    }
}

impl MelodyStyle {
    /// Parse a style name (case-insensitive; "fa"/"persian"/"iranian" and common genres).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_lowercase().as_str() {
            "pop" => Some(Self::Pop),
            "sad" | "ballad" | "emotional" => Some(Self::Sad),
            "rock" => Some(Self::Rock),
            "lofi" | "lo-fi" | "chill" => Some(Self::Lofi),
            "jazz" => Some(Self::Jazz),
            "persian" | "iranian" | "fa" | "farsi" => Some(Self::Persian),
            "cinematic" | "film" | "epic" => Some(Self::Cinematic),
            _ => None,
        }
    }

    fn default_tempo(self) -> f32 {
        match self {
            Self::Pop => 110.0,
            Self::Sad => 72.0,
            Self::Rock => 128.0,
            Self::Lofi => 80.0,
            Self::Jazz => 120.0,
            Self::Persian => 96.0,
            Self::Cinematic => 70.0,
        }
    }

    fn default_mode(self) -> Mode {
        match self {
            Self::Pop | Self::Jazz => Mode::Major,
            Self::Sad | Self::Rock | Self::Cinematic => Mode::Minor,
            Self::Lofi => Mode::Dorian,
            Self::Persian => Mode::PhrygianDominant,
        }
    }

    /// Chord roots as scale degrees, one chord per bar, looping.
    fn progression(self) -> [i32; 4] {
        match self {
            Self::Pop => [0, 4, 5, 3],
            Self::Sad => [0, 5, 2, 6],
            Self::Rock => [0, 6, 5, 6],
            Self::Lofi | Self::Jazz => [1, 4, 0, 5],
            Self::Persian => [0, 1, 0, 3],
            Self::Cinematic => [0, 5, 3, 4],
        }
    }

    fn sevenths(self) -> bool {
        matches!(self, Self::Jazz | Self::Lofi)
    }

    fn melody_instrument(self) -> Instrument {
        match self {
            Self::Pop | Self::Jazz => Instrument::Triangle,
            Self::Sad => Instrument::Sine,
            Self::Rock => Instrument::Saw,
            Self::Lofi | Self::Persian => Instrument::Pluck,
            Self::Cinematic => Instrument::Pad,
        }
    }

    fn drum_rows(self) -> [&'static str; 3] {
        match self {
            Self::Rock | Self::Pop => ["x...x...", "..x...x.", "xxxxxxxx"],
            Self::Jazz => ["x.......", "..x...x.", "x.x.x.x."],
            _ => ["x.....x.", "....x...", "x.x.x.x."],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MelodyOptions {
    pub style: MelodyStyle,
    pub tempo_bpm: Option<f32>,
    /// Tonic pitch class 0..=11 (C = 0). `None` = C.
    pub key_root: Option<u8>,
    /// Overrides the style's default mode.
    pub mode: Option<Mode>,
    pub seed: u64,
    pub include_bass: bool,
    pub include_drums: bool,
}

impl Default for MelodyOptions {
    fn default() -> Self {
        Self {
            style: MelodyStyle::Pop,
            tempo_bpm: None,
            key_root: None,
            mode: None,
            seed: 1,
            include_bass: true,
            include_drums: true,
        }
    }
}

impl MelodyOptions {
    /// Options from a creative request. Unknown styles fall back to Pop; the seed is derived
    /// from the lyrics so the same request always yields the same tune.
    pub fn from_request(request: &LyricToMelodyRequest) -> Self {
        let key = request.key.as_deref().and_then(parse_key);
        Self {
            style: request
                .style
                .as_deref()
                .and_then(MelodyStyle::parse)
                .unwrap_or(MelodyStyle::Pop),
            tempo_bpm: request.tempo_bpm,
            key_root: key.map(|(root, _)| root),
            mode: key.and_then(|(_, mode)| mode),
            seed: fnv1a(request.lyrics.as_bytes()),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MelodyError {
    #[error("the lyrics are empty")]
    EmptyLyrics,
    #[error("too many lines or characters (limit 32 lines, 8000 characters)")]
    TooLong,
    #[error(transparent)]
    Composition(#[from] CompositionError),
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Parse a key such as "A minor", "F# minor", "Bb major", "Am" or "D".
/// Returns the tonic pitch class and the mode when it is stated.
pub fn parse_key(text: &str) -> Option<(u8, Option<Mode>)> {
    let text = text.trim();
    let mut chars = text.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let mut root: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let mut rest = rest.trim_start();
    if let Some(r) = rest.strip_prefix(['#', '♯']) {
        root += 1;
        rest = r;
    } else if let Some(r) = rest.strip_prefix(['b', '♭']) {
        // "Bb" is B-flat, but a bare "b" after a letter is only a flat sign here.
        root -= 1;
        rest = r;
    }
    let mode = match rest.trim().to_lowercase().as_str() {
        "" => None,
        "m" | "min" | "minor" => Some(Mode::Minor),
        "maj" | "major" => Some(Mode::Major),
        "dorian" => Some(Mode::Dorian),
        _ => return None,
    };
    Some((root.rem_euclid(12) as u8, mode))
}

/// Estimate the syllables of a word. Latin words use vowel groups (with a silent final "e");
/// Arabic-script words (Persian) use about one syllable per 2.5 letters. A heuristic, but
/// good enough to give each sung word a sensible number of notes.
pub fn count_syllables(word: &str) -> usize {
    let normalized = normalize_for_search(word);
    let letters: Vec<char> = normalized.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return usize::from(normalized.chars().any(|c| c.is_numeric()));
    }
    if letters
        .iter()
        .any(|c| ('\u{0600}'..='\u{06FF}').contains(c))
    {
        return ((letters.len() as f32 / 2.5).round() as usize).max(1);
    }
    let is_vowel = |c: &char| "aeiouy".contains(*c);
    let mut groups = 0;
    let mut previous = false;
    for c in &letters {
        let v = is_vowel(c);
        if v && !previous {
            groups += 1;
        }
        previous = v;
    }
    let text: String = letters.iter().collect();
    if groups > 1 && text.ends_with('e') && !text.ends_with("le") {
        groups -= 1;
    }
    groups.max(1)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Weighted choice over `(value, weight)` pairs.
    fn pick(&mut self, options: &[(i32, u64)]) -> i32 {
        let total: u64 = options.iter().map(|(_, w)| w).sum();
        let mut roll = self.next() % total;
        for (value, weight) in options {
            if roll < *weight {
                return *value;
            }
            roll -= weight;
        }
        options[0].0
    }
}

/// Compose a melody with chords, bass and drums for the lyrics.
pub fn generate(lyrics: &str, options: &MelodyOptions) -> Result<Composition, MelodyError> {
    let lines: Vec<&str> = lyrics
        .lines()
        .map(str::trim)
        .filter(|l| l.chars().any(|c| c.is_alphanumeric()))
        .collect();
    if lines.is_empty() {
        return Err(MelodyError::EmptyLyrics);
    }
    if lines.len() > MAX_LINES || lyrics.chars().count() > MAX_LYRIC_CHARS {
        return Err(MelodyError::TooLong);
    }

    let style = options.style;
    let scale = options.mode.unwrap_or_else(|| style.default_mode()).scale();
    let root = i32::from(options.key_root.unwrap_or(0) % 12);
    // Melody register: tonic between F#3 and F4 so the pitch class equals the key.
    let base = if root > 5 { 48 + root } else { 60 + root };
    let progression = style.progression();
    let pitch = |degree: i32| -> i32 {
        base + 12 * degree.div_euclid(7) + scale[degree.rem_euclid(7) as usize]
    };
    let clamp_pitch = |p: i32| p.clamp(0, 127) as u8;
    let chord_degrees = |bar: usize| -> Vec<i32> {
        let root_degree = progression[bar % 4];
        let mut degrees = vec![root_degree, root_degree + 2, root_degree + 4];
        if style.sevenths() {
            degrees.push(root_degree + 6);
        }
        degrees
    };

    let tempo = options.tempo_bpm.unwrap_or_else(|| style.default_tempo());
    let title: String = lines[0].chars().take(40).collect();
    let mut composition = Composition::new(&title, tempo);
    let mut melody = Part::new("Melody", style.melody_instrument());
    melody.volume = 0.9;
    melody.pan = 0.1;
    let mut chords = Part::new("Chords", Instrument::Pad);
    chords.volume = 0.5;
    chords.pan = -0.2;
    let mut bass = Part::new("Bass", Instrument::Triangle);
    bass.volume = 0.8;

    let mut rng = Rng(options.seed ^ 0x9E37_79B9_7F4A_7C15 | 1);
    let mut degree: i32 = 0;
    let line_count = lines.len();

    for (line_index, line) in lines.iter().enumerate() {
        let syllables: usize = line
            .split_whitespace()
            .map(count_syllables)
            .sum::<usize>()
            .clamp(1, 32);
        let unit = if syllables <= 8 {
            ((BEATS_PER_LINE / syllables as f32) * 2.0).floor() / 2.0
        } else if syllables <= 16 {
            0.5
        } else {
            0.25
        }
        .max(0.25);
        let line_start = line_index as f32 * BEATS_PER_LINE;
        let first_bar = line_index * 2;

        for k in 0..syllables {
            let offset = k as f32 * unit;
            let is_last = k + 1 == syllables;
            let bar = first_bar + (offset / 4.0) as usize;
            let tones = chord_degrees(bar);

            let step = rng.pick(&[(-3, 1), (-2, 2), (-1, 4), (0, 1), (1, 4), (2, 2), (3, 1)]);
            degree = (degree + step).clamp(-2, 9);

            let strong = offset % 2.0 == 0.0;
            if is_last {
                // Phrase endings: question (dominant) then answer (tonic); the song ends on tonic.
                let targets: &[i32] = if line_index + 1 == line_count || line_index % 2 == 1 {
                    &[0, 7]
                } else {
                    &[4]
                };
                degree = *targets
                    .iter()
                    .min_by_key(|t| ((**t - degree).abs(), **t))
                    .expect("targets is never empty");
            } else if strong {
                // Land on a chord tone on strong beats; prefer the nearest one.
                let snapped = (0..=3).find_map(|distance| {
                    [0, 1, -1]
                        .iter()
                        .take(if distance == 0 { 1 } else { 3 })
                        .map(|sign| degree + sign * distance)
                        .find(|d| {
                            (-2..=9).contains(d)
                                && tones.iter().any(|t| (*t - *d).rem_euclid(7) == 0)
                        })
                });
                if let Some(d) = snapped {
                    degree = d;
                }
            }

            let duration = if is_last {
                (BEATS_PER_LINE - offset).min(2.0_f32.max(unit))
            } else {
                unit
            };
            let velocity = if offset % 4.0 == 0.0 { 100 } else { 85 };
            melody.add_note(
                line_start + offset,
                duration * 0.95,
                clamp_pitch(pitch(degree)),
                velocity,
            );
        }
    }

    // One chord per bar, plus a final bar that resolves to the tonic.
    let bars = line_count * 2 + 1;
    for bar in 0..bars {
        let start = bar as f32 * 4.0;
        let final_bar = bar + 1 == bars;
        let degrees = if final_bar {
            vec![0, 2, 4]
        } else {
            chord_degrees(bar)
        };
        let pitches: Vec<u8> = degrees
            .iter()
            .map(|d| clamp_pitch(pitch(*d) - 12))
            .collect();
        chords.add_chord(start, 3.95, &pitches, 70);
        if options.include_bass {
            let root_pitch = clamp_pitch(pitch(degrees[0]) - 24);
            if final_bar {
                bass.add_note(start, 3.5, root_pitch, 95);
            } else {
                bass.add_note(start, 1.9, root_pitch, 95);
                bass.add_note(start + 2.0, 1.9, root_pitch, 90);
            }
        }
    }
    let final_start = (bars - 1) as f32 * 4.0;
    melody.add_note(final_start, 3.5, clamp_pitch(pitch(0)), 100);

    composition.parts.push(melody);
    composition.parts.push(chords);
    if options.include_bass {
        composition.parts.push(bass);
    }
    if options.include_drums {
        let [kick, snare, hat] = style.drum_rows();
        let pattern = DrumPattern::from_rows(&[
            (Instrument::Kick, kick),
            (Instrument::Snare, snare),
            (Instrument::HiHat, hat),
        ])?;
        pattern.append_to(&mut composition, 0.0, bars as u32, 0.5);
    }
    composition.validate()?;
    Ok(composition)
}

/// Compose from a creative request (style, tempo and key hints are honoured).
pub fn generate_from_request(request: &LyricToMelodyRequest) -> Result<Composition, MelodyError> {
    generate(&request.lyrics, &MelodyOptions::from_request(request))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LYRICS: &str = "Hello my dear friend\nThe night is calm and bright\nWalking home together\nUnder silver light";

    fn melody_pitches(c: &Composition) -> Vec<u8> {
        c.parts[0].notes.iter().map(|n| n.pitch).collect()
    }

    #[test]
    fn counts_syllables_for_latin_and_persian_words() {
        assert_eq!(count_syllables("hello"), 2);
        assert_eq!(count_syllables("beautiful"), 3);
        assert_eq!(count_syllables("time"), 1);
        assert_eq!(count_syllables("table"), 2);
        assert_eq!(count_syllables("the"), 1);
        assert_eq!(count_syllables("rhythm"), 1);
        assert_eq!(count_syllables("سلام"), 2);
        assert_eq!(count_syllables("من"), 1);
        assert_eq!(count_syllables("123"), 1);
        assert_eq!(count_syllables("..."), 0);
    }

    #[test]
    fn parses_keys_and_styles() {
        assert_eq!(parse_key("A minor"), Some((9, Some(Mode::Minor))));
        assert_eq!(parse_key("F# minor"), Some((6, Some(Mode::Minor))));
        assert_eq!(parse_key("Bb major"), Some((10, Some(Mode::Major))));
        assert_eq!(parse_key("Am"), Some((9, Some(Mode::Minor))));
        assert_eq!(parse_key("D"), Some((2, None)));
        assert_eq!(parse_key("H major"), None);
        assert_eq!(parse_key("C lydian"), None);
        assert_eq!(MelodyStyle::parse(" LoFi "), Some(MelodyStyle::Lofi));
        assert_eq!(MelodyStyle::parse("farsi"), Some(MelodyStyle::Persian));
        assert_eq!(MelodyStyle::parse("polka"), None);
    }

    #[test]
    fn generates_one_note_per_syllable_plus_an_ending() {
        let c = generate(LYRICS, &MelodyOptions::default()).unwrap();
        let syllables: usize = LYRICS
            .lines()
            .map(|l| l.split_whitespace().map(count_syllables).sum::<usize>())
            .sum();
        assert_eq!(c.parts[0].notes.len(), syllables + 1);
        assert_eq!(c.parts[0].name, "Melody");
        assert_eq!(c.tempo_bpm, 110.0);
        // 4 lines -> 8 bars + 1 resolving bar of chords.
        assert_eq!(c.parts[1].notes.len() / 3, 9);
        assert!(c.validate().is_ok());
        assert_eq!(c.title, "Hello my dear friend");
    }

    #[test]
    fn same_seed_same_tune_different_seed_different_tune() {
        let a = generate(LYRICS, &MelodyOptions::default()).unwrap();
        let b = generate(LYRICS, &MelodyOptions::default()).unwrap();
        assert_eq!(a, b);
        let c = generate(
            LYRICS,
            &MelodyOptions {
                seed: 99,
                ..Default::default()
            },
        )
        .unwrap();
        assert_ne!(melody_pitches(&a), melody_pitches(&c));
    }

    #[test]
    fn melody_stays_in_the_requested_key_and_scale() {
        // F# minor: pitch classes F# G# A B C# D E.
        let key = [6, 8, 9, 11, 1, 2, 4];
        let opts = MelodyOptions {
            key_root: Some(6),
            mode: Some(Mode::Minor),
            ..Default::default()
        };
        let c = generate(LYRICS, &opts).unwrap();
        for part in &c.parts[..3] {
            for n in &part.notes {
                assert!(key.contains(&(n.pitch % 12)), "{} not in key", n.pitch);
            }
        }
        // The song ends on the tonic.
        assert_eq!(c.parts[0].notes.last().unwrap().pitch % 12, 6);
    }

    #[test]
    fn persian_style_uses_the_phrygian_dominant_colour() {
        let opts = MelodyOptions {
            style: MelodyStyle::Persian,
            key_root: Some(9),
            ..Default::default()
        };
        let c = generate("دلم تنگ است برای تو\nشب بی ستاره و تنها", &opts).unwrap();
        // A hijaz-like scale: A Bb C# D E F G
        let scale = [9, 10, 1, 2, 4, 5, 7];
        assert!(c.parts[0]
            .notes
            .iter()
            .all(|n| scale.contains(&(n.pitch % 12))));
        assert_eq!(c.tempo_bpm, 96.0);
    }

    #[test]
    fn first_strong_note_of_each_phrase_is_a_chord_tone_of_pop() {
        let c = generate(LYRICS, &MelodyOptions::default()).unwrap();
        // Pop in C major, first bar chord is I = C E G -> pitch classes 0, 4, 7.
        let first = c.parts[0].notes[0].pitch % 12;
        assert!([0, 4, 7].contains(&first), "{first}");
    }

    #[test]
    fn options_can_disable_bass_and_drums() {
        let opts = MelodyOptions {
            include_bass: false,
            include_drums: false,
            ..Default::default()
        };
        let c = generate(LYRICS, &opts).unwrap();
        assert_eq!(c.parts.len(), 2);
        let full = generate(LYRICS, &MelodyOptions::default()).unwrap();
        assert_eq!(full.parts.len(), 2 + 1 + 3);
    }

    #[test]
    fn rejects_empty_and_oversized_lyrics() {
        assert_eq!(
            generate("  \n ...\n", &MelodyOptions::default()),
            Err(MelodyError::EmptyLyrics)
        );
        let long = "la la la\n".repeat(40);
        assert_eq!(
            generate(&long, &MelodyOptions::default()),
            Err(MelodyError::TooLong)
        );
        let bad_tempo = MelodyOptions {
            tempo_bpm: Some(5.0),
            ..Default::default()
        };
        assert!(matches!(
            generate(LYRICS, &bad_tempo),
            Err(MelodyError::Composition(CompositionError::InvalidTempo))
        ));
    }

    #[test]
    fn request_hints_are_honoured_and_the_result_renders_to_audio() {
        let request = LyricToMelodyRequest {
            lyrics: LYRICS.into(),
            style: Some("Lofi".into()),
            tempo_bpm: Some(90.0),
            key: Some("F# minor".into()),
            duration_secs: None,
            also_instrumental: false,
        };
        let c = generate_from_request(&request).unwrap();
        assert_eq!(c.tempo_bpm, 90.0);
        assert_eq!(c.parts[0].instrument, Instrument::Pluck);
        assert_eq!(c.parts[0].notes.last().unwrap().pitch % 12, 6);
        // Deterministic per request.
        assert_eq!(c, generate_from_request(&request).unwrap());

        let wav = c.to_wav_bytes(22_050).unwrap();
        assert!(wav.len() > 44 + 22_050 * 4 * 10);
        assert!(c.to_midi_bytes().is_ok());
    }

    #[test]
    fn every_style_produces_a_renderable_song() {
        for style in [
            MelodyStyle::Pop,
            MelodyStyle::Sad,
            MelodyStyle::Rock,
            MelodyStyle::Lofi,
            MelodyStyle::Jazz,
            MelodyStyle::Persian,
            MelodyStyle::Cinematic,
        ] {
            let c = generate(
                LYRICS,
                &MelodyOptions {
                    style,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(c.render_pcm(16_000).is_ok(), "{style:?}");
        }
    }
}
