# NAVAQ Android platform layer

Feature guide (Persian) with usage and how each feature works: see the [root README](../README.md). This file is the Android engineering reference.

This module is the native Android playback host for the Rust workspace.

## Included

- AndroidX Media3 ExoPlayer as the device audio renderer.
- MediaSessionService for background playback.
- MediaSession transport controls surfaced to the lock screen, notification shade,
  Bluetooth/headset controls, and compatible Android media clients.
- Android audio-focus integration through ExoPlayer's audio attributes.
- Pause-on-headphones-disconnect behavior through Media3's noisy-audio handling.
- A Media3 PCM audio processor that routes decoded 16-bit or float PCM through
  the Rust stateful DSP pipeline (gain, parametric EQ, optional echo and peak
  ceiling), then detects generic energy onsets from the processed signal.
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

The Media3 service is the first native platform layer. The Rust command/event bridge is present for playback-domain events. The Media3 processor owns a format-specific Rust PCM pipeline through JNI.
PCM is copied into a bounded per-buffer float array for processing, then written
back to Media3's output buffer; the pipeline instance persists across buffers so
filter and delay state remains continuous. Core `SetEqualizer` actions are
forwarded into that pipeline. The Kotlin onset detector reports generic energy
transients; with advanced haptics enabled the Rust `HapticAnalyzer` classifies
bass, snare and hi-hat bands instead. The room reverb (echo stage) is driven by
the spatial-audio settings (`ACTION_SET_ROOM`).


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


## AVA voice assistant

AVA is an optional, isolated Android voice-assistant adapter that uses the existing Rust `PlayerCore` and Media3 player. It does not create a second audio engine.

- `ui.NavaqHomeActivity` is the launcher: native glass UI (library, search, mini/full player, theme and interface settings) on top of the Media3 session. `AvaActivity` remains reachable from Settings > Playback.
- `AvaActivity` provides It provides one-time model installation, runtime microphone/audio permission handling, start/stop listening, transcript and command-result display, and explicit confirmation before replacing an active queue. `AvaVoiceController` remains reusable by a future host screen.
- `AvaSettingsFragment` can be embedded in the host settings navigation. It persists preferences in `navaq_ava_preferences`, shared with `AvaSettingsStore`.
- The default profile is both music sources, local command processing, automatic language, and visual-only responses. Silent mode suppresses both speech and result broadcasts; Voice mode is opt-in.
- Device audio is discovered read-only through MediaStore. The app must request `AvaPermissions.requiredAudioPermission()` at runtime before device-library access.
- The in-app library integration is an explicit adapter: register `AvaInAppCatalog.register(AvaInAppMusicProvider { query -> ... })` during host application startup. Convert existing NAVAQ library records into `AvaCatalogTrack` values with a playable `content_uri` or `local_file` source. The core repository currently has no Android application/library owner to register automatically.
- AVA uses Sherpa-ONNX with the multilingual Whisper tiny INT8 model for fully local Persian/English speech-to-text. The first installation downloads the model archive once (about 116 MB) from the Sherpa-ONNX release; after extraction, microphone audio is processed on-device and is never sent to a cloud recognizer. The model is stored in app-private storage. The app needs internet only for that one-time model installation and software updates, not for recognition. Automatic language detection is restricted to Persian and English.
- All recognized commands are interpreted locally by the Rust command parser. Unknown requests are not forwarded to a cloud AI.
- The offline Whisper recognizer remains loaded across successive foreground utterances, avoiding a full model reload for every command.
- Optional wake-phrase gating accepts “هی آوا”, “Hey Ava”, “Okay Ava”, or “AVA” only while the user-started foreground session is active; it is not a system-wide or background hotword service.
- Optional “هی آوا” / “Hey Ava” phrases are accepted as command prefixes only while the user-started foreground listening session is active. True always-on wake-word detection is not implemented; AVA stops listening when the user presses Stop or leaves the Activity. Android background microphone restrictions and battery policies make permanent listening a separate, explicit feature.

### AVA Android voice commands in this build

The Android action adapter is intentionally limited to the commands requested for this feature:

