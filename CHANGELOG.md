# Changelog

This file summarizes user-visible changes in each Sonux release.

## [1.2.1] - 28/09/2026

Sonux 1.2.1 is a maintenance update with no intended audio behavior changes.

### Maintenance

- Updated React and React DOM together with their TypeScript definitions to
  version 19, avoiding the incompatible split dependency updates.
- Updated the Tauri frontend API, dialog plugin and CLI, Material Symbols,
  Vite, Zustand, jsdom, and Vitest to reviewed current releases.
- Replaced end-of-life Node.js 20 build support with the shared Node.js
  24.21.0 LTS baseline used across the projects.
- Added an early installer version check with actionable guidance when Node.js
  is present but does not match the required project version.

## [1.2.0] - 31/08/2026

Sonux 1.2.0 improves first-run guidance, application identity, localization,
and audio startup behavior.

### Interface and onboarding

- Redesigned and polished the mixer, application, profile, processing, and
  settings interfaces with more consistent spacing, controls, explanations,
  focus behavior, and responsive layouts.
- Replaced the abstract onboarding diagram with a five-step first-run tour
  containing faithful miniature previews of the mixer, app routing, profiles,
  and microphone processing.
- Added a final onboarding choice between the ready-made Game, Chat, Media,
  and Aux board and a minimal custom starting board. The tour can be replayed
  from Settings without changing the current setup.
- Improved startup synchronization so transient frontend/backend timing does
  not leave the interface permanently disconnected after Sonux is available.

### Applications and languages

- Resolved related PipeWire streams to a canonical desktop application.
  Routing, hiding, drag-and-drop, and inactive-history operations now act on
  the complete group while preserving strict backend validation.
- Improved desktop-file and Steam identity resolution, including helpers that
  share launchers or reveal additional stream identities later.
- Added optional JSON language packs under
  `$XDG_CONFIG_HOME/sonux/locales`, with system-language selection, safe
  validation, CLDR plural forms, right-to-left support, English fallback, and
  an included custom-pack template.

### Audio routing and reliability

- Added an acknowledged WirePlumber 0.5 pre-link policy. Remembered streams
  now select their Sonux channel before their first playback link instead of
  briefly reaching the physical output at full stream volume.
- Confirmed the fix live with Brave returning after its PipeWire stream had
  disappeared: playback began at the configured Media volume without the
  previous onset spike.
- Preserved a safe fallback when Sonux channels are unavailable, excluded
  ignored applications from pre-link routing, and kept assignment, profile,
  channel-removal, and factory-reset metadata transactions coherent.
- Smoothed native processing after idle boundaries by discarding only stale
  buffered samples, resetting DSP state, and applying a short stereo-aligned
  resume fade.
- Fixed application restart so the old graph and route metadata are torn down
  before the replacement initializes.

### Upgrade notes

- The WirePlumber hook is installed automatically on first launch. Until the
  next login or WirePlumber service restart, Sonux safely uses its existing
  live-router fallback. No manual routing-file edits are required.
- Existing settings, assignments, profiles, and backups remain compatible.
- PipeWire with PulseAudio compatibility and WirePlumber 0.5 or newer remain
  required.

## [1.1.1] - 29/08/2026

Sonux 1.1.1 is a maintenance update with no intended audio or interface
behavior changes.

### Maintenance

- Restored clean builds under Rust 1.98 by adopting the equivalent slice
  chunking API available since the project's Rust 1.88 minimum.
- Updated Material Symbols, Zustand, Vite's React plugin, Vite, and Vitest to
  their reviewed minor or patch releases.

## [1.1.0] - 2026-08-13

Sonux 1.1.0 expands profile, microphone, metering, and recovery support while
strengthening the native PipeWire audio path introduced in earlier releases.

### Highlights

- Manage complete audio profiles: create fresh setups, copy, rename, delete,
  search, and activate profiles from the Profiles screen. Sonux retains an
  automatically protected fallback profile.
- Switch profiles automatically when a linked application starts or an
  assigned output device appears.
- Create and restore configuration backups. Sonux creates a recovery backup
  before replacing the active configuration.
- Optionally publish up to four independently processed microphone channels,
  each with its own input, gain, EQ, gate, compressor, limiter, mute, and
  profile state. This requires the native PipeWire backend.
- View live dBFS meters with peak and clip indicators, and choose the refresh
  rate or turn meter animation off. This requires the native PipeWire backend.
- Create stereo or spatial 7.1 custom channels and configure default devices,
  output failover, and Sonux device-label styles.
- Use improved keyboard controls, focus handling, accessible labels, dialogs,
  and explanatory tooltips throughout the mixer.

### Reliability and compatibility

- Hardened audio graph updates and recovery when PipeWire nodes, ports, links,
  or input devices disappear and return.
- Prevented unsafe microphone self-routing and made follow-default microphones
  choose only valid physical inputs.
- Removed unbounded real-time parameter waits and protected capture processing
  from invalid buffer offsets and concurrent ring-buffer overwrites.
- Made profile and configuration writes atomic, strictly validated, and
  serialized with backup, restore, and factory-reset operations.
- Preserved malformed or older profile data for recovery and added migration
  for legacy secondary microphone names.
- Made profile switching refresh the interface from one coherent backend
  snapshot, preventing mixed state during overlapping manual and automatic
  switches.
- Added source-controlled PipeWire binding compatibility patches for Clang 22
  while retaining the required upstream MIT notices.

### Upgrade notes

- Existing settings and profiles are migrated automatically. Creating a manual
  backup in Settings before upgrading is still recommended.
- Multiple microphone channels and live meters require the native PipeWire
  backend. The `pactl` fallback remains available with reduced functionality.
- PipeWire with PulseAudio compatibility and WirePlumber 0.5 or newer remain
  required.
- Runtime testing is confirmed on CachyOS and Arch Linux. Other distributions
  and package formats listed in the README are intended targets, not verified
  compatibility.

[1.2.1]: https://github.com/Haxinpro/Sonux/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/Haxinpro/Sonux/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/Haxinpro/Sonux/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/Haxinpro/Sonux/compare/v1.0.1...v1.1.0
