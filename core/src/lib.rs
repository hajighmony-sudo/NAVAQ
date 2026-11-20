//! NAVAQ's platform-independent audio domain and playback state core.
//!
//! This crate deliberately contains no UI and no platform audio implementation.
//! Android Media3 and iOS AVFoundation adapters will consume this stable command/event API.

pub mod backend;
pub mod engine;
pub mod model;
pub mod queue;

pub use engine::PlayerCore;
pub use model::{
    AudioSource, BackendAction, BackendEvent, CoreOutput, PlaybackState, PlaybackStatus,
    PlayerCommand, PlayerError, PlayerEvent, RepeatMode, Track, TrackId,
};
pub use queue::PlaybackQueue;
