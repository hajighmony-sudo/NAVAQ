# NAVAQ Core

The `navaq-core` crate is the platform-independent domain layer. It owns deterministic state, validation, algorithms, and serializable command/event contracts. It must not depend on Android, Media3, UIKit, network clients, filesystems, or UI frameworks.

## Public entry points

- `PlayerCore`: deterministic playback state machine, queue decisions, transport commands, and backend actions.
- `PlayerRuntime<B>`: coordinates `PlayerCore` with an `AudioBackend`, handles asynchronous backend events, audio focus, sleep timers, and persistence restoration.
- `AudioBackend`: the adapter contract implemented by a native host. Execute actions in order and return callbacks through non-blocking polling.
- `PlayerSnapshot`: versioned logical playback state. Restore always returns a paused state; the host decides when to resume after rebuilding its audio session.
- `AvaCore`: local Persian/English transcript-to-intent interpretation. Speech recognition, permissions, wake-word capture, and music lookup remain host responsibilities.
- `UiSettings`: shared, serializable visual preference schema. Hosts render the settings and should validate or sanitize values before applying them.

## Capability map

| Area | Core modules | Host boundary |
|---|---|---|
| Playback, queue, history, playlists | `engine`, `runtime`, `model`, `queue`, `history`, `playlist`, `bookmarks`, `ratings`, `persistence` | Native decoder, storage, and UI |
| Audio processing | `dsp`, `audio_pipeline`, `audio_analysis`, `equalizer`, `replaygain`, `spatial`, `spatial_render` | Feed decoded PCM into the pipeline and route output to the device |
| Playback experience | `audio_session`, `sleep_timer`, `cue`, `lyrics`, `metadata`, `m3u`, `text` | OS audio focus, file access, metadata providers, and presentation |
| Discovery and transfers | `discover`, `discover_catalog`, `discover_session`, `downloads`, `download_queue` | Provider HTTP clients, download transport, permissions, and file persistence |
| AVA | `ava` | Speech-to-text, speech output, permissions, music catalog and command execution |
| Creative and social | `composer`, `composition`, `creative`, `melody`, `music_dna`, `matching`, `social`, `room_control` | Rendering/export, opt-in identity, network transport, and room coordination |
| Haptics | `haptic`, `haptic_analyzer` | Device vibrator and real-time audio event wiring |
| UI preferences | `ui_settings` | Native/web rendering and durable settings storage |

## Integration rules

1. Keep domain decisions deterministic and independent of platform APIs.
2. Treat all host-provided tracks, URLs, snapshots, settings, and remote metadata as untrusted input; validate at the boundary.
3. Do not perform network or filesystem work from the core. Return typed requests/results for the host to execute.
4. Execute backend actions in the order emitted. Backend callbacks must include track IDs so stale events can be ignored.
5. Persist snapshots as versioned data. Do not automatically resume audio during restoration.
6. Keep optional or privacy-sensitive features opt-in at the host boundary. In particular, social matching, room sharing, scrobbling, and haptics must not silently transmit or activate.
7. A core primitive is not considered end-to-end integrated merely because it compiles. The host must explicitly connect it to its decoder, storage, network service, or UI and test that path on-device.

## Validation

The workspace CI runs formatting, all unit and integration tests, and Clippy with warnings denied:

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

`core/tests/public_contracts.rs` exercises the public API across playback snapshot serialization/restoration, bilingual AVA intent interpretation, and UI settings serialization/sanitization. Module-level unit tests cover the detailed algorithms and state transitions.
