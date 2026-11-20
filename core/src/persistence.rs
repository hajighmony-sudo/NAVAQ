//! Versioned, platform-neutral playback snapshots for process-death recovery.

use crate::model::PlaybackState;
use crate::queue::{PlaybackQueueSnapshot, QueueSnapshotError};
use thiserror::Error;

pub const PLAYER_SNAPSHOT_VERSION: u32 = 1;

/// Complete durable state required to reconstruct the logical player.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlayerSnapshot {
    pub version: u32,
    pub state: PlaybackState,
    pub queue: PlaybackQueueSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SnapshotError {
    #[error("unsupported player snapshot version: {0}")]
    UnsupportedVersion(u32),
    #[error(transparent)]
    InvalidQueue(#[from] QueueSnapshotError),
    #[error("snapshot current track does not match the queue selection")]
    CurrentTrackMismatch,
    #[error("snapshot contains invalid playback settings")]
    InvalidPlaybackSettings,
}

