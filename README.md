# NAVAQ

NAVAQ is being developed as a cross-platform music playback core. This repository currently focuses on the platform-independent Rust domain layer; it contains no user interface.

## Workspace

- `core/` — playback state, command/event contracts, and queue management.
- Native Android and iOS playback adapters are separate integration targets and are not yet implemented in this initial foundation.

## Core boundary

`PlayerCore` owns deterministic playback state and queue decisions. It does not decode audio or access speakers. A native backend is responsible for preparing and rendering audio, managing OS audio sessions, and reporting callbacks such as prepared, position, buffering, and errors.

## Development

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Current implementation status

Implemented: typed track/source models, playback state, command/event contract, queue operations, repeat-one/repeat-all behavior, state validation, and unit tests.

Not yet implemented: native decoding/output, background services, Android Auto, CarPlay, DSP/equalizer, streaming, downloads, lyrics, library scanning, and plugin loading. These will be added behind the core API in later increments rather than represented as completed features.
