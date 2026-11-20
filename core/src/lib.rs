//! NAVAQ's platform-independent audio domain and playback state core.
//!
//! This crate deliberately contains no UI and no platform audio implementation.
//! Android Media3 and iOS AVFoundation adapters will consume this stable command/event API.

pub mod audio_session;
pub mod backend;
pub mod engine;
pub mod equalizer;
pub mod history;
pub mod library;
pub mod lyrics;
pub mod model;
pub mod persistence;
pub mod playlist;
pub mod queue;
pub mod runtime;
pub mod sleep_timer;

pub use audio_session::{AudioFocusChange, AudioSessionAction, AudioSessionPolicy};
pub use backend::{dispatch_output, AudioBackend, BackendDispatchError, BackendError};
pub use engine::PlayerCore;
pub use equalizer::{EqualizerBand, EqualizerPreset, EqualizerSettings};
pub use history::PlayHistory;
pub use library::{AlbumSummary, Library, SortKey};
pub use lyrics::{LyricLine, Lyrics};
pub use model::{
    AudioSource, BackendAction, BackendEvent, CoreOutput, PlaybackState, PlaybackStatus,
    PlayerCommand, PlayerError, PlayerEvent, RepeatMode, Track, TrackId,
};
pub use persistence::{PlayerSnapshot, SnapshotError, PLAYER_SNAPSHOT_VERSION};
pub use playlist::{Playlist, PlaylistError, PlaylistStore};
pub use queue::{PlaybackQueue, PlaybackQueueSnapshot, QueueSnapshotError};
pub use runtime::{PlayerRuntime, RuntimeError};
pub use sleep_timer::SleepTimer;
