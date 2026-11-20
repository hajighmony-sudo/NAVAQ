# NAVAQ

NAVAQ is being developed as a cross-platform music playback core. This repository currently focuses on the platform-independent Rust domain layer; it contains no user interface.

## Workspace

- `core/` — playback state, command/event contracts, queue management, audio-session policy, and playback snapshot recovery.
- Native Android and iOS playback adapters are separate integration targets and are not yet implemented.

## Core boundary

`PlayerCore` owns deterministic playback state and queue decisions. It does not decode audio or access speakers. A native backend is responsible for preparing and rendering audio, managing OS audio sessions, and reporting callbacks such as prepared, position, buffering, and errors.

`AudioBackend` is the adapter contract. Native implementations execute ordered backend actions and return asynchronous callbacks through non-blocking event polling. The runtime connects that contract to the player core.

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

Implemented: typed track/source models, playback state, command/event contract, queue operations, repeat-one/repeat-all, shuffle order preservation, validated state settings, native backend contract, asynchronous event runtime, audio-focus/interruption policy, versioned queue/player snapshots, safe paused restoration, and unit tests.

Not yet implemented: actual native Android Media3 and iOS AVFoundation adapters, background services, Android Auto, CarPlay, DSP/equalizer, streaming, downloads, lyrics, library scanning, and plugin loading. These require platform-specific projects and integration tests; they are not represented as completed features in this Rust-only core.
