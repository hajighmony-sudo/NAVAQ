# NAVAQ Discover

Discover is available from the NAVAQ home screen.

## Features

- **Audio search:** searches public audio files in Wikimedia Commons and displays the direct media URL, MIME type, and known file size.
- **Embedded browser:** opens the wider web inside NAVAQ. Text entered in the address field is treated as a URL when it looks like one, otherwise it is searched with DuckDuckGo.
- **Direct downloads:** downloads user-selected HTTP(S) URLs through Android DownloadManager. Transfers continue under Android's download manager and display system download notifications.
- **Offline library:** tracks Discover downloads in the app and opens completed files using Android's audio intent resolver. Files are stored under the app-specific external Music/NAVAQ directory.
- **Rust URL policy:** Android calls `NavaqNativeBridge.nativeValidateDownloadUrl`; the Rust core rejects unsupported schemes, malformed URLs, credentials in URLs, and local/private destinations. The Android host additionally resolves the initial hostname and rejects private/loopback/link-local addresses.

## Scope and limitations

- The built-in structured audio search provider is Wikimedia Commons. The Browser tab is the general web browsing/search path; it is not a universal indexed search API.
- A website may not expose a direct downloadable media URL. In that case, Discover can browse the page but cannot extract protected streams or bypass DRM, authentication, paywalls, or site restrictions.
- Android DownloadManager owns transfer execution, retry behavior, and system notifications. Discover can inspect progress, cancel/remove tasks, and list completed files; it does not implement a custom byte-range transfer engine.
- Hostname validation is performed before enqueueing. DownloadManager follows server redirects itself, so this client-side validation is not a general-purpose server-side SSRF boundary.
- Offline downloads are stored in app-specific external storage and may be removed when NAVAQ is uninstalled.
- Search and browser require an internet connection. Previously downloaded files can be opened offline.

## Architecture

Rust owns the portable Discover/download contracts and URL policy. The Android activity owns HTTP provider requests, WebView rendering, DownloadManager integration, Android notifications, and the offline file list. Existing Media3 playback behavior is not replaced by Discover.
