//! Turns audio into haptic events: detects bass/kick, snare-like and hi-hat transients in
//! PCM and maps them to the user's `HapticSettings`.
//!
//! The analyzer is streaming and allocation-light: feed it interleaved f32 blocks of any size
//! and collect events with millisecond timestamps. The host plays the resulting pulses on the
//! vibrator, controller or wearable (ideally a little ahead of the audio to hide latency).

use crate::haptic::{HapticPulse, HapticSettings, HapticTrigger};

/// Analysis hop (frame) length in milliseconds.
const HOP_MS: f32 = 5.0;
/// Frames of energy history used for the adaptive threshold (about one second).
const HISTORY_FRAMES: usize = 200;
const LOW_CUTOFF_HZ: f32 = 150.0;
const HIGH_CUTOFF_HZ: f32 = 3_500.0;
/// An onset must be this many times louder than the recent average.
const ONSET_RATIO: f32 = 2.2;
/// A band only fires when it holds at least this share of the loudest band's energy; this
/// stops filter leakage (a kick's click, noise reaching the bass band) from false triggers.
const DOMINANCE: f32 = 0.1;
/// Band energies (mean square) below this are silence.
const ENERGY_FLOOR: f32 = 1.0e-4;

/// A detected musical event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HapticEvent {
    pub time_ms: u64,
    pub trigger: HapticTrigger,
    /// 0.0..=1.0 relative strength of the hit.
    pub intensity: f32,
}

#[derive(Debug, Clone)]
struct Band {
    trigger: HapticTrigger,
    history: Vec<f32>,
    next: usize,
    filled: usize,
    refractory_frames: u32,
    cooldown: u32,
}

impl Band {
    fn new(trigger: HapticTrigger, refractory_ms: f32) -> Self {
        Self {
            trigger,
            history: vec![0.0; HISTORY_FRAMES],
            next: 0,
            filled: 0,
            refractory_frames: (refractory_ms / HOP_MS).ceil() as u32,
            cooldown: 0,
        }
    }

    fn average(&self) -> f32 {
        if self.filled == 0 {
            0.0
        } else {
            self.history[..self.filled.min(HISTORY_FRAMES)]
                .iter()
                .sum::<f32>()
                / self.filled.min(HISTORY_FRAMES) as f32
        }
    }

    /// Returns the intensity of a detected onset for this frame's energy.
    fn step(&mut self, energy: f32, loudest: f32) -> Option<f32> {
        let average = self.average();
        let mut hit = None;
        if self.cooldown > 0 {
            self.cooldown -= 1;
        } else if energy > ENERGY_FLOOR
            && energy >= loudest * DOMINANCE
            && energy > average * ONSET_RATIO
        {
            let ratio = energy / average.max(1.0e-6);
            let loudness = (energy.sqrt() * 2.5).min(1.0);
            let sharpness = (0.35 + 0.15 * ratio.ln().max(0.0)).min(1.0);
            hit = Some((loudness * 0.6 + sharpness * 0.4).clamp(0.2, 1.0));
            self.cooldown = self.refractory_frames;
        }
        self.history[self.next] = energy;
        self.next = (self.next + 1) % HISTORY_FRAMES;
        self.filled = (self.filled + 1).min(HISTORY_FRAMES);
        hit
    }
}

/// Streaming transient detector for three frequency bands.
#[derive(Debug, Clone)]
pub struct HapticAnalyzer {
    channels: usize,
    hop_samples: usize,
    low_coeff: f32,
    high_coeff: f32,
    low_state: f32,
    high_state: f32,
    band_low: f32,
    band_mid: f32,
    band_high: f32,
    sums: [f32; 3],
    samples_in_hop: usize,
    frame_index: u64,
    bass: Band,
    snare: Band,
    hihat: Band,
    sample_rate: f32,
}

