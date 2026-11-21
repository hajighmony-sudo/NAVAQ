use crate::equalizer::EqualizerSettings;
use crate::haptic::HapticSettings;
use crate::spatial::SpatialSettings;
use serde::{Deserialize, Serialize};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackId(pub String);

impl From<&str> for TrackId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AudioSource {
    LocalFile(String),
    RemoteUrl(String),
    ContentUri(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub source: AudioSource,
}

impl Track {
    /// Validate untrusted track metadata before it enters the playback queue.
    pub fn is_valid(&self) -> bool {
        let source_is_valid = match &self.source {
            AudioSource::LocalFile(value)
            | AudioSource::RemoteUrl(value)
            | AudioSource::ContentUri(value) => !value.trim().is_empty() && value.len() <= 4096,
        };
        !self.id.0.trim().is_empty()
            && self.id.0.len() <= 256
            && !self.title.trim().is_empty()
            && self.title.len() <= 512
            && self.artist.as_ref().map_or(true, |value| value.len() <= 256)
            && self.album.as_ref().map_or(true, |value| value.len() <= 256)
            && self.duration_ms.map_or(true, |duration| duration > 0)
            && source_is_valid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Playing,
    Paused,
    Stopped,
    Completed,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

/// A-B repeat region inside the current track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopRegion {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackState {
    pub status: PlaybackStatus,
    pub current_track: Option<TrackId>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub speed: f32,
    pub repeat_mode: RepeatMode,
    pub shuffle_enabled: bool,
    pub is_buffering: bool,
    #[serde(default)]
    pub equalizer: EqualizerSettings,
    /// Crossfade between tracks in milliseconds (0 = off).
    #[serde(default)]
    pub crossfade_ms: u32,
    /// Remove a track from the queue once playback has moved past it.
    #[serde(default)]
    pub consume: bool,
    /// Stop after the current track instead of advancing (repeat-one still repeats it).
    #[serde(default)]
    pub single: bool,
    /// Active A-B loop; cleared whenever the current track changes.
    #[serde(default)]
    pub loop_region: Option<LoopRegion>,
    /// Advanced haptic feedback configuration.
    #[serde(default)]
    pub haptic: HapticSettings,
    /// Personalized spatial / room simulation settings.
    #[serde(default)]
    pub spatial: SpatialSettings,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Idle,
            current_track: None,
            position_ms: 0,
            duration_ms: None,
            volume: 1.0,
            speed: 1.0,
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            is_buffering: false,
            equalizer: EqualizerSettings::default(),
            crossfade_ms: 0,
            consume: false,
            single: false,
            loop_region: None,
            haptic: HapticSettings::default(),
            spatial: SpatialSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlayerCommand {
    Play,
    Pause,
    Resume,
    Stop,
    Seek {
        position_ms: u64,
    },
    Next,
    Previous,
    PlayTrack(TrackId),
    SetQueue {
        tracks: Vec<Track>,
        start_index: usize,
    },
    AddToQueue(Track),
    PlayNext(Track),
    RemoveFromQueue(TrackId),
    /// Move a queued track to `new_index` in queue (not shuffled playback) order.
    MoveInQueue {
        track_id: TrackId,
        new_index: usize,
    },
    /// Remove every track and reset the player to idle.
    ClearQueue,
    SetVolume(f32),
    SetSpeed(f32),
    SetRepeatMode(RepeatMode),
    SetShuffle(bool),
    SetEqualizer(EqualizerSettings),
    /// Crossfade length in milliseconds, 0 to 12000.
    SetCrossfade(u32),
    SetConsume(bool),
    SetSingle(bool),
    /// Set or clear (`None`) the A-B loop of the current track.
    SetLoopRegion(Option<LoopRegion>),
    /// Re-randomize the remaining shuffled order with the given seed (no-op without shuffle).
    Reshuffle {
        seed: u64,
    },
    /// Advanced haptic feedback settings.
    SetHaptic(HapticSettings),
    /// Personalized spatial audio / room simulation settings.
    SetSpatial(SpatialSettings),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BackendAction {
    Prepare(Track),
    Play,
    Pause,
    Stop,
    SeekTo(u64),
    SetVolume(f32),
    SetSpeed(f32),
    SetEqualizer(EqualizerSettings),
    SetCrossfade(u32),
    SetHaptic(HapticSettings),
    SetSpatial(SpatialSettings),
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CoreOutput {
    pub backend_actions: Vec<BackendAction>,
    pub events: Vec<PlayerEvent>,
}

/// Events reported asynchronously by a platform audio backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BackendEvent {
    Prepared {
        track_id: TrackId,
        duration_ms: Option<u64>,
    },
    Position {
        track_id: TrackId,
        position_ms: u64,
    },
    Buffering {
        track_id: TrackId,
        is_buffering: bool,
    },
    Ended {
        track_id: TrackId,
    },
    Failed {
        track_id: TrackId,
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlayerEvent {
    StateChanged(PlaybackState),
    TrackChanged(Option<TrackId>),
    QueueChanged,
    PlaybackStarted,
    PlaybackPaused,
    PlaybackStopped,
    PlaybackCompleted,
    PlaybackError {
        code: String,
        message: String,
    },
    /// The track was listened to long enough to count as a play (scrobble).
    ScrobbleReady(TrackId),
}

#[derive(Debug, Error, PartialEq)]
pub enum PlayerError {
    #[error("volume must be a finite value between 0.0 and 1.0")]
    InvalidVolume,
    #[error("speed must be a finite value between 0.25 and 4.0")]
    InvalidSpeed,
    #[error("track metadata or source is invalid")]
    InvalidTrack,
    #[error("track was not found in the playback queue")]
    TrackNotFound,
    #[error("playback queue is empty")]
    EmptyQueue,
    #[error("queue start index is outside the queue")]
    InvalidQueueStartIndex,
    #[error("queue index is outside the queue")]
    InvalidQueueIndex,
    #[error("a track with the same id is already in the playback queue")]
    DuplicateTrack,
    #[error("equalizer needs 1-32 bands with positive frequencies and gains within -12..12 dB")]
    InvalidEqualizer,
    #[error("crossfade must be between 0 and 12000 ms")]
    InvalidCrossfade,
    #[error("loop region must start before it ends and fit inside the current track")]
    InvalidLoopRegion,
    #[error("haptic settings or patterns are invalid")]
    InvalidHaptic,
    #[error("spatial audio / room settings are invalid")]
    InvalidSpatial,
}
