# AVA — NAVAQ voice assistant module

AVA is an additive Android/Rust module. It reuses the existing `NavaqPlaybackService`
and Rust `PlayerCore`; it does not create a second player.

## Current implementation

- Bilingual deterministic command parsing in Rust (Persian and English).
- Fully local, deterministic Persian/English command interpretation; no cloud AI route or AI-provider credentials.
- Persistent settings shared by `AvaSettingsStore` and `AvaSettingsFragment`.
- Persian (`fa-IR`) is the first-run/default recognition language. English and automatic Persian/English detection remain selectable.
- Read-only device music discovery through Android MediaStore.
- An adapter contract for the existing NAVAQ in-app music catalog.
- Commands are submitted to the existing MediaSession playback service.
- Optional text-to-speech output; Visual is the default and Silent never speaks.
- Optional confirmation before replacing an existing playback queue.
- Explicit microphone and audio-library permission helpers.
- Sherpa-ONNX offline multilingual Whisper tiny INT8 speech recognition for Persian and English, with automatic language detection restricted to Persian/English.
- One-time, in-app speech-model installation with progress, free-space validation, archive-size validation, and app-private model storage.
- Interrupted model downloads retain a partial archive and resume on retry when the server provides an HTTP validator and supports byte-range requests; a changed or non-resumable response safely restarts the transfer.
- Continuous foreground voice sessions: after one explicit Start action, AVA keeps one microphone/Whisper recognizer session alive, detects successive utterances locally, and reuses the loaded model until Stop is pressed or the Activity leaves the foreground.
- Optional local wake-phrase gating for “هی آوا”, “Hey Ava”, “Okay Ava”, or “AVA” within the user-started foreground listening session; commands without the wake phrase are not executed in this mode.
- Optional Persian/English wake-phrase prefixes such as “هی آوا” and “Hey Ava” within an already-started listening session.
- A launcher Activity for AVA with microphone permission flow, start/stop listening, recognized transcript display, model setup, command-result display, and music-change confirmation.

## Embed the settings page

The Android module provides `AvaSettingsFragment`. Add it to the host app's
existing settings navigation; no existing settings screen needs to be replaced.
The fragment and `AvaSettingsStore` share the private preference file
`navaq_ava_preferences`.

Available settings:
- AVA enabled/disabled
- Music source: both, device, or in-app
- Response: visual, silent, or voice
- Language: Persian by default, English, or automatic Persian/English detection
- Confirm before replacing the current queue

## Connect the in-app music library

Register a provider once during app startup. Return the app's already-indexed
tracks as `AvaCatalogTrack` entries. A track source must use one of the Rust
source kinds supported by NAVAQ: `content_uri`, `local_file`, or `remote_url`.

```kotlin
AvaInAppCatalog.register { query ->
    // Map the existing NAVAQ library search results to AvaCatalogTrack.
    existingMusicLibrary.search(query).map { item ->
        AvaCatalogTrack(
            id = "app:${item.id}",
            title = item.title,
            artist = item.artist,
            album = item.album,
            durationMs = item.durationMs,
            sourceKind = "content_uri",
            sourceValue = item.contentUri.toString()
        )
    }
}
```

The example names above represent the host app's existing library API; adapt
those property names to the actual catalog implementation. AVA does not invent
or create a replacement music database.

## Microphone and media permissions

Request `AvaPermissions.microphonePermission()` at runtime before starting
`AvaSpeechInput`. Request `AvaPermissions.requiredAudioPermission()` only
when the user chooses Device or Both as the music source. Do not request either
permission silently.

Launch NAVAQ to the AVA screen. Install the speech model once while connected to the internet; after installation the model runs locally. The AVA screen requests microphone permission and, when the selected source includes device music, audio-library permission. A visible screen can start listening after permission is granted:

```kotlin
speechInput.startListening(
    languageTag = settings.speechLanguageTag(),
    preferOffline = true
)
```

Pass recognized transcripts to `AvaCommandClient.submitTranscript(context, text)`.
Register a package-scoped receiver for `NavaqPlaybackService.ACTION_AVA_RESULT`
to display the visual status. When the app receives
`ACTION_AVA_CONFIRMATION_REQUIRED`, present explicit Confirm/Cancel controls and
call `AvaCommandClient.confirmMusicChange(context, accepted)`.

## Important integration boundaries

- The current repository has no existing Activity/navigation/settings UI to
  attach the fragment to. The fragment is reusable, but the host app must add it
  to its settings navigation.
- The in-app provider must be registered by the real NAVAQ library owner.
- AVA does not use an online AI engine. Recognized text is interpreted by the
  local Rust command parser; unsupported requests receive a local explanation
  rather than being sent to a cloud model.
- Offline speech uses Sherpa-ONNX and the multilingual Whisper tiny INT8 model.
  The model is installed once from the AVA screen or settings page (approximately
  116 MB) and then runs from app-private storage without network access. The
  first-time download is required before the offline microphone recognizer can start.
- Automatic language detection is constrained to Persian and English; users can
  also pin either language in settings. Persian is selected on a fresh install.
  The recognizer captures one utterance at a time, preserves a short audio pre-roll,
  and stops on a silence interval or after its maximum capture window. The same
  Whisper model instance is reused for successive utterances instead of being
  loaded for every command.
- Optional wake-phrase mode recognizes “هی آوا”, “Hey Ava”, “Okay Ava”, or
  “AVA” locally, but only inside a user-started foreground listening session.
  It is not a low-power system-wide hotword service; AVA stops listening when
  the user presses Stop or leaves the Activity.
- Android 11+ background microphone behavior is permission- and OS-policy-bound.
  AVA does not attempt hidden, permanent microphone capture.
- The service command client should be called from a visible user interaction,
  or while the playback service is already active, to respect Android background
  service-start restrictions.

## Optional offline speech model

AVA's multilingual Whisper tiny INT8 speech model is an optional app-private download,
not an APK asset. Users can install it from AVA settings when they want offline Persian
and English speech recognition. The settings page reports the installed model size and
allows the model files to be removed to reclaim storage. Removing the optional model also clears any retained partial archive and its resume metadata, so interrupted downloads do not leave hidden cache files behind. Removing this model does not
remove AVA or change other NAVAQ features; offline speech recognition remains unavailable
until the model is downloaded again. Speech recognition stays on-device after installation.

## Language defaults and package size

Persian (`fa-IR`) is the default language for new installations and for accounts
without a saved language preference. Existing explicit language choices are preserved.
The currently used Whisper model is multilingual, so the same optional model download
provides both Persian and English; there is no separate Persian model package.
The speech model remains an optional download and is not preloaded into the APK. If a transfer is interrupted, AVA keeps the partial archive in app cache and resumes it on the next retry when the server's ETag/Last-Modified validator and HTTP Range response allow a safe continuation; otherwise it starts a clean transfer. The completed archive is only extracted after its expected total size is verified.
The Sherpa-ONNX native runtime is still packaged with the Android app at this stage,
so the optional model download reduces initial model storage but does not yet remove
the runtime libraries from the APK.
