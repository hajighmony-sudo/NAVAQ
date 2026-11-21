//! NAVAQ's platform-independent audio domain and playback state core.
//!
//! This crate deliberately contains no UI or platform audio implementation.
//! Native Android and iOS adapters consume its command/event API.

pub mod audio_session;
pub mod backend;
pub mod bookmarks;
pub mod composer;
pub mod creative;
pub mod cue;
pub mod dsp;
pub mod engine;
pub mod equalizer;
pub mod haptic;
pub mod history;
pub mod library;
pub mod lyrics;
pub mod m3u;
pub mod metadata;
pub mod model;
pub mod music_dna;
pub mod persistence;
pub mod playlist;
pub mod queue;
pub mod ratings;
pub mod replaygain;
pub mod runtime;
pub mod scrobble;
pub mod sleep_timer;
pub mod social;
pub mod spatial;
pub mod text;

#[cfg(target_os = "android")]
mod android_jni;

pub use audio_session::{AudioFocusChange, AudioSessionAction, AudioSessionPolicy};
pub use backend::{dispatch_output, AudioBackend, BackendDispatchError, BackendError};
pub use bookmarks::BookmarkStore;
pub use composer::{Composition, ComposerError, Instrument, InstrumentTrack, NoteEvent};
pub use creative::{
    CreativeError, CreativeJob, CreativeRequest, CreativeResult, CreativeStore,
    InstrumentalRequest, JobId, JobStatus, LyricToMelodyRequest, StemKind,
};
pub use cue::{parse_cue, CueSheet, CueTrack};
pub use dsp::{mix_equal_power_crossfade, DspError, DspProcessor};
pub use engine::PlayerCore;
pub use equalizer::{EqualizerBand, EqualizerPreset, EqualizerSettings};
pub use haptic::{HapticError, HapticPattern, HapticPulse, HapticSettings, HapticTrigger};
pub use history::PlayHistory;
pub use library::{AlbumSummary, Library, SortKey};
pub use lyrics::{LyricLine, Lyrics};
pub use m3u::M3uEntry;
pub use metadata::{guess_from_filename, GuessedMetadata};
pub use model::{
    AudioSource, BackendAction, BackendEvent, CoreOutput, LoopRegion, PlaybackState,
    PlaybackStatus, PlayerCommand, PlayerError, PlayerEvent, RepeatMode, Track, TrackId,
};
pub use music_dna::{rank_matches, DnaBuilder, MatchResult, TasteProfile};
pub use persistence::{PlayerSnapshot, SnapshotError, PLAYER_SNAPSHOT_VERSION};
pub use playlist::{Playlist, PlaylistError, PlaylistStore};
pub use queue::{PlaybackQueue, PlaybackQueueSnapshot, QueueSnapshotError};
pub use ratings::{RatingOutOfRange, UserRatings};
pub use replaygain::{
    effective_volume, linear_gain, ReplayGainMode, ReplayGainSettings, ReplayGainTags,
};
pub use runtime::{PlayerRuntime, RuntimeError};
pub use scrobble::ScrobbleTracker;
pub use sleep_timer::SleepTimer;
pub use social::{
    EffectStack, LiveEffect, ParticipantId, RoomId, RoomStore, SharedRoom, SocialError,
};
pub use spatial::{
    Absorption, RoomDimensions, SpatialError, SpatialSettings, SpeakerLayout, SpeakerPosition,
};
pub use text::{fuzzy_score, normalize_for_search};
