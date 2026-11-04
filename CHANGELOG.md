# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versioning: [SemVer](https://semver.org/).
A release is created automatically when the `version` in `Cargo.toml` on `main` has no tag yet.

## [1.0.1] – 2026-11-04

### Fixed
- The installer reported "Win+J" even when an existing `WinPick.ini` configured a different hotkey. It now reads the settings and shows the hotkey actually in use.

### Added
- On start, WinPick shows a notification with the active hotkey (not when started automatically with Windows; the autostart entry now passes `--autostart`).

## [1.0.0] – 2026-11-04

### Added
- Global hotkey (default **Win+J**) opens an overlay on the monitor under the mouse cursor.
- Live previews of all open windows as a scrollable grid (number of columns configurable).
- Search box filters by window title, case-insensitive.
- **Enter** brings all matching windows to the front (first tile in front); with an empty search box only the first tile. **Click** brings a single window to the front, **Esc** closes.
- Settings: hotkey, columns, window size in %, margin in mm, background color, start with Windows.
- Tray icon with Open, Settings, About and Exit.
- Portable: a single EXE, settings in `WinPick.ini` next to the EXE.
- Installation via `irm … | iex`.

### Design
- WinPick never touches other processes' input state: no `AttachThreadInput`, no injected key presses. The overlay is hidden before windows are reordered, and windows that do not respond are skipped.
