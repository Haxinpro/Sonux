# Changelog

This file summarizes user-visible changes in each Sonux release.

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

[1.1.1]: https://github.com/Haxinpro/Sonux/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/Haxinpro/Sonux/compare/v1.0.1...v1.1.0
