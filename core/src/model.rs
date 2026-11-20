use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
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
    SetVolume(f32),
    SetSpeed(f32),
    SetRepeatMode(RepeatMode),
    SetShuffle(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendAction {
    Prepare(Track),
    Play,
    Pause,
    Stop,
    SeekTo(u64),
    SetVolume(f32),
    SetSpeed(f32),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CoreOutput {
    pub backend_actions: Vec<BackendAction>,
    pub events: Vec<PlayerEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    StateChanged(PlaybackState),
    TrackChanged(Option<TrackId>),
    QueueChanged,
    PlaybackStarted,
    PlaybackPaused,
    PlaybackStopped,
    PlaybackCompleted,
}

#[derive(Debug, Error, PartialEq)]
pub enum PlayerError {
    #[error("volume must be a finite value between 0.0 and 1.0")]
    InvalidVolume,
    #[error("speed must be a finite value between 0.25 and 4.0")]
    InvalidSpeed,
    #[error("track was not found in the playback queue")]
    TrackNotFound,
    #[error("playback queue is empty")]
    EmptyQueue,
}
