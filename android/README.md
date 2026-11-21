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


## Music Island and OEM media surfaces

NAVAQ publishes playback through one AndroidX Media3 `MediaSession`. This is
the supported integration point for system media controls and OEM media surfaces;
the app does not draw over the status bar or request the special "display over
other apps" permission.

- **Xiaomi HyperOS / HyperIsland:** HyperOS documents automatic island exposure
  for compliant media-style notifications. Media3's `MediaSessionService`
  publishes the media notification from the active session. Keep playback
  metadata populated and only publish it while actual user-started playback is
  active.
- **Samsung One UI / Now Bar:** the media player surface is controlled by the
  device's One UI version and the user's Now Bar / Media player settings. NAVAQ
  exposes the standard active media session and playback metadata; availability
  and presentation remain controlled by Samsung System UI.
- **Other Android devices:** the same session provides the standard lock-screen,
  notification-shade, headset, Bluetooth, and compatible cast/media controls.

The service now supplies title, artist, album, playable state, and optional
artwork when an artwork URI is present in the track payload under
`artwork_uri`, `artwork_url`, `cover_uri`, or `image_url`. Artwork schemes
are restricted to Android resource, content, local file, and HTTP(S) URIs.

### Device-side checks

1. Start playback from NAVAQ; an idle app must not publish a fake playing item.
2. On Xiaomi, confirm HyperOS is current and check that system media notifications
   are enabled for the app/device.
3. On Samsung, enable the Media player category in the Now Bar settings on
   supported One UI devices.
4. Verify pause/resume and track changes from the system surface; they must
   control the same MediaSession player.

There is no public, universal API that lets a third-party app force its own
custom UI into every OEM's island. Therefore NAVAQ uses the documented Android
media-session/notification route rather than brittle hidden APIs or overlay
permissions. Exact appearance depends on the OEM, OS release, device model,
and user settings.
