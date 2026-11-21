# AVA — NAVAQ voice assistant module

AVA is an additive Android/Rust module. It reuses the existing `NavaqPlaybackService`
and Rust `PlayerCore`; it does not create a second player.

## Current implementation

- Bilingual deterministic command parsing in Rust (Persian and English).
- Adaptive route decision for unknown requests; deterministic music commands stay local.
- Persistent settings shared by `AvaSettingsStore` and `AvaSettingsFragment`.
- Read-only device music discovery through Android MediaStore.
- An adapter contract for the existing NAVAQ in-app music catalog.
- Commands are submitted to the existing MediaSession playback service.
- Optional text-to-speech output; Visual is the default and Silent never speaks.
- Optional confirmation before replacing an existing playback queue.
- Explicit microphone and audio-library permission helpers.

## Embed the settings page

The Android module provides `AvaSettingsFragment`. Add it to the host app's
existing settings navigation; no existing settings screen needs to be replaced.
The fragment and `AvaSettingsStore` share the private preference file
`navaq_ava_preferences`.

Available settings:
- AVA enabled/disabled
- Music source: both, device, or in-app
- AI mode: adaptive, offline-only, or online-only
- Response: visual, silent, or voice
- Language: automatic, Persian, or English
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

A visible screen can start listening after permission is granted:

```kotlin
speechInput.startListening(
    languageTag = settings.speechLanguageTag(),
    preferOffline = settings.preferOfflineSpeech(networkAvailable)
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
- An online AI endpoint/provider is not configured in this repository. Adaptive
  routing selects the online route for complex requests, but does not pretend a
  cloud model was called. Add a provider with app-managed credentials before
  enabling complex online actions.
- Offline speech uses Android's on-device recognizer when the OS exposes one.
  If it is unavailable, AVA reports that limitation rather than silently
  switching to a network recognizer. A bundled Sherpa-ONNX Persian/English
  model is a later integration step.
- Wake-word activation is not exposed in this settings screen because no
  wake-word engine is integrated. Push-to-talk is the supported activation path.
- Android 11+ background microphone behavior is permission- and OS-policy-bound.
  AVA does not attempt hidden, permanent microphone capture.
- The service command client should be called from a visible user interaction,
  or while the playback service is already active, to respect Android background
  service-start restrictions.
