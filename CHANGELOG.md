# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Vietnamese version: [CHANGELOG.vi.md](CHANGELOG.vi.md).

## [Unreleased]

### Added

- **"What’s new" page.** The button at the bottom of the sidebar (replacing "Open log") shows the changes of every version in your language; when a newer version is waiting, its notes appear on top with an Update button.
- **Notice after updating.** The first launch after an update shows the main changes and where to read the rest.
- **"Logs" tab in Settings.** Open the log file or its folder, and pick another folder to store logs in (the old log is moved over); system folders are refused.

### Changed

- **Opening the log moved to Settings → Logs.**
- **Cleaner notes in the update bar.** "What’s new" shows only the part in your language.

## [0.3.0] - 2026-11-25

First release with an installer and auto update, in two languages.

### Added

- **Installer and auto update.** Installs into Program Files; the app checks for new versions (at launch and every 6 hours) and updates in one click, never without your consent.
- **Vietnamese and English.** Switch instantly in Settings → Appearance → Language, no restart; the AI assistant also answers in the chosen language.
- **New Settings page.** Grouped by category, with MCP setup guides for each AI client (Claude, Codex, Cursor, VS Code…) and detailed machine info.

### Fixed

- **Cleanup skipped Temp folders.** No more false "goes through a link" errors when the Temp path uses a short name (`RUNNER~1`).

[unreleased]: https://github.com/FomeG/FomegWindowsApp/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/FomeG/FomegWindowsApp/releases/tag/v0.3.0
