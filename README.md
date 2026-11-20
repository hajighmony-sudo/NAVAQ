# NAVAQ

NAVAQ is being developed as a cross-platform music playback core. This repository currently focuses on the platform-independent Rust domain layer; it contains no user interface.

## Workspace

- `core/` — playback state, command/event contracts, queue management, audio-session policy, snapshot recovery, equalizer/crossfade settings, library, playlists, lyrics (LRC), sleep timer, play history, **and exclusive features** (Shared Listening Rooms, Music DNA matching, creative tools, advanced haptics, personalized spatial audio).
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

The Android service currently bridges player commands, backend preparation/buffering/position/end/error callbacks, and supported Media3 actions. The platform-independent core now includes stateful PCM equalizer/gain processing and equal-power crossfade mixing primitives in `core/src/dsp.rs`. These algorithms are ready for a native PCM pipeline, but the Android Media3 service does not yet insert them into its decoding/output path. MediaStore library scanning, durable Android storage wiring, and a user-facing Android interface also remain host-integration tasks.

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
- `dsp` — stateful multi-band peaking-EQ and gain processing for interleaved f32 PCM, plus equal-power crossfade mixing; native backends own PCM pipeline integration.
- `m3u` — parse and write M3U/M3U8 with `#EXTINF`.
- `guess_from_filename` — track number, artist and title from file names when tags are missing.
- `BookmarkStore` — resume positions for long tracks (ignores the first 5 s and the last 10 s).
- `UserRatings` — favorites and 1-5 ratings, resolvable to tracks for `SetQueue`.

### Exclusive / differentiating modules

- **`social`** — Shared Listening Rooms with simultaneous control. Multiple participants share a track; anyone can apply live effects (Echo, Reverb, PitchShift, VocalOverlay, Custom). `RoomStore` manages rooms, host promotion, per-participant effect stacks. Networking & actual audio mixing are host responsibilities.
- **`music_dna`** — Taste profiles built from play history + ratings + optional genre/tempo tags. Cosine similarity matching to discover users with very close taste and build collaborative playlists (`rank_matches`, `DnaBuilder`, `TasteProfile`).
- **`creative`** — Lyric-to-Melody generator requests, instrumental / stem-separation jobs (`CreativeStore`, `LyricToMelodyRequest`, `InstrumentalRequest`). The core tracks job lifecycle; the host (local model or cloud) produces the audio and returns an ordinary `AudioSource` that can be enqueued.
- **`haptic`** — Advanced haptic patterns driven by musical events (BassHit, Snare, HiHat, VocalOnset, Beat). Intensity, duration, sharpness, custom patterns. Host maps patterns onto device vibrator / controller / wearable.
- **`spatial`** — Personalized spatial audio. Virtual room dimensions, absorption, speaker layouts (Stereo / 5.1 / Custom), listener position, HRTF profile and reverb mix. Host renders the actual binaural / spatial stream.

Player modes: `SetConsume` (drop tracks once played), `SetSingle` (stop after the current track; repeat-one still repeats), `SetLoopRegion` (A-B loop, cleared when the track changes), and `Reshuffle { seed }` (new random upcoming order; the current and played tracks stay put). The consume/single semantics follow MPD's playback options; the rest follows common practice in players such as MPD, Auxio and rodio-based apps. These projects were used as design references; no code was copied.

These modules do no I/O. Scanning files, reading tags, networking, AI inference, haptic hardware and spatial rendering are host responsibilities.

## Current implementation status

Implemented: everything in the modules above plus typed track/source models, playback state, command/event contract, queue operations (move, clear, unique ids, next-track lookup for gapless preloading), repeat/shuffle, native backend contract, asynchronous event runtime, audio-focus handling with ducking, failure recovery, versioned snapshots with safe paused restoration, and unit tests.

Not in the core (platform work): Media3/AVFoundation adapters, background services, Android Auto, CarPlay, the DSP that actually applies the equalizer, network streaming, downloads, file scanning and tag reading, real-time WebRTC for Shared Rooms, generative AI backends, and plugin loading.
