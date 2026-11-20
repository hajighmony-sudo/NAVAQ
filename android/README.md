# NAVAQ Android platform layer

This module is the native Android playback host for the Rust workspace.

## Included

- AndroidX Media3 ExoPlayer as the device audio renderer.
- MediaSessionService for background playback.
- MediaSession transport controls surfaced to the lock screen, notification shade,
  Bluetooth/headset controls, and compatible Android media clients.
- Android audio-focus integration through ExoPlayer's audio attributes.
- Pause-on-headphones-disconnect behavior through Media3's noisy-audio handling.

## Build

Open the repository root in Android Studio, or run:

```sh
gradle :android:assembleDebug
```

The Android SDK (API 36) and JDK 17 are required.

## Integration boundary

The Rust crate remains the owner of queue and playback-domain decisions. This
module owns Android-only responsibilities: decoding/output, MediaSession,
foreground-service lifetime, audio focus, and OS media controls.

The Media3 service is the first native platform layer. The command/event bridge
between Rust `PlayerRuntime` and this service is the next integration step;
Media3 is not presented as a completed JNI bridge yet.
