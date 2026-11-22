# NAVAQ Discover & Downloader

Discover is available from the NAVAQ home screen.

## Features

- **Audio search:** searches public audio files in Wikimedia Commons and displays the direct media URL, MIME type, and known file size.
- **Embedded browser:** opens the wider web inside NAVAQ. Text entered in the address field is treated as a URL when it looks like one, otherwise it is searched with DuckDuckGo.
- **Audio and video downloads:** accepts direct HTTP(S) media-file links and downloadable resources explicitly exposed by a website's WebView download callback. Recognized file types include MP4/M4V, WebM, MKV, MOV, 3GP, AVI, MP3, M4A/AAC, FLAC, OGG, WAV and Opus.
- **Network choice:** new downloads can use Wi-Fi plus mobile data or Wi-Fi/unmetered networks only. The selected policy is saved between app launches; changing it affects newly queued downloads, not tasks already submitted to Android DownloadManager.
- **Offline library:** tracks Discover downloads, displays transfer state and percentage progress, provides cancel/remove actions, and opens completed audio/video files with an Android media app. Files are stored under the app-specific external Music/NAVAQ directory.
- **Rust URL policy:** Android calls `NavaqNativeBridge.nativeValidateDownloadUrl`; the Rust core rejects unsupported schemes, malformed URLs, credentials in URLs, and local/private destinations. The Android host additionally resolves the initial hostname and rejects private/loopback/link-local addresses.

## Scope and limitations

- The built-in structured search provider currently indexes public audio on Wikimedia Commons. The Browser tab is general web browsing/search, not a universal multi-site media search API.
- Video downloads work when a site provides a direct media-file URL or explicitly exposes a downloadable resource through Android's download callback. A normal webpage URL is not a video file. Discover does not extract hidden streams from arbitrary pages or provide site-specific extractors.
- Only media you own, have permission to save, or that is explicitly offered for download should be downloaded. Discover does not bypass DRM, authentication, paywalls, age gates, or site restrictions.
- Android DownloadManager owns transfer execution, retry behavior, and system notifications. Discover can inspect progress, cancel/remove tasks, and list completed files; custom segmented transfers, in-app pause/resume, and download-speed/ETA estimation are not implemented yet.
- The Wi-Fi-only option relies on Android's metered-network classification. Roaming is disabled for these requests.
- Hostname validation is performed before enqueueing. DownloadManager follows server redirects itself, so this client-side validation is not a general-purpose server-side SSRF boundary.
- Offline downloads are stored in app-specific external storage and may be removed when NAVAQ is uninstalled.
- Search and browser require an internet connection. Previously downloaded files can be opened offline.

## Architecture

Rust owns the portable Discover/download contracts and URL policy. The Android activity owns HTTP provider requests, WebView rendering, DownloadManager integration, Android notifications, and the offline file list. Existing Media3 playback behavior is not replaced by Discover.
