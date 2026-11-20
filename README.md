# NAVAQ

NAVAQ is being developed as a cross-platform music playback core. This repository currently focuses on the platform-independent Rust domain layer; it contains no user interface.

## Workspace

- `core/` — playback state, command/event contracts, queue management, audio-session policy, snapshot recovery, equalizer/crossfade settings, library, playlists, lyrics (LRC), sleep timer, and play history.
- `android/` — native Android playback host using AndroidX Media3, MediaSessionService, and a JNI bridge to the Rust core.
- The iOS AVFoundation adapter is not yet implemented.

## Core boundary

`PlayerCore` owns deterministic playback state and queue decisions. It does not decode audio or access speakers. A native backend is responsible for preparing and rendering audio, managing OS audio sessions, and reporting callbacks such as prepared, position, buffering, and errors.

`AudioBackend` is the adapter contract. Native implementations execute ordered backend actions and return asynchronous callbacks through non-blocking event polling. The runtime connects that contract to the player core.

## Playback semantics

- Resume/Play continue from the current position and only re-prepare when the native item was released (Stop, completion, failure) or never loaded.
- Previous restarts the track after 3 s of playback; at the start of the queue it restarts the first track (or wraps with repeat-all).
- Late `Position` callbacks after Stop/completion and `Ended` callbacks that race with Pause are ignored.
- Track ids must be unique within a queue; the host should mint distinct ids if the same file appears twice.

## Playback persistence

`PlayerCore::snapshot()` captures the versioned logical playback state and the complete queue order, including shuffle state. `PlayerCore::restore()` validates the snapshot and returns the restored core plus the actions needed to re-prepare the current track.

`PlayerRuntime::restore()` additionally dispatches those actions to a supplied native backend. Restored playback is deliberately paused rather than automatically resumed; the host must re-establish its audio session and decide when to resume. Snapshot data can be serialized with Serde (for example, JSON) and stored by the host application.

## Android platform layer

The Android application module lives in `android/`. It uses Media3 ExoPlayer for native audio decoding/output and MediaSessionService for background playback and system media controls. Android audio focus and noisy-audio handling are delegated to Media3. The Rust core is compiled as a JNI shared library; `android/scripts/build-rust-android.sh` builds the `arm64-v8a` and `x86_64` libraries when the Android NDK is installed.

Build the Android APK after compiling the Rust JNI libraries:

```sh
android/scripts/build-rust-android.sh
gradle :android:assembleDebug
```

The Android service currently bridges player commands, backend preparation/buffering/position/end/error callbacks, and the supported Media3 actions. Equalizer DSP, crossfade rendering, MediaStore library scanning, and a user-facing Android interface remain separate implementation tasks; settings are not presented as applied DSP until a native processor exists.

## Development

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Pure-logic modules

- `Library` — unique tracks, upsert/remove, multi-word Unicode search, sorting, albums/artists.
- `PlaylistStore` — create/rename/delete, add/remove/move tracks, resolve to `Track`s for `SetQueue`.
- `Lyrics::parse_lrc` — synchronized lyrics with multi-timestamp lines and `[offset:]`; `line_at(position_ms)`.
- `EqualizerSettings` + presets — validated, applied through `SetEqualizer`; `SetCrossfade` (0–12000 ms).
- `SleepTimer` — duration or end-of-track, driven by `PlayerRuntime::tick` / `poll_backend`.
- `PlayHistory` — bounded recent plays and play counts; the host calls `record_play`.
- `replaygain` — `linear_gain` / `effective_volume` from ReplayGain tags (track/album mode, preamp, clipping guard).
- `m3u` — parse and write M3U/M3U8 with `#EXTINF`.
- `guess_from_filename` — track number, artist and title from file names when tags are missing.
- `BookmarkStore` — resume positions for long tracks (ignores the first 5 s and the last 10 s).
- `UserRatings` — favorites and 1-5 ratings, resolvable to tracks for `SetQueue`.

Player modes: `SetConsume` (drop tracks once played), `SetSingle` (stop after the current track; repeat-one still repeats), `SetLoopRegion` (A-B loop, cleared when the track changes), and `Reshuffle { seed }` (new random upcoming order; the current and played tracks stay put). The consume/single semantics follow MPD's playback options; the rest follows common practice in players such as MPD, Auxio and rodio-based apps. These projects were used as design references; no code was copied.

These modules do no I/O. Scanning files, reading tags and storing data are host responsibilities.

## Current implementation status

Implemented: everything in the modules above plus typed track/source models, playback state, command/event contract, queue operations (move, clear, unique ids, next-track lookup for gapless preloading), repeat/shuffle, native backend contract, asynchronous event runtime, audio-focus handling with ducking, failure recovery, versioned snapshots with safe paused restoration, and unit tests.

Not in the core (platform work): Media3/AVFoundation adapters, background services, Android Auto, CarPlay, the DSP that actually applies the equalizer, network streaming, downloads, file scanning and tag reading, and plugin loading.
