//! NAVAQ's platform-independent audio domain and playback state core.
//!
//! This crate deliberately contains no UI or platform audio implementation.
//! Native Android and iOS adapters consume its command/event API.

pub mod audio_session;
pub mod backend;
pub mod bookmarks;
pub mod engine;
pub mod dsp;
pub mod equalizer;
pub mod history;
pub mod library;
pub mod lyrics;
pub mod m3u;
pub mod metadata;
pub mod model;
pub mod persistence;
pub mod playlist;
pub mod queue;
pub mod ratings;
pub mod replaygain;
pub mod runtime;
pub mod sleep_timer;

#[cfg(target_os = "android")]
mod android_jni;

pub use audio_session::{AudioFocusChange, AudioSessionAction, AudioSessionPolicy};
pub use backend::{dispatch_output, AudioBackend, BackendDispatchError, BackendError};
pub use bookmarks::BookmarkStore;
pub use engine::PlayerCore;
pub use dsp::{mix_equal_power_crossfade, DspError, DspProcessor};
pub use equalizer::{EqualizerBand, EqualizerPreset, EqualizerSettings};
pub use history::PlayHistory;
pub use library::{AlbumSummary, Library, SortKey};
pub use lyrics::{LyricLine, Lyrics};
pub use m3u::M3uEntry;
pub use metadata::{guess_from_filename, GuessedMetadata};
pub use model::{
    AudioSource, BackendAction, BackendEvent, CoreOutput, LoopRegion, PlaybackState,
    PlaybackStatus, PlayerCommand, PlayerError, PlayerEvent, RepeatMode, Track, TrackId,
};
pub use persistence::{PlayerSnapshot, SnapshotError, PLAYER_SNAPSHOT_VERSION};
pub use playlist::{Playlist, PlaylistError, PlaylistStore};
pub use queue::{PlaybackQueue, PlaybackQueueSnapshot, QueueSnapshotError};
pub use ratings::{RatingOutOfRange, UserRatings};
pub use replaygain::{
    effective_volume, linear_gain, ReplayGainMode, ReplayGainSettings, ReplayGainTags,
};
pub use runtime::{PlayerRuntime, RuntimeError};
pub use sleep_timer::SleepTimer;