impl HapticAnalyzer {
    /// `None` for a zero channel count or a sample rate below 8 kHz.
    pub fn new(sample_rate_hz: u32, channels: usize) -> Option<Self> {
        if channels == 0 || sample_rate_hz < 8_000 {
            return None;
        }
        let sr = sample_rate_hz as f32;
        let coeff = |fc: f32| 1.0 - (-2.0 * std::f32::consts::PI * fc / sr).exp();
        Some(Self {
            channels,
            hop_samples: ((sr * HOP_MS / 1000.0) as usize).max(1),
            low_coeff: coeff(LOW_CUTOFF_HZ),
            high_coeff: coeff(HIGH_CUTOFF_HZ),
            low_state: 0.0,
            high_state: 0.0,
            band_low: 0.0,
            band_mid: 0.0,
            band_high: 0.0,
            sums: [0.0; 3],
            samples_in_hop: 0,
            frame_index: 0,
            bass: Band::new(HapticTrigger::BassHit, 120.0),
            snare: Band::new(HapticTrigger::Snare, 100.0),
            hihat: Band::new(HapticTrigger::HiHat, 50.0),
            sample_rate: sr,
        })
    }

    /// Analyze a block of interleaved samples; returns the events found in it.
    pub fn process(&mut self, interleaved: &[f32]) -> Vec<HapticEvent> {
        let mut events = Vec::new();
        for frame in interleaved.chunks_exact(self.channels) {
            let mono = frame.iter().sum::<f32>() / self.channels as f32;
            // Split into low (<150 Hz), mid and high (>3.5 kHz) with one-pole filters.
            self.low_state += self.low_coeff * (mono - self.low_state);
            self.high_state += self.high_coeff * (mono - self.high_state);
            self.band_low = self.low_state;
            self.band_high = mono - self.high_state;
            self.band_mid = self.high_state - self.low_state;

            self.sums[0] += self.band_low * self.band_low;
            self.sums[1] += self.band_mid * self.band_mid;
            self.sums[2] += self.band_high * self.band_high;
            self.samples_in_hop += 1;

            if self.samples_in_hop == self.hop_samples {
                let n = self.hop_samples as f32;
                let energies = [self.sums[0] / n, self.sums[1] / n, self.sums[2] / n];
                let time_ms = (self.frame_index as f32 * self.hop_samples as f32 / self.sample_rate
                    * 1000.0) as u64;
                let loudest = energies.iter().cloned().fold(0.0_f32, f32::max);
                for (band, energy) in [&mut self.bass, &mut self.snare, &mut self.hihat]
                    .into_iter()
                    .zip(energies)
                {
                    if let Some(intensity) = band.step(energy, loudest) {
                        events.push(HapticEvent {
                            time_ms,
                            trigger: band.trigger,
                            intensity,
                        });
                    }
                }
                self.sums = [0.0; 3];
                self.samples_in_hop = 0;
                self.frame_index += 1;
            }
        }
        events
    }
}

/// A pulse scheduled at an absolute time.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledPulse {
    pub time_ms: u64,
    pub pulse: HapticPulse,
}

