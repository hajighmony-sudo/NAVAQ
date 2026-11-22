# NAVAQ

NAVAQ is a music application project with a platform-independent Rust playback/domain core and a native Android host. The Android app includes Media3 playback, AVA voice controls, and the Discover experience described below. 
## Workspace

- `core/` — playback state, command/event contracts, queue management, audio-session policy, snapshot recovery, equalizer/crossfade settings, library, playlists, lyrics (LRC), sleep timer, play history, **and exclusive features** (Shared Listening Rooms, Music DNA matching, creative tools, advanced haptics, personalized spatial audio).
- `android/` — native Android app using AndroidX Media3, MediaSessionService, a JNI bridge to Rust, AVA voice controls, and NAVAQ Discover and Downloader (public audio search, in-app browser, direct audio/video downloads, Wi-Fi/mobile-data policy, and offline download list).
- The iOS AVFoundation adapter is not yet implemented.

## Core boundary

`PlayerCore` owns deterministic playback state and queue decisions. It does not decode audio or access speakers. A native backend is responsible for preparing and rendering audio, managing OS audio sessions, and reporting callbacks such as prepared, position, buffering, and errors.

`AudioBackend` is the adapter contract. Native implementations execute ordered backend actions and return asynchronous callbacks through non-blocking event polling. The runtime connects that contract to the player core.

## Playback semantics

- Resume/Play continue from the current position and only re-prepare when the native item was released (Stop, completion, failure) or never loaded.
- Previous restarts the track after 3 s of playback; at the start of the queue it restarts the first track (or wraps with repeat-all).
- Late `Position` callbacks after Stop/completion and `Ended` callbacks that race with Pause are ignored.
- Track ids must be unique within a queue; track metadata and source identifiers are validated at queue insertion and snapshot restoration. The host should mint distinct ids if the same file appears twice.

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

The Android playback service publishes normalized title, display title, artist, album, and optional artwork URI through Media3 `MediaSession` metadata. Playback state and supported transport controls are exposed by the session and its automatically maintained MediaStyle notification. This is the public Android interoperability path used by compatible OEM media surfaces; Xiaomi HyperOS Super Island and Samsung One UI Now Bar decide whether and how to present a third-party session based on device model, OS version, settings, and OEM policy. NAVAQ does not use private OEM APIs or claim that an island can be forcibly displayed. Actual HyperOS/One UI presentation and artwork loading still require testing on physical supported devices. The Android service currently bridges player commands, backend preparation/buffering/position/end/error callbacks, and supported Media3 actions. `NavaqHaptics` provides an opt-in, rate-limited Android vibrator adapter for classified bass/snare/hi-hat/vocal/beat events; it is intentionally disabled by default and awaits event wiring from the audio-analysis pipeline. The platform-independent core now includes stateful PCM equalizer/gain processing and equal-power crossfade mixing primitives in `core/src/dsp.rs`. It also exposes `PcmAudioPipeline` in `core/src/audio_pipeline.rs`, which composes equalizer, ReplayGain/master gain, optional stateful echo, peak limiting, non-finite input sanitization before stateful filters, and PCM meter statistics (including sanitized/limited sample counts) for native decoders. Hosts must feed decoded PCM buffers through this pipeline; it is not automatically inserted into Media3's decoder/output path. These algorithms are ready for a native PCM pipeline, but the Android Media3 service does not yet insert them into its decoding/output path. Discover and AVA now provide native Android user-facing screens. Discover handles its own direct-download storage in app-specific Music/NAVAQ storage; broader MediaStore scanning/import and user-managed library synchronization remain separate host-integration tasks.

## NAVAQ Discover (Android)

Discover is a native Android feature reachable from the AVA home screen. It is separate from the Media3 playback service and does not replace or modify the existing playback engine.

### What is included

