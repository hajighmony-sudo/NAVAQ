//! AVA voice-assistant domain core.
//!
//! This module is intentionally isolated from the existing playback engine. It
//! interprets short bilingual voice commands and produces a typed intent for an
//! Android host to resolve and execute. Speech recognition and music-library
//! access remain platform adapters; command interpretation is fully local.

use crate::model::{PlayerCommand, RepeatMode};
use crate::text::normalize_for_search;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AvaMusicSources {
    Device,
    InApp,
    #[default]
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AvaResponseMode {
    Silent,
    Voice,
    /// Visual-only is the safe default; AVA never speaks unless explicitly enabled.
    #[default]
    Visual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AvaLanguage {
    Persian,
    English,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AvaActivationMode {
    #[default]
    PushToTalk,
    WakeWord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvaSettings {
    pub enabled: bool,
    pub music_sources: AvaMusicSources,
    pub response_mode: AvaResponseMode,
    pub language: AvaLanguage,
    pub activation_mode: AvaActivationMode,
    pub confirm_music_before_playback: bool,
}

impl Default for AvaSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            music_sources: AvaMusicSources::Both,
            response_mode: AvaResponseMode::Visual,
            language: AvaLanguage::Auto,
            activation_mode: AvaActivationMode::PushToTalk,
            confirm_music_before_playback: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvaExecutionRoute {
    Local,
    OfflineFallback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AvaIntent {
    Transport { command: PlayerCommand },
    PlayRandom,
    SearchMusic { query: String },
    Unknown { transcript: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AvaDecision {
    pub intent: AvaIntent,
    pub route: AvaExecutionRoute,
    pub response_mode: AvaResponseMode,
    pub normalized_transcript: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AvaCore;

impl AvaCore {
    pub fn new() -> Self {
        Self
    }

    /// Interpret a recognized transcript using only deterministic local rules.
    pub fn interpret(
        &self,
        transcript: &str,
        settings: &AvaSettings,
        _network_available: bool,
    ) -> AvaDecision {
        let normalized = clean_transcript(&normalize_for_search(transcript));
        let intent = parse_intent(&normalized, transcript);
        let is_deterministic = !matches!(intent, AvaIntent::Unknown { .. });
        let route = if is_deterministic {
            AvaExecutionRoute::Local
        } else {
            AvaExecutionRoute::OfflineFallback
        };

        AvaDecision {
            intent,
            route,
            response_mode: settings.response_mode,
            normalized_transcript: normalized,
        }
    }
}

fn clean_transcript(input: &str) -> String {
    input
        .trim()
        .trim_matches(|c: char| c.is_ascii_punctuation() || "،؛؟.!؟".contains(c))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_intent(normalized: &str, original: &str) -> AvaIntent {
    // Accept an optional spoken wake phrase when AVA is already in a
    // user-started listening session. This is command-prefix handling, not
    // background microphone activation.
    let mut value = normalized.trim();
    for prefix in [
        "هی آوا، ",
        "هی آوا ",
        "اوکی آوا، ",
        "اوکی آوا ",
        "hey ava, ",
        "hey ava ",
        "okay ava, ",
        "okay ava ",
        "ok ava, ",
        "ok ava ",
        "آوا، ",
        "آوا ",
    ] {
        if let Some(command) = value.strip_prefix(prefix) {
            value = command.trim();
            break;
        }
    }
    if value.is_empty() {
        return AvaIntent::Unknown {
            transcript: original.to_owned(),
        };
    }

    if contains_any(
        value,
        &["آهنگ بعدی", "بعدی", "next song", "next track", "next"],
    ) {
        return AvaIntent::Transport {
            command: PlayerCommand::Next,
        };
    }
    if contains_any(
        value,
        &[
            "آهنگ قبلی",
            "قبلی",
            "previous song",
            "previous track",
            "previous",
        ],
    ) {
        return AvaIntent::Transport {
            command: PlayerCommand::Previous,
        };
    }
    if contains_any(value, &["نگه دار", "مکث", "توقف موقت", "pause", "hold on"]) {
        return AvaIntent::Transport {
            command: PlayerCommand::Pause,
        };
    }
    if contains_any(
        value,
        &["ادامه بده", "ادامه", "از سر بگیر", "resume", "continue"],
    ) {
        return AvaIntent::Transport {
            command: PlayerCommand::Resume,
        };
    }
    if contains_any(
        value,
        &["متوقف کن", "توقف", "قطع کن", "stop playback", "stop"],
    ) {
        return AvaIntent::Transport {
            command: PlayerCommand::Stop,
        };
    }

    if contains_any(
        value,
        &[
            "shuffle on",
            "turn shuffle on",
            "حالت تصادفی روشن",
            "تصادفی کن",
        ],
    ) {
        return AvaIntent::Transport {
            command: PlayerCommand::SetShuffle(true),
        };
    }
    if contains_any(
        value,
        &["shuffle off", "turn shuffle off", "حالت تصادفی خاموش"],
    ) {
        return AvaIntent::Transport {
            command: PlayerCommand::SetShuffle(false),
        };
    }
    if contains_any(value, &["repeat one", "تکرار همین آهنگ", "تکرار آهنگ"]) {
        return AvaIntent::Transport {
            command: PlayerCommand::SetRepeatMode(RepeatMode::One),
        };
    }
    if contains_any(value, &["repeat all", "تکرار همه", "تکرار صف"]) {
        return AvaIntent::Transport {
            command: PlayerCommand::SetRepeatMode(RepeatMode::All),
        };
    }
    if contains_any(value, &["repeat off", "تکرار خاموش", "بدون تکرار"]) {
        return AvaIntent::Transport {
            command: PlayerCommand::SetRepeatMode(RepeatMode::Off),
        };
    }
    if contains_any(value, &["mute", "بی صدا", "صدا قطع"]) {
        return AvaIntent::Transport {
            command: PlayerCommand::SetVolume(0.0),
        };
    }
    if let Some(volume) = parse_volume(value) {
        return AvaIntent::Transport {
            command: PlayerCommand::SetVolume(volume),
        };
    }

    if contains_any(
        value,
        &[
            "یک آهنگ پخش کن",
            "یه آهنگ پخش کن",
            "آهنگ پخش کن",
            "موسیقی پخش کن",
            "یه چیزی پخش کن",
            "پخش تصادفی",
            "random song",
            "play a song",
            "play music",
            "play something",
            "shuffle music",
            "play",
            "پخش کن",
        ],
    ) {
        return AvaIntent::PlayRandom;
    }

    // Natural Persian orderings: "فلان آهنگ رو پخش کن", "پخش کن آهنگ فلان".
    for suffix in [" رو پخش کن", " را پخش کن", " پخش کن"] {
        if let Some(query) = value.strip_suffix(suffix) {
            let query = query.trim();
            let query = query
                .strip_prefix("آهنگ ")
                .or_else(|| query.strip_prefix("موسیقی "))
                .unwrap_or(query);
            let query = query.strip_suffix(" آهنگ").unwrap_or(query).trim();
            if !query.is_empty() && query != "آهنگ" && query != "موسیقی" {
                return AvaIntent::SearchMusic {
                    query: query.to_owned(),
                };
            }
        }
    }

    for prefix in [
        "پخش کن آهنگ ",
        "پخش کن موسیقی ",
        "پخش آهنگ ",
        "پخش موسیقی ",
        "آهنگ ",
        "موسیقی ",
        "play song ",
        "play track ",
        "play ",
    ] {
        if let Some(query) = value.strip_prefix(prefix) {
            let query = query
                .trim()
                .trim_end_matches(" رو پخش کن")
                .trim_end_matches(" را پخش کن")
                .trim_end_matches(" پخش کن")
                .trim();
            if !query.is_empty() {
                return AvaIntent::SearchMusic {
                    query: query.to_owned(),
                };
            }
        }
    }

    AvaIntent::Unknown {
        transcript: original.to_owned(),
    }
}

fn parse_volume(value: &str) -> Option<f32> {
    let value = value
        .strip_prefix("set volume ")
        .or_else(|| value.strip_prefix("volume to "))
        .or_else(|| value.strip_prefix("volume "))
        .or_else(|| value.strip_prefix("بلندی صدا "))
        .or_else(|| value.strip_prefix("صدای "))
        .or_else(|| value.strip_prefix("صدا "))?
        .trim()
        .trim_end_matches('%')
        .trim_end_matches("percent")
        .trim_end_matches("درصد")
        .trim();
    let percent = value.parse::<f32>().ok()?;
    if !(0.0..=100.0).contains(&percent) {
        return None;
    }
    Some(percent / 100.0)
}

fn contains_any(value: &str, candidates: &[&str]) -> bool {
    candidates.contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_ava_product_decisions() {
        let settings = AvaSettings::default();
        assert_eq!(settings.music_sources, AvaMusicSources::Both);
        assert_eq!(settings.response_mode, AvaResponseMode::Visual);
        assert_eq!(settings.activation_mode, AvaActivationMode::PushToTalk);
    }

    #[test]
    fn parses_persian_and_english_transport_commands() {
        let core = AvaCore::new();
        let settings = AvaSettings::default();
        for (phrase, expected) in [
            ("آهنگ بعدی", PlayerCommand::Next),
            ("آهنگ قبلی", PlayerCommand::Previous),
            ("نگه دار", PlayerCommand::Pause),
            ("ادامه بده", PlayerCommand::Resume),
            ("توقف", PlayerCommand::Stop),
            ("next", PlayerCommand::Next),
            ("pause", PlayerCommand::Pause),
            ("resume", PlayerCommand::Resume),
        ] {
            let decision = core.interpret(phrase, &settings, false);
            assert_eq!(decision.intent, AvaIntent::Transport { command: expected });
            assert_eq!(decision.route, AvaExecutionRoute::Local);
        }
    }

    #[test]
    fn accepts_optional_ava_wake_phrase_before_commands() {
        let core = AvaCore::new();
        let settings = AvaSettings::default();
        for phrase in ["هی آوا، آهنگ بعدی", "Hey Ava, next song", "اوکی آوا بعدی"]
        {
            assert_eq!(
                core.interpret(phrase, &settings, false).intent,
                AvaIntent::Transport {
                    command: PlayerCommand::Next
                }
            );
        }
        assert_eq!(
            core.interpret("هی آوا آهنگ آرام رو پخش کن", &settings, false)
                .intent,
            AvaIntent::SearchMusic {
                query: "آرام".to_owned()
            }
        );
    }

    #[test]
    fn parses_random_and_named_music_requests() {
        let core = AvaCore::new();
        let settings = AvaSettings::default();
        assert_eq!(
            core.interpret("یه آهنگ پخش کن", &settings, false).intent,
            AvaIntent::PlayRandom
        );
        assert_eq!(
            core.interpret("play music", &settings, false).intent,
            AvaIntent::PlayRandom
        );
        assert_eq!(
            core.interpret("play song hello", &settings, false).intent,
            AvaIntent::SearchMusic {
                query: "hello".to_owned()
            }
        );
        assert_eq!(
            core.interpret("آهنگ hello رو پخش کن", &settings, false)
                .intent,
            AvaIntent::SearchMusic {
                query: "hello".to_owned()
            }
        );
    }

    #[test]
    fn parses_natural_persian_named_track_orderings() {
        let core = AvaCore::new();
        let settings = AvaSettings::default();
        for phrase in [
            "فلان آهنگ رو پخش کن",
            "فلان آهنگ را پخش کن",
            "پخش کن آهنگ فلان",
        ] {
            assert_eq!(
                core.interpret(phrase, &settings, false).intent,
                AvaIntent::SearchMusic {
                    query: "فلان".to_owned()
                }
            );
        }
    }

    #[test]
    fn parses_shuffle_repeat_and_volume_controls() {
        let core = AvaCore::new();
        let settings = AvaSettings::default();
        assert_eq!(
            core.interpret("shuffle on", &settings, false).intent,
            AvaIntent::Transport {
                command: PlayerCommand::SetShuffle(true),
            }
        );
        assert_eq!(
            core.interpret("repeat all", &settings, false).intent,
            AvaIntent::Transport {
                command: PlayerCommand::SetRepeatMode(RepeatMode::All),
            }
        );
        assert_eq!(
            core.interpret("volume 50", &settings, false).intent,
            AvaIntent::Transport {
                command: PlayerCommand::SetVolume(0.5),
            }
        );
        assert_eq!(
            core.interpret("mute", &settings, false).intent,
            AvaIntent::Transport {
                command: PlayerCommand::SetVolume(0.0),
            }
        );
    }

    #[test]
    fn unknown_requests_never_use_online_ai_route() {
        let core = AvaCore::new();
        let settings = AvaSettings::default();
        assert_eq!(
            core.interpret("یک درخواست پیچیده", &settings, true).route,
            AvaExecutionRoute::OfflineFallback
        );
        assert_eq!(
            core.interpret("یک درخواست پیچیده", &settings, false).route,
            AvaExecutionRoute::OfflineFallback
        );
    }

    #[test]
    fn voice_response_is_only_enabled_by_explicit_setting() {
        let core = AvaCore::new();
        let mut settings = AvaSettings::default();
        assert_eq!(
            core.interpret("next", &settings, false).response_mode,
            AvaResponseMode::Visual
        );
        settings.response_mode = AvaResponseMode::Voice;
        assert_eq!(
            core.interpret("next", &settings, false).response_mode,
            AvaResponseMode::Voice
        );
    }
}
