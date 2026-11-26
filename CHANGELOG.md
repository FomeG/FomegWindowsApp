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

### Fixed

- **Update buttons ask first.** The Update buttons in "What’s new" and in Settings → About ask for confirmation, like the update bar.
- **Less background work.** The Ports page stops refreshing after you leave it or while updates are paused, and the RAM chart of startup and service items stops recording while paused.
- **Ending a process re-checks it right before acting.** On the Processes and Ports pages and in AI assistant proposals, a process that exited or whose PID was taken by another program after you confirmed is skipped and reported instead of being hit by mistake.
- **Uninstall no longer waits forever.** If an app's uninstaller could not be started, uninstalling reports the error instead of hanging.
- **Cleanup sizes match and drive C: updates at once.** An item's size, the inspector and the "selected" total agree, and the part that will be kept (files changed in the last 24 hours, files in use at the last clean) is shown instead of a confusing 0 B. After a clean, the free space of drive C: changes right away, with no rescan.
- **Disk updates at once after a delete.** Sizes and free space change immediately without rescanning the drive; a folder that was only partly deleted is re-measured on its own. Deleting from the Windows right-click menu updates the Disk page too, and cleanup keeps the numbers of the cleaned folders right.
- **Restart works again.** Restart on normal apps and Explorer works when the app runs as administrator; the old process is ended only once the new one is ready. A sandboxed process is refused instead of being relaunched with more privileges.

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