/// Convert detected events into pulses using the user's settings: disabled settings give
/// nothing, inactive triggers are skipped, and event strength scales each pulse together
/// with the master intensity.
pub fn schedule_pulses(settings: &HapticSettings, events: &[HapticEvent]) -> Vec<ScheduledPulse> {
    if !settings.enabled || !settings.is_valid() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for event in events {
        if !settings.active_triggers.contains(&event.trigger) {
            continue;
        }
        let pattern = settings.pattern_for(event.trigger);
        let mut offset = 0u64;
        for pulse in &pattern.pulses {
            offset += u64::from(pulse.delay_ms);
            out.push(ScheduledPulse {
                time_ms: event.time_ms + offset,
                pulse: HapticPulse {
                    intensity: (pulse.intensity * event.intensity * settings.master_intensity)
                        .clamp(0.0, 1.0),
                    delay_ms: 0,
                    ..pulse.clone()
                },
            });
            offset += u64::from(pulse.duration_ms);
        }
    }
    out.sort_by_key(|p| p.time_ms);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 44_100;

    fn tone(freq: f32, amp: f32, ms: u32) -> Vec<f32> {
        (0..SR * ms / 1000)
            .map(|n| amp * (2.0 * std::f32::consts::PI * freq * n as f32 / SR as f32).sin())
            .collect()
    }

    fn silence(ms: u32) -> Vec<f32> {
        vec![0.0; (SR * ms / 1000) as usize]
    }

    /// Deterministic white noise.
    fn noise(amp: f32, ms: u32) -> Vec<f32> {
        let mut x: u32 = 0x1234_5678;
        (0..SR * ms / 1000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                amp * ((x as f32 / u32::MAX as f32) * 2.0 - 1.0)
            })
            .collect()
    }

    fn run(samples: &[f32]) -> Vec<HapticEvent> {
        let mut analyzer = HapticAnalyzer::new(SR, 1).unwrap();
        // Feed in odd-sized chunks to exercise block-boundary handling.
        samples
            .chunks(1_013)
            .flat_map(|chunk| analyzer.process(chunk))
            .collect()
    }

    #[test]
    fn rejects_invalid_configuration() {
        assert!(HapticAnalyzer::new(SR, 0).is_none());
        assert!(HapticAnalyzer::new(4_000, 2).is_none());
    }

    #[test]
    fn kick_pattern_triggers_bass_hits_at_the_right_times() {
        let mut audio = silence(300);
        for _ in 0..4 {
            audio.extend(tone(60.0, 0.8, 80));
            audio.extend(silence(420));
        }
        let events = run(&audio);
        let bass: Vec<_> = events
            .iter()
            .filter(|e| e.trigger == HapticTrigger::BassHit)
            .collect();
        assert_eq!(bass.len(), 4, "{events:?}");
        for (i, hit) in bass.iter().enumerate() {
            let expected = 300 + i as u64 * 500;
            assert!(
                hit.time_ms.abs_diff(expected) <= 40,
                "{} vs {expected}",
                hit.time_ms
            );
            assert!(hit.intensity > 0.3);
        }
        assert!(events.iter().all(|e| e.trigger != HapticTrigger::HiHat));
    }

    #[test]
    fn high_frequency_noise_bursts_trigger_hihats_not_bass() {
        let mut audio = silence(200);
        for _ in 0..3 {
            audio.extend(noise(0.6, 20));
            audio.extend(silence(180));
        }
        let events = run(&audio);
        let hats = events
            .iter()
            .filter(|e| e.trigger == HapticTrigger::HiHat)
            .count();
        assert!(hats >= 3, "{events:?}");
        assert!(events.iter().all(|e| e.trigger != HapticTrigger::BassHit));
    }

    #[test]
    fn a_sustained_tone_fires_once_not_continuously() {
        let mut audio = silence(100);
        audio.extend(tone(80.0, 0.7, 2_000));
        let bass = run(&audio)
            .into_iter()
            .filter(|e| e.trigger == HapticTrigger::BassHit)
            .count();
        assert_eq!(bass, 1);
    }

    #[test]
    fn silence_and_very_quiet_audio_produce_nothing() {
        assert!(run(&silence(1_000)).is_empty());
        let mut quiet = silence(100);
        quiet.extend(tone(60.0, 0.005, 300));
        assert!(run(&quiet).is_empty());
    }

    #[test]
    fn stereo_input_is_downmixed() {
        let mono = tone(60.0, 0.8, 80);
        let mut audio = Vec::new();
        audio.extend(vec![0.0; 2 * 13_230]);
        for s in &mono {
            audio.push(*s);
            audio.push(*s);
        }
        audio.extend(vec![0.0; 2 * 20_000]);
        let mut analyzer = HapticAnalyzer::new(SR, 2).unwrap();
        let events = analyzer.process(&audio);
        assert_eq!(
            events
                .iter()
                .filter(|e| e.trigger == HapticTrigger::BassHit)
                .count(),
            1
        );
    }

    #[test]
    fn scheduling_respects_enabled_triggers_and_scales_intensity() {
        let events = vec![
            HapticEvent {
                time_ms: 100,
                trigger: HapticTrigger::BassHit,
                intensity: 1.0,
            },
            HapticEvent {
                time_ms: 200,
                trigger: HapticTrigger::HiHat,
                intensity: 1.0,
            },
            HapticEvent {
                time_ms: 300,
                trigger: HapticTrigger::Snare,
                intensity: 0.5,
            },
        ];
        let mut settings = HapticSettings::default();
        assert!(schedule_pulses(&settings, &events).is_empty(), "disabled");

        settings.enabled = true;
        settings.master_intensity = 1.0;
        let pulses = schedule_pulses(&settings, &events);
        // Default active triggers: bass and snare (hi-hat skipped).
        assert_eq!(pulses.len(), 2);
        assert_eq!(pulses[0].time_ms, 100);
        assert!((pulses[0].pulse.intensity - 0.9).abs() < 1e-6);
        assert!((pulses[1].pulse.intensity - 0.35).abs() < 1e-6);

        settings.master_intensity = 0.5;
        let softer = schedule_pulses(&settings, &events);
        assert!((softer[0].pulse.intensity - 0.45).abs() < 1e-6);
    }
}
