# NAVAQ Android platform layer

This module is the native Android playback host for the Rust workspace.

## Included

- AndroidX Media3 ExoPlayer as the device audio renderer.
- MediaSessionService for background playback.
- MediaSession transport controls surfaced to the lock screen, notification shade,
  Bluetooth/headset controls, and compatible Android media clients.
- Android audio-focus integration through ExoPlayer's audio attributes.
- Pause-on-headphones-disconnect behavior through Media3's noisy-audio handling.
- A Media3 PCM audio processor that observes decoded 16-bit or float PCM and
  detects generic energy onsets without changing the audio samples.
- Optional beat haptics dispatched from the audio-processing path to Android's
  vibrator on the main thread. Haptics are disabled by default; the preference is
  persisted locally and can be changed by the in-app service API or the explicit
  `ACTION_SET_BEAT_HAPTICS` service action with `EXTRA_ENABLED`.

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

The Media3 service is the first native platform layer. The Rust command/event bridge is present for playback-domain events. The beat
analyzer currently runs in the Android Media3 PCM path (rather than calling the
Rust analyzer through JNI), so the detector and haptic dispatch are connected
without copying PCM across JNI. The analyzer reports generic energy transients;
it does not classify kick, snare, hi-hat, or vocals. The equalizer/echo DSP chain
in Rust is still not inserted into Media3's output path.


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

The service supplies title, artist, album, and playable state. The Android
adapter can also read artwork URI keys (`artwork_uri`, `artwork_url`,
`cover_uri`, or `image_url`) if supplied by a future enriched track payload;
the current Rust `Track` model does not yet expose artwork metadata, so cover
art is not claimed as end-to-end complete.

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
