# NAVAQ

NAVAQ is being developed as a cross-platform music playback core. This repository currently focuses on the platform-independent Rust domain layer; it contains no user interface.

## Workspace

- `core/` — playback state, command/event contracts, queue management, audio-session policy, and playback snapshot recovery.
- Native Android and iOS playback adapters are separate integration targets and are not yet implemented.

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

## Development

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Current implementation status

Implemented: typed track/source models, unique track ids (duplicates are rejected), queue move/clear/remove with a stable current track, next-track lookup for gapless preloading, repeat-all wrap-around for Previous, playback state, command/event contract, queue operations (including explicit empty-queue clearing), repeat-one/repeat-all, shuffle order preservation, validated state settings, native backend contract, asynchronous event runtime, backend-failure recovery with safe re-prepare on retry, audio-focus/interruption policy wired into the runtime (pause/resume and ducking that never overwrites the user's volume), runtime events preserved across backend failures (`take_pending_events`), versioned queue/player snapshots, safe paused restoration, and unit tests.

Not yet implemented: actual native Android Media3 and iOS AVFoundation adapters, background services, Android Auto, CarPlay, DSP/equalizer, streaming, downloads, lyrics, library scanning, and plugin loading. These require platform-specific projects and integration tests; they are not represented as completed features in this Rust-only core.
