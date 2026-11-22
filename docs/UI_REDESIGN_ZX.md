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