- **Public audio search:** searches Wikimedia Commons for audio files, displays the title, MIME type and available file size, and supports loading subsequent result pages.
- **In-app browser:** an Android WebView with DuckDuckGo as the default search engine, address/search input, back/forward/reload controls, and HTTP(S)-only top-level navigation.
- **Public audio/video downloads:** searches Wikimedia Commons for public media and downloads supported direct HTTPS media files from its media host only. Supported common formats include MP4, WebM, MKV, MOV, 3GP, AVI, MP3, M4A/AAC, FLAC, OGG, WAV and Opus.
- **Network controls:** choose Wi-Fi only or Wi-Fi plus mobile data for newly queued downloads; roaming is disabled.
- **Download queue and offline list:** stores DownloadManager IDs in app preferences, shows task states and percentage progress, allows active tasks to be cancelled and finished entries to be removed, and opens completed audio/video files with an Android media app.
- **Rust URL validation:** the Android bridge calls `nativeValidateDownloadUrl`, which delegates URL policy checks to `core/src/downloads.rs`. The Android host also checks the initial DNS result for local/private addresses before enqueueing.

### Build and entry point

Open NAVAQ, then choose **Open NAVAQ Discover** from the AVA home screen. To build the Android app locally:

```sh
android/scripts/build-rust-android.sh
gradle :android:assembleDebug
```

### Important boundaries

- Structured audio search currently uses Wikimedia Commons; the Browser tab is general web browsing/search, not a universal multi-site audio index.
- Downloads are restricted to public Wikimedia Commons media files hosted on `upload.wikimedia.org`; media URLs from other hosts are rejected. The Browser tab remains for ordinary web browsing and does not extract media from arbitrary pages.
- Check the license and attribution terms for each Commons file before reuse. DRM, protected streams, authentication, paywalls, age gates and site restrictions are not bypassed.
- Android DownloadManager owns transfer execution, retry policy, and notifications. Discover reports its state and can cancel tasks; it does not yet provide a custom resumable/range-download engine or in-app pause/resume controls.
- The initial hostname is checked before enqueueing, but DownloadManager follows redirects itself. This client-side check is not a complete server-side SSRF boundary.
- Files are saved under app-specific external Music/NAVAQ storage. They are available offline while retained by Android, but may be removed when the app is uninstalled. Search and browsing require internet access.
- The offline list is scoped to downloads started by Discover; it is not a full device-wide MediaStore music scanner.

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
- `audio_analysis` — streaming PCM energy-onset detector with partial-frame buffering, adaptive noise floor, sensitivity threshold, refractory window and timestamped onset events. It detects generic transients; it does not claim kick/snare/vocal classification.
- `dsp` — stateful multi-band peaking-EQ and gain processing for interleaved f32 PCM, plus equal-power crossfade mixing; native backends own PCM pipeline integration.
- `m3u` — parse and write M3U/M3U8 with `#EXTINF`.
- `guess_from_filename` — track number, artist and title from file names when tags are missing.
- `BookmarkStore` — resume positions for long tracks (ignores the first 5 s and the last 10 s).
- `UserRatings` — favorites and 1-5 ratings, resolvable to tracks for `SetQueue`.
- `text` — `normalize_for_search` unifies Persian/Arabic letter variants, digits, diacritics and ZWNJ; `fuzzy_score` is an fzf-style subsequence matcher. `Library::search` uses the normalization, `Library::search_fuzzy` ranks fuzzy matches, `Library::find_duplicates` groups likely duplicate tracks.
- `cue` — `parse_cue` reads CUE sheets (FLAC+CUE albums) into track start/end times.
- `ScrobbleTracker` — the 30 s / half-track / 4 min listening rule, built into `PlayerCore`: it emits `PlayerEvent::ScrobbleReady(id)` once per qualifying play; seeks and A-B loop jumps are not counted.

### Exclusive / differentiating modules

