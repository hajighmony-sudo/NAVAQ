//! NAVAQ's platform-independent audio domain and playback state core.
//!
//! This crate deliberately contains no UI and no platform audio implementation.
//! Android Media3 and iOS AVFoundation adapters will consume this stable command/event API.

pub mod audio_session;
pub mod backend;
pub mod engine;
pub mod model;
pub mod persistence;
pub mod queue;
pub mod runtime;

pub use audio_session::{AudioFocusChange, AudioSessionAction, AudioSessionPolicy};
pub use engine::PlayerCore;
pub use model::{
    AudioSource, BackendAction, BackendEvent, CoreOutput, PlaybackState, PlaybackStatus,
    PlayerCommand, PlayerError, PlayerEvent, RepeatMode, Track, TrackId,
};
pub use persistence::{PlayerSnapshot, SnapshotError, PLAYER_SNAPSHOT_VERSION};
pub use queue::{PlaybackQueue, PlaybackQueueSnapshot, QueueSnapshotError};
pub use runtime::{PlayerRuntime, RuntimeError};
