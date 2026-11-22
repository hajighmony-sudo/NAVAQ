# NAVAQ Downloader

The Downloader is the dedicated media-download workflow in NAVAQ Settings. It is intentionally separate from playback, AVA, library, and other app features.

## Supported sources

- Public audio/video files and file pages hosted by Wikimedia Commons.
- HTTPS only.
- Allowed hosts are checked for every request and redirect: commons.wikimedia.org and upload.wikimedia.org.
- A Commons file page may expose a direct public media resource. Only candidate media URLs on upload.wikimedia.org are offered.
- Supported direct formats include MP3, M4A/AAC, MP4/M4V, WebM, MKV, MOV, AVI, 3GP, FLAC, OGG/OGA, WAV, Opus and MPEG-TS where the source actually serves that format.
- Public, unencrypted HLS VOD can be processed when the playlist and media segments use the supported source hosts. Encrypted HLS and live playlists are rejected.

This is not a universal site extractor. Other website domains are rejected. It does not retrieve hidden streams, bypass DRM, authentication, age restrictions, paywalls, or site access rules. Check the license and attribution conditions before reusing a Commons file.

## Queue and controls

- Up to three transfer workers run concurrently.
- Each job has queued, running, paused, completed, and failed states.
- Jobs can be paused, resumed, retried, cancelled, or removed from the queue.
- Direct downloads write to a temporary .part file and resume with HTTP Range when the server supports it.
- A resumed response must return HTTP 206 and a Content-Range start matching the saved byte count; otherwise the transfer is rejected rather than silently corrupting the file.
- Progress, speed, ETA, state, and error are shown in the Settings download queue.
- Progress and task metadata are periodically persisted. After process restart, unfinished tasks return as paused and can be resumed from the queue.
- Wi-Fi-only is checked before a task starts and during direct transfer. If the active network becomes metered, the transfer pauses and waits for the user to resume it on Wi-Fi.
- Completed files are stored in the app's NAVAQ media directory and are added to the local music library where the format is supported.
- Optional audio extraction copies a compatible audio track into M4A without re-encoding. If the container/codec cannot be copied, the original media file is retained.

## Discover integration

Discover separately provides Commons audio/video search, creator/license metadata, a browser, and a DownloadManager-backed offline list. That list has automatic progress refresh, retry for failed tasks, cancel/remove, and bulk removal of completed items. The Discover direct-download entry point is also restricted to upload.wikimedia.org.

## Reliability and security notes

- Rust URL validation is called through NavaqNativeBridge.
- Android additionally checks DNS results for local, private, loopback, and link-local addresses.
- Redirects are followed manually and revalidated against the source allowlist.
- TLS is required; HTTPS-to-HTTP downgrade is rejected.
- Credentials in URLs and non-allowlisted hosts are rejected.
- HLS segment URLs are checked individually before being fetched.
- Wi-Fi-only policy pauses direct transfers if Android reports a metered active network.
- Android background execution limits and battery management remain controlled by the OS. A foreground transfer service is used while work is active, but no app can guarantee uninterrupted background execution on every device.

## Known limitations

- No site-specific extractor for arbitrary video platforms.
- No DRM, protected-stream, login, age-gate, or paywall bypass.
- HLS is limited to supported, public, unencrypted VOD playlists; byte-range playlists, separate alternate audio renditions, live playlists, and encrypted streams are not supported.
- HLS progress is segment-based and a paused HLS job restarts its segment sequence rather than resuming within a segment.
- Parallel HTTP range segments, checksums, configurable speed limits, scheduling, and a user-selected storage folder are not yet implemented.
- On Android, background service start/stop rules and battery optimization can affect long-running transfers.
