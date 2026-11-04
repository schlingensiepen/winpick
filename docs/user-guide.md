# WinPick – User guide

## Installation

**Option A – one-liner (recommended).** Open PowerShell and enter:

```powershell
irm https://raw.githubusercontent.com/schlingensiepen/winpick/main/deploy/install.ps1 | iex
```

The script
1. downloads `WinPick.exe` from the latest release,
2. verifies the SHA-256 checksum,
3. places the file in `%LOCALAPPDATA%\Programs\WinPick`,
4. shows the hotkey currently configured and
5. asks *Start WinPick now?* (Enter or Y = yes, N = no). If yes, WinPick starts with the settings dialog open, so you can check or change the hotkey right away.

No admin rights are required. Running the command again updates to the latest version.
To install a specific version, set `$env:WINPICK_VERSION = 'v1.0.0'` before running it.

**Option B – manual.** Download `WinPick.exe` from the [releases page](https://github.com/schlingensiepen/winpick/releases/latest), put it in any folder (a USB stick works too) and run it.
On first start Windows SmartScreen may warn you because the file is not signed: *More info → Run anyway*.

## Usage

After starting, WinPick runs in the background and shows an icon in the notification area of the taskbar.

| Action | Effect |
|---|---|
| **Win+J** (configurable) | Open or close the overlay |
| Typing | Filters the windows: only windows whose title contains the text remain (case-insensitive) |
| **Enter** | Brings **all** shown windows to the front. The first tile ends up in front, the others directly behind it in the order shown. Minimized windows are restored. With an empty search box, Enter brings only the first tile to the front (like Alt+Tab). |
| Click on a tile | Brings only this window to the front |
| **Esc** or click outside | Closes the overlay |
| Mouse wheel, ↑/↓, Page Up/Page Down | Scroll the grid |
| Ctrl+A / Ctrl+Backspace | Select the search text / delete the previous word |

The overlay appears on the screen where the mouse cursor is. Windows are sorted by last use (most recently active first). The right end of the search box shows how many windows are shown out of how many in total.

Starting the EXE again while WinPick is already running simply opens the overlay. `WinPick.exe --settings` opens the settings dialog instead (both for a new and for an already running instance).

## Tray icon

- **Left click:** open the overlay
- **Right click:** menu with *Open overview*, *Settings…*, *About* (version, path of the settings file) and *Exit*

## Settings

Open them with the **Settings** button in the overlay or from the tray icon.

| Setting | Meaning | Default |
|---|---|---|
| Hotkey | Modifiers (Ctrl, Alt, Shift, Win) plus one key | Win+J |
| Grid columns | Number of tiles per row (1–10) | 4 |
| Window size | Size of the overlay in % of the work area (screen without taskbar) | 100 % |
| Additional margin | Distance to the screen edge in millimeters | 20 mm |
| Background color | Base color of the overlay; tiles and text adapt (light/dark) | dark gray |
| Start with Windows | Adds WinPick to the user's autostart | off |

If a hotkey is already used by Windows or another program, WinPick tells you and keeps the previous one.

Settings are stored in `WinPick.ini` in the same folder as the EXE. If that folder is read-only, WinPick uses `%APPDATA%\WinPick\WinPick.ini`.

## Uninstall

```powershell
irm https://raw.githubusercontent.com/schlingensiepen/winpick/main/deploy/uninstall.ps1 | iex
```

The script stops WinPick, removes the autostart entry and deletes the program folder including the settings. For a manual installation: turn off "Start with Windows", exit WinPick from the tray icon and delete the EXE.

## FAQ

**A tile shows "no preview" or "minimized".**
Some windows, especially minimized ones, do not provide an image to the Desktop Window Manager. WinPick then shows the program icon. Searching and bringing the window to the front still work.

**A window does not come to the front.**
Windows of programs running as administrator cannot be controlled by a normal program, for security reasons. Workaround: run WinPick as administrator as well. Windows that are not responding are skipped on purpose.

**Windows on other virtual desktops are missing.**
This is intended: WinPick shows the windows of the current virtual desktop.

**Win+J does not work.**
The hotkey is probably taken. WinPick reports this on startup. Choose a different hotkey in the settings.
