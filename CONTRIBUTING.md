# Contributing to Sonux

Thank you for your interest in Sonux.

Sonux is a small personal and learning project without a fixed development or
release schedule. Bug reports, documentation improvements, compatibility
findings, and focused fixes are welcome, but I cannot guarantee that every
issue or pull request will receive a response or be accepted.

Forks are also welcome if you want to develop the project in a different
direction.

## Before Opening an Issue

- Search existing issues to avoid duplicates.
- Test with the latest available Sonux version when practical.
- Include your Linux distribution, desktop environment, PipeWire version, and
  WirePlumber version.
- Describe the expected and actual behaviour.
- Include clear reproduction steps and relevant logs.
- Remove usernames, home-directory paths, device serial numbers, and other
  personal information from logs and screenshots.

Do not report suspected security vulnerabilities through a public issue.
Follow the private reporting instructions in [SECURITY.md](SECURITY.md).

## Suggested Contributions

Contributions are particularly useful in these areas:

- Fixes for reproducible bugs
- Security and dependency improvements
- Testing and compatibility fixes for additional Linux distributions
- PipeWire and WirePlumber compatibility
- Documentation corrections
- Accessibility and usability improvements
- Focused performance or reliability improvements

For substantial interface changes, new dependencies, or architectural changes,
consider opening an issue first to discuss the proposal.

## Development Setup

Install the system dependencies listed in the
[README](README.md#requirements-and-build-dependencies), then install the
JavaScript dependencies:

```bash
npm ci
```

Run the application in development mode:

```bash
npm run tauri dev
```

## Checks

Before submitting a pull request, run the relevant checks:
```bash
npm test
npm run build
cargo fmt --all -- --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```
If a check cannot be run on your system, explain that in the pull-request
description.

## Pull Requests

Please keep pull requests focused on one subject.

A pull request should:

- Explain what changed and why.
- Describe how the change was tested.
- Identify the Linux distribution and relevant hardware used for testing.
- Include screenshots for visible interface changes.
- Avoid unrelated formatting or refactoring.
- Avoid committing generated build files, caches, or local configuration.
- Update lockfiles when dependencies intentionally change.
- Preserve existing attribution, licensing, and third-party notices.
- Mention substantial AI-assisted or automated changes in the description.

New media, icons, audio samples, or other assets must have a clearly documented
source and licence that permits redistribution with Sonux. Do not submit assets
when their copyright or redistribution terms are uncertain.

## Licensing

Sonux is licensed under the
GNU General Public License v3.0 (LICENSE).

By submitting a contribution, you agree that your contribution may be
distributed under the same GPL-3.0-only licence. You must have the right to
submit the code, documentation, or assets included in your contribution.

Third-party material must retain any notices required by its original licence.