- **`social`** — Shared Listening Rooms state model with revision-checked commands, participant membership, host-only shared-track/global-mix changes, shared transport updates, latency-aware drift correction (gentle playback-rate adjustment or hard seek), private effects, and participant-contributed room-wide effects (Echo, Reverb, PitchShift, VocalOverlay, Custom). `RoomStore` is in-memory; WebSocket/WebRTC transport, clock-drift correction, presence, moderation and real-time audio mixing still belong to a server/native host.
- **`music_dna`** — Taste profiles built from play history + ratings + optional genre/tempo/energy tags. Similarity scores normalize vectors and validate inputs. `rank_opted_in_matches` excludes profiles that have not explicitly enabled discovery and supports a minimum history threshold. `rank_collaborative_tracks` combines per-participant track-affinity scores into a consensus ranking and penalizes missing coverage; account privacy, consent UI, candidate score generation and collaborative playlist persistence remain host responsibilities.
- **`creative` + `composer`** — Lyric-to-Melody and instrumental/stem-separation job contracts plus a manual multi-track composition model (tempo, meter, key, bars, instruments, notes, velocity, volume and pan). Manual arrangements are validated, can be queued for rendering, and can be exported as a Standard MIDI File. Actual synthesis, AI melody generation and stem separation require a host renderer/model and are not bundled into this Rust crate.
- **`haptic`** — Haptic patterns for BassHit, Snare, HiHat, VocalOnset and Beat, with intensity scaling and trigger gating. A host still needs beat/onset analysis and platform vibrator/controller adapters.
- **`spatial`** — Personalized room dimensions, surface absorption, validated speaker layouts, listener position, HRTF profile and a Sabine-style RT60 estimate. A real HRTF convolution/binaural renderer and device/head tracking are not included.

#### Behaviour layers on top of these modules

- **`ui_settings`** — user-customisable theme/interface schema (`UiSettings`: theme mode, accent, wallpaper, glass blur/opacity, corner radius, font scale, density, cover shape, list/nav style, effects, tabs) with presets, validation and `sanitized()` for older or hand-edited files. The interactive glass UI prototype in `design/ui-simulator/index.html` (open in a browser) uses the same settings.
- **`room_control`** — simultaneous control of a `SharedRoom`. `apply_control(from, ControlCommand, now_ms)` (alongside the revision-checked `apply_command`) accepts Play/Pause/Seek/SetTrack/Enqueue/SkipToNext and room-wide effects from anyone allowed by `ControlMode` (`HostOnly`, `Everyone`, `Delegated` + grants); personal effects (echo, vocal overlay...) need no control right. The host clock decides ordering, every accepted command bumps `revision`, `current_position_ms(now)` gives the room clock, `correct_drift` tells a client to do nothing, nudge speed by ±4 % or seek, and `advance_if_finished` moves through the shared queue.
- **`matching`** — `suggest_friends` (closest taste, with shared artists/genres as the reason, skipping friends), `build_shared_playlist` (average affinity of everyone, duplicates merged, fair share per person, no same artist back to back).
- **`haptic_analyzer`** — `HapticAnalyzer` detects bass/kick, snare-like and hi-hat transients in streaming PCM (three bands, adaptive threshold, refractory time); `schedule_pulses` turns the events into timed pulses using `HapticSettings` and master intensity.
- **`spatial_render`** — `render_spatial(&SpatialSettings)` computes per-speaker distance, azimuth, delay, time-alignment delay, level trim, interaural time difference, first-order wall reflections and the room's RT60 (Sabine) with furniture absorption (`FurnitureItem`: sofa, bed, curtains, carpet, ...). The host's spatializer applies the numbers.
- **`composition`** — manual instrumentals. Write notes per `Part` (sine/square/saw/triangle/pluck/pad and kick/snare/hi-hat), use `DrumPattern::from_rows` (`"x...X..."` step sequencer) and `chord_pitches`, then `render_pcm`, `to_wav_bytes` or `to_midi_bytes`.
- **`melody`** — offline lyric-to-melody: `generate_melody(lyrics, &MelodyOptions)` or `melody_from_request(&LyricToMelodyRequest)` builds melody, pad chords, bass and drums as a `Composition` (styles: pop, sad, rock, lofi, jazz, persian, cinematic; key and tempo hints; Persian lyrics supported; deterministic per lyrics and seed). Render it with `composition` and play the file like any track.

