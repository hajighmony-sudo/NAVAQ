# NAVAQ Discover & Downloader

Discover is available from the NAVAQ home screen.

## Features

- **Audio/video search:** searches public audio and video files in Wikimedia Commons and displays the direct media URL, MIME type, known file size, license label, and creator/artist metadata where supplied.
- **Embedded browser:** opens the wider web inside NAVAQ. Text entered in the address field is treated as a URL when it looks like one, otherwise it is searched with DuckDuckGo.
- **Audio and video downloads:** downloads public media files from Wikimedia Commons over HTTPS. Recognized file types include MP4/M4V, WebM, MKV, MOV, 3GP, AVI, MP3, M4A/AAC, FLAC, OGG, WAV and Opus. The source allowlist is enforced before a task is queued.
- **Network choice:** new downloads can use Wi-Fi plus mobile data or Wi-Fi/unmetered networks only. The selected policy is saved between app launches; changing it affects newly queued downloads, not tasks already submitted to Android DownloadManager.
- **Offline library:** tracks Discover downloads, automatically refreshes progress while the tab is open, shows queue totals and transferred bytes, provides retry for failed tasks, cancel/remove actions, bulk removal of completed tasks, and opens completed audio/video files with an Android media app. Files are stored under the app-specific external Music/NAVAQ directory.
- **Rust URL policy:** Android calls `NavaqNativeBridge.nativeValidateDownloadUrl`; the Rust core rejects unsupported schemes, malformed URLs, credentials in URLs, and local/private destinations. The Android host additionally resolves the initial hostname and rejects private/loopback/link-local addresses.

## Scope and limitations

- The structured search provider indexes public audio and video files on Wikimedia Commons. The Browser tab is general web browsing/search only; it does not turn arbitrary websites into download sources.
- Downloading is intentionally limited to HTTPS media files hosted on `upload.wikimedia.org`. Links from other hosts are rejected, even if a page exposes them through the WebView download callback.
- Check each file's license and attribution requirements before reusing it. Discover does not extract hidden streams, bypass DRM, authentication, paywalls, age gates, or site restrictions.
- Android DownloadManager owns transfer execution, automatic retry behavior, and system notifications. Discover can inspect progress, retry failed tasks by queueing them again, cancel/remove tasks, bulk-remove completed tasks, and list completed files. It does not provide an in-app pause/resume control or download-speed/ETA estimation.
- The Wi-Fi-only option relies on Android's metered-network classification. Roaming is disabled for these requests.
- Hostname validation is performed before enqueueing. DownloadManager follows server redirects itself, so this client-side validation is not a general-purpose server-side SSRF boundary.
- Offline downloads are stored in app-specific external storage and may be removed when NAVAQ is uninstalled.
- Search and browser require an internet connection. Previously downloaded files can be opened offline.

## Architecture

Rust owns the portable Discover/download contracts and URL policy. The Android activity owns HTTP provider requests, WebView rendering, DownloadManager integration, Android notifications, and the offline file list. Existing Media3 playback behavior is not replaced by Discover.
