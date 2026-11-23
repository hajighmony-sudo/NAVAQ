//! NAVAQ's platform-independent audio domain and playback state core.
//!
//! This crate deliberately contains no UI or platform audio implementation.
//! Native Android and iOS adapters consume its command/event API.

pub mod audio_analysis;
pub mod audio_pipeline;
pub mod audio_session;
pub mod ava;
pub mod backend;
pub mod bookmarks;
pub mod composer;
pub mod composition;
pub mod creative;
pub mod cue;
pub mod discover;
pub mod discover_catalog;
pub mod discover_session;
pub mod download_queue;
pub mod downloads;
pub mod dsp;
pub mod engine;
pub mod equalizer;
pub mod haptic;
pub mod haptic_analyzer;
pub mod history;
pub mod library;
pub mod lyrics;
pub mod m3u;
pub mod matching;
pub mod melody;
pub mod metadata;
pub mod model;
pub mod music_dna;
pub mod persistence;
pub mod playlist;
pub mod queue;
pub mod ratings;
pub mod replaygain;
pub mod room_control;
pub mod runtime;
pub mod scrobble;
pub mod sleep_timer;
pub mod social;
pub mod spatial;
pub mod spatial_render;
pub mod text;
pub mod ui_settings;

#[cfg(target_os = "android")]
mod android_jni;

pub use audio_analysis::{
    AudioAnalysisError, AudioAnalysisSettings, AudioOnset, AudioOnsetAnalyzer,
};
pub use audio_pipeline::{
    EchoSettings, PcmAudioPipeline, PcmFormat, PcmPipelineError, PcmPipelineSettings,
    PcmProcessStats,
};
pub use audio_session::{AudioFocusChange, AudioSessionAction, AudioSessionPolicy};
pub use ava::{
    AvaActivationMode, AvaCore, AvaDecision, AvaExecutionRoute, AvaIntent, AvaLanguage,
    AvaMusicSources, AvaResponseMode, AvaSettings,
};
pub use backend::{dispatch_output, AudioBackend, BackendDispatchError, BackendError};
pub use bookmarks::BookmarkStore;
pub use composer::{ComposerError, Composition, Instrument, InstrumentTrack, NoteEvent};
pub use composition::{
    chord_pitches, ChordQuality, Composition as SongComposition, CompositionError, DrumPattern,
    Instrument as SynthInstrument, Note, Part,
};
pub use creative::{
    CreativeError, CreativeJob, CreativeRequest, CreativeResult, CreativeStore,
    InstrumentalRequest, JobId, JobStatus, LyricToMelodyRequest, StemKind,
};
pub use cue::{parse_cue, CueSheet, CueTrack};
pub use discover::{
    DiscoverError, DiscoverProvider, DiscoverResult, DiscoverSearchPage, DiscoverSearchRequest,
};
pub use discover_catalog::{merge_discover_pages, DiscoverProviderRegistry};
pub use discover_session::{
    plan_provider_searches, DiscoverProviderFailure, DiscoverProviderSearch, DiscoverSession,
};
pub use download_queue::{DownloadManagerExt, DownloadSummary};
pub use downloads::{
    validate_http_url, DownloadError, DownloadId, DownloadManager, DownloadRequest, DownloadStatus,
    DownloadTask,
};
pub use dsp::{mix_equal_power_crossfade, DspError, DspProcessor, EchoProcessor};
pub use engine::PlayerCore;
pub use equalizer::{EqualizerBand, EqualizerPreset, EqualizerSettings};
pub use haptic::{HapticError, HapticPattern, HapticPulse, HapticSettings, HapticTrigger};
pub use haptic_analyzer::{schedule_pulses, HapticAnalyzer, HapticEvent, ScheduledPulse};
pub use history::PlayHistory;
pub use library::{AlbumSummary, Library, SortKey};
pub use lyrics::{LyricLine, Lyrics};
pub use matching::{
    build_shared_playlist, shared_artists, shared_genres, suggest_friends, FriendSuggestion,
    PlaylistParticipant,
};
pub use melody::{
    count_syllables, generate as generate_melody, generate_from_request as melody_from_request,
    parse_key, MelodyError, MelodyOptions, MelodyStyle, Mode,
};
pub use metadata::{guess_from_filename, GuessedMetadata};
pub use model::{
    AudioSource, BackendAction, BackendEvent, CoreOutput, LoopRegion, PlaybackState,
    PlaybackStatus, PlayerCommand, PlayerError, PlayerEvent, RepeatMode, Track, TrackId,
};
pub use music_dna::{
    rank_collaborative_tracks, rank_matches, rank_opted_in_matches, CollaborativeTrackCandidate,
    CollaborativeTrackResult, DnaBuilder, MatchResult, TasteCandidate, TasteProfile,
};
pub use persistence::{PlayerSnapshot, SnapshotError, PLAYER_SNAPSHOT_VERSION};
pub use playlist::{Playlist, PlaylistError, PlaylistStore};
pub use queue::{PlaybackQueue, PlaybackQueueSnapshot, QueueSnapshotError};
pub use ratings::{RatingOutOfRange, UserRatings};
pub use replaygain::{
    effective_volume, linear_gain, ReplayGainMode, ReplayGainSettings, ReplayGainTags,
};
pub use room_control::{ControlCommand, ControlMode, DriftAction};
pub use runtime::{PlayerRuntime, RuntimeError};
pub use scrobble::ScrobbleTracker;
pub use sleep_timer::SleepTimer;
pub use social::{
    calculate_room_sync, EffectStack, LiveEffect, ParticipantId, RoomCommand, RoomId, RoomStore,
    RoomSyncDecision, RoomSyncPolicy, SharedRoom, SocialError,
};
pub use spatial::{
    Absorption, RoomDimensions, SpatialError, SpatialSettings, SpeakerLayout, SpeakerPosition,
};
pub use spatial_render::{render as render_spatial, SpatialRendering, SpeakerFeed};
pub use text::{fuzzy_score, normalize_for_search};
pub use ui_settings::{
    CoverShape, Density, ListStyle, NavStyle, ThemeMode, UiPreset, UiSettings, UiSettingsError,
    Wallpaper,
};