Player modes: `SetConsume` (drop tracks once played), `SetSingle` (stop after the current track; repeat-one still repeats), `SetLoopRegion` (A-B loop, cleared when the track changes), and `Reshuffle { seed }` (new random upcoming order; the current and played tracks stay put). The consume/single semantics follow MPD's playback options; the rest follows common practice in players such as MPD, Auxio and rodio-based apps. These projects were used as design references; no code was copied.

These modules do no I/O. Scanning files, reading tags, networking, AI inference, haptic hardware and spatial rendering are host responsibilities.

## Current implementation status

Implemented: everything in the modules above plus typed track/source models, playback state, command/event contract, queue operations (move, clear, unique ids, next-track lookup for gapless preloading), repeat/shuffle, native backend contract, asynchronous event runtime, audio-focus handling with ducking, failure recovery, versioned snapshots with safe paused restoration, and unit tests.

Not in the core (platform work): real-time network transport and audio mixing for Shared Rooms; account/authentication, safety/moderation and consent UX; on-device beat/source separation and AI inference; synthesis/rendering of manual arrangements and generated melodies; audio-reactive beat/onset analysis and event wiring (the Android actuator exists, but is not yet connected to the analyzer); full binaural/HRTF rendering; MediaStore-wide scanning/import and tag reading; complete Android DSP integration; and the iOS AVFoundation adapter. Discover's direct HTTP(S) download queue and app-specific offline list are implemented in the Android host.


### Social, sensory and creative integration status

The core APIs are intentionally platform-neutral. The following boundaries are
important when planning an end-to-end release:

| Feature | In repository | Still required for a live product |
| --- | --- | --- |
| Shared Listening Rooms | Room state, revision-checked commands, participant effect stacks, host authority, drift correction | Authenticated room server, WebSocket/WebRTC transport, clock source, low-latency audio routing/mixing, reconnect and moderation flows |
| Music DNA Matching | Taste profile builder, similarity scoring, opt-in candidate ranking | Explicit discovery/privacy controls, backend candidate discovery, block/report controls, shared-playlist storage and invitations |
| Haptic feedback | Trigger patterns, intensity settings, Android vibrator adapter | Audio beat/onset detector, Rust-to-Android event dispatch, user settings UI and device QA |
| Personalized spatial audio | Room/speaker/listener models, validation and RT60 estimate | Room setup UI, calibrated measurements, HRTF dataset/renderer, head tracking where supported, native audio pipeline integration |
| Manual instrumental creation | Editable note/instrument arrangement and MIDI export | SoundFont/instrument renderer, preview transport, editing UI, export/share workflow |
| Lyric-to-Melody | Validated job request/result lifecycle and playback-ready result contract | Actual local/cloud generative model, melody/harmony arrangement, rendering, cancellation/progress adapter and consent/cost controls |
| Instrumental/stem creation | Stem request/result contracts | Actual source-separation model and audio renderer |

The `audio_analysis` module now supplies timestamped generic onset events suitable for a host-controlled beat-reactive path; mapping them to `HapticTrigger::Beat` is possible, but the Android service has not yet wired that dispatch. It does not infer instrument identity or vocal onsets. The Android haptic adapter accepts only classified events from a trusted host
component. It does not record microphone audio or perform background capture.
The app must expose a clear user toggle before dispatching events. For shared
rooms, participant-generated vocal/effect streams must be explicitly enabled
and mixed by the room host/server; the Rust room model alone is not a network
audio mixer.
