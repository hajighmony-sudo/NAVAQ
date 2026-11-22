# NAVAQ UI refresh — ZX-inspired design pass

Target branch: `core-zx-ui-redesign` (based on `core`)

## Design intent

Keep NAVAQ's existing Android View toolkit and playback/service boundaries intact while bringing across the visual language of the ZX branch: deep midnight surfaces, cyan/violet/gold accents, restrained neon edge lighting, geometric glass panels, a floating HUD dock, and motion that responds to selection.

The ZX reference files reviewed were:
- `shared/src/commonMain/kotlin/dev/cluvex/zedsecure/ui/zx/ZxTheme.kt`
- `shared/src/commonMain/kotlin/dev/cluvex/zedsecure/ui/zx/ZxNavBar.kt`
- `shared/src/commonMain/kotlin/dev/cluvex/zedsecure/ui/zx/ZxHome.kt`
- `shared/src/commonMain/kotlin/dev/cluvex/zedsecure/ui/vault/VaultScreen.kt`

## Implemented in this pass

- Added a three-tone neon palette to NAVAQ's `GlassTheme`; the user's selected accent remains the primary accent.
- Reworked glass edge rendering into a subtle accent/cyan/violet/gold gradient with a restrained inner highlight.
- Replaced abrupt press feedback with short alpha/scale transitions and enabled keyboard focus on pressable views.
- Refreshed the floating/docked navigation into a more HUD-like selected tile, with a gradient selection marker and entrance motion.
- Added a Bento-style library overview card with live counts for tracks, favourites and playlists.
- Kept Media3 playback, Rust bridge, downloader, LAN room, AVA, persistence and settings data paths untouched.

## Reference projects reviewed

- Android official Compose samples: https://github.com/android/compose-samples
  - Used as reference for adaptive UI, theme patterns, custom design systems and UI testing.
- Appyx: https://github.com/bumble-tech/appyx
  - Reviewed for gesture-aware navigation and transition patterns; not added as a dependency because NAVAQ currently uses Android Views.
- Why Not Compose: https://github.com/ImaginativeShohag/Why-Not-Compose
  - Reviewed for Compose motion and component ideas; patterns are being adapted rather than copied.
- Auxio: https://github.com/OxygenCobalt/Auxio
  - Reference for music-library and playback UX.
- Datmusic Android: https://github.com/alashow/datmusic-android
  - Reference for music search/download/player information architecture.

## Guardrails

- No ZX Compose source is copied into NAVAQ.
- No new third-party runtime dependency is introduced by this UI pass.
- Existing playback and native interfaces are not reimplemented.
- Download source restrictions and existing downloader behavior are unchanged.
- User-selected theme, accent, density, blur, opacity and animation settings remain respected.

## Validation

GitHub Actions Android CI is configured to build the universal debug APK, verify native ABI payloads and build the release variant. The branch push triggered the workflow; final build status must be checked from the run linked in the pull request before merging.


## Follow-up implementation pass

- Added low-amplitude fade/slide transitions when switching between the library, search and settings screens; transitions respect the existing animation preference.
- Improved library filter chips with selected-state gradients, a visible outline and accessibility labels.
- Rebuilt download queue entries as glass status cards with distinct complete/failed/paused/active states, readable progress tracks and clearer touch affordance. Download scheduling and network policy are unchanged.
- Brought the AVA console cards and controls into the same midnight-glass/cyan-violet visual language, with tactile button feedback. Voice recognition, permission handling, notification actions and command routing are unchanged.
- Applied the same glass-card and button treatment to Discover. Search, browser, offline library, and the existing Wikimedia Commons-only download safeguards remain unchanged.
- Refined shared settings icon chips and option chips so all settings subsections inherit the same selected-state treatment.
- Added a settings Control Center with live theme, equalizer, shared-room and active-download summaries plus direct shortcuts into the existing sections.
- Improved accessibility descriptions for settings switches, chips and color swatches.
- Matched AVA's scroll surface to the new midnight background.
- Elevated the full player with a multi-tone ambient stage, a gradient primary playback control and clearer active-state accents for queue and sleep timer actions.

## Validation status

- Source changes are committed to `core-zx-ui-redesign` and included in PR #2.
- The earlier Android CI attempt was cancelled before APK assembly; it is not a passing build.
- Changed Android CI concurrency to queue rather than cancel active builds when several commits are pushed in quick succession.
- A fresh Android CI run is in progress/queued for the updated branch; do not merge or distribute an APK until the latest run completes successfully.