| Persian voice command | Android behavior |
|---|---|
| «به شماره … زنگ بزن» | Opens the system dialer with the recognized number filled in; the user taps Call |
| «وای‌فای را باز کن» | Opens Wi-Fi settings |
| «بلوتوث را باز کن» | Opens Bluetooth settings |
| «دوربین را باز کن» | Opens the camera app |
| «پیام‌ها را باز کن» | Opens the default messaging app |
| «آلارم را باز کن» | Opens the system alarms screen |
| «تنظیمات باتری را باز کن» | Opens battery settings |
| «تنظیمات اعلان‌ها را باز کن» | Opens NAVAQ's app notification settings |

Persian digits (۰–۹), Arabic/Persian letter variants, and zero-width non-joiners are normalized before local command matching. Matching and dispatch are on-device; no online AI service is required. The app does not toggle radios silently, send messages, create alarms, or place calls. Android/OEM availability can vary, and the user remains in control of the opened system screen.

## Android system integrations and smart notifications

NAVAQ keeps Android system actions behind visible, user-initiated intents:

- AVA can open the Android phone dialer, general/Wi-Fi/Bluetooth/sound/battery settings, messaging app, camera, clock/alarms, and NAVAQ's app-notification settings using Persian or English voice commands.
- AVA can prepare the Android dialer with a number spoken by the user (Persian and English digit forms are normalized). NAVAQ never places the call itself: the user reviews the number and taps Call in the system dialer. NAVAQ does not read contacts, access call history, or request `CALL_PHONE` / contacts permissions.
- Smart notifications are opt-in from the AVA screen. Android 13+ notification permission is requested only after the user enables them.
- Notification channels are separated into AVA/activity, downloads, and playback errors. Users can manage each channel in Android settings and can disable NAVAQ smart notifications inside the app.
- AVA results and playback errors can appear as notifications when enabled. Completed NAVAQ-owned downloads are verified against the app's private DownloadManager ID list before a notification is posted.
- Download progress remains managed by Android DownloadManager. Media playback controls remain managed by Media3's MediaSession notification; NAVAQ does not create a duplicate playback notification.
- Android OEM surfaces, notification presentation, lock-screen visibility, and notification permission remain under OS/device/user control.

These integrations deliberately avoid hidden APIs, overlay permissions, automatic phone calls, contacts scraping, and background microphone access.

## Native UI features (`ui` package)

- Glass library/player/settings UI with live theme customisation (`UiSettings` mirrors the Rust `ui_settings` module).
- 10-band equalizer (Settings > Sound) applied by the Rust DSP; preserved across audio reconfiguration.
- Personal spatial audio: room size, furniture and reverb are rendered by `nativeRenderSpatial`; the result drives a simplified room reverb tap in the Rust PCM pipeline (`nativeSetDspRoom`). It is not a binaural/HRTF renderer.
- Advanced haptics: the Rust `HapticAnalyzer` (`nativeCreateHaptic`/`nativeProcessHaptic`) replaces the generic onset detector and maps bass hits, snares and hi-hats to `NavaqHaptics` triggers.
- Song creation: lyrics (or wordless mode) to melody and harmony through `nativeGenerateSong`, saved as WAV + MIDI and added to the library.
- Shared listening works without a server, like casting a phone to a TV: the host creates a room (6-digit code, address shown, mDNS `_navaq._tcp`), guests on the same Wi-Fi enter the code and follow the host's track and position. Phones are kept in sync with a ping-based clock-offset estimate: small errors are absorbed by nudging speed (+-6%), larger ones by a seek, the next track is prefetched for gapless hand-off, and a per-device delay slider compensates Bluetooth latency. The host serves the audio over a small HTTP server with Range support; the token in each URL is issued only after the code is accepted. Cleartext HTTP is enabled for this LAN traffic. The room is kept alive in the background by the same foreground service.
- Music DNA is exchanged as a copy-paste code (artist names only) and compared on-device by the Rust core.
- NAVAQ Downloader (Settings > Downloads): paste a link to a media file, an HLS `.m3u8` stream (unencrypted VOD) or a web page that embeds audio/video; it probes the link (redirects validated against private addresses), downloads with pause/resume, optional audio-only extraction (MediaExtractor/MediaMuxer, no re-encoding) and Wi-Fi-only. No site-specific extractors, DRM or encrypted streams. Downloads and shared rooms are kept alive by a foreground `dataSync` service (`NavaqTransferService`) that stops itself when idle. Finished files join the library.
- Not wired: internet-wide rooms (would need a relay server); playback bypasses the Rust PlayerCore queue.
