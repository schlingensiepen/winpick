# WinPick – Developer guide

## Overview

WinPick is a native Win32 application written in Rust on top of Microsoft's [`windows`](https://crates.io/crates/windows) crate, without a UI framework. The goal is a single executable with a statically linked C runtime that imports only Windows system libraries. CI checks that no Visual C++ runtime DLL is imported.

Minimum system: Windows 10 (uses `GetDpiForMonitor`, `AdjustWindowRectExForDpi`, `SystemParametersInfoForDpi`, per-monitor DPI v2).

## Layout

```
src/main.rs        the whole application, organized in sections (see below)
res/app.manifest   common controls v6, PerMonitorV2 DPI, asInvoker
build.rs           generates app.ico from code, writes the resource script
                   (icon, manifest, version info) and embeds it
.cargo/config.toml static CRT for the MSVC targets
deploy/*.ps1       install/uninstall via irm | iex
Cargo.toml         the single source of the version number
```

Sections in `main.rs`:

| Section | Content |
|---|---|
| Settings | `Settings` struct, INI load/save (`GetPrivateProfile*`), fallback to `%APPDATA%`, validation of the hotkey |
| Autostart | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; updated on startup if the EXE was moved |
| Hotkey names | display names for keys and hotkeys |
| Theme | derives all colors from the background color (light/dark), accent from `DwmGetColorizationColor` |
| State | one `State` struct for the single UI thread, reached through `st()` |
| Window enumeration | `EnumWindows` with filters (see below), DWM thumbnail registration, own icon copies |
| Layout | tile geometry, hit testing, positioning and clipping of the thumbnails |
| Show / hide / activate | overlay placement, z-order logic |
| Drawing | double buffering; shapes with the GDI+ flat API (anti-aliased), text with GDI (ClearType) |
| Tray, search box, window procedures, settings dialog, entry point | as named |

### State handling

All UI code runs on one thread. Window procedures re-enter each other (for example `SetWindowTextW` on the search box sends `EN_CHANGE` back to the overlay). The state therefore lives in a heap-allocated `State` reached through `st()`, not in a `RefCell`, which would panic on re-entry. Rule: never keep a reference obtained from `st()` across a call that can send a window message; call `st()` again afterwards.

### Window selection

`EnumWindows` returns windows in z-order, most recently used first. WinPick takes top-level windows that meet all of these conditions:
- visible, with a non-empty title,
- not from its own process,
- no `WS_EX_TOOLWINDOW`,
- no owner (or `WS_EX_APPWINDOW`),
- not `DWMWA_CLOAKED` (this excludes windows on other virtual desktops and hidden UWP windows),
- not a shell class (`Progman`, `WorkerW`, `Shell_TrayWnd`, `Shell_SecondaryTrayWnd`).

Icons are fetched with `WM_GETICON` (`SendMessageTimeoutW`, 60 ms, abort if hung) and copied with `CopyIcon`, because the other process may destroy its icon at any time.

### Previews

When the overlay opens, a DWM thumbnail is registered for each window (`DwmRegisterThumbnail`). They are released again when it closes. DWM composites thumbnails over the window, so GDI clipping does not affect them. `update_thumbs()` therefore computes the visible part itself while scrolling: it intersects the destination rectangle with the grid area and adjusts `rcSource` to match. If a window reports a source size below 48 px (typical for minimized windows), the program icon is drawn instead.

### Bringing windows to the front

Windows only lets the process that received the user's last input call `SetForegroundWindow`. WinPick has that right in exactly two situations: after the hotkey (`WM_HOTKEY`) and after the user's Enter or click in the overlay. It relies on that alone.

WinPick deliberately avoids the common tricks for working around the foreground lock:
- **`AttachThreadInput`** merges the input queues of two threads, so a busy thread stalls the other ([The dangers of attaching input queues](https://devblogs.microsoft.com/oldnewthing/20080801-00/?p=21393)).
- **Injected key presses (Alt)** arrive asynchronously, often in the window that was just activated. There they open menu mode, for example the menu bar in Firefox.

An earlier implementation used both. After using it, other programs sometimes misbehaved for a while.

`activate_windows()` works like this:
1. Discard windows that no longer exist or do not respond (`IsHungAppWindow`).
2. Hide the overlay, so nothing is reordered underneath a topmost window.
3. For each window, back to front: restore minimized windows with `ShowWindowAsync` and place the window on top with `SetWindowPos(HWND_TOP, SWP_NOACTIVATE)`.
4. Activate the front window with `SetForegroundWindow`.

For Enter, the shown list is passed in reverse order. The first tile therefore ends up in front, and the others keep their relative order. With an empty search box, only the first tile is activated, so that Enter does not restore every minimized window on the desktop at once.

If the overlay does not get the foreground when it opens (for example on a second start without foreground rights), it is hidden again immediately rather than staying on top without receiving input.

Known limit: UIPI (privilege isolation between processes) prevents a normal process from moving or activating windows of elevated processes.

### DPI

Per-monitor v2 applies process-wide (manifest plus `SetProcessDpiAwarenessContext`). The DPI is read from the target monitor every time the overlay opens. Fonts and all sizes are scaled through `s()`. `WM_DPICHANGED` is ignored on purpose, since position and size are set in `show_overlay()` anyway.

## Building

### Windows

Install Rust ([rustup](https://rustup.rs/)) with the MSVC toolchain and the Visual Studio Build Tools (C++ workload, Windows SDK), then:

```powershell
cargo build --release
```

Result: `target\release\WinPick.exe`.

### Checking on Linux

Linking and resource compilation require the Windows SDK. On Linux the code can still be type-checked and linted:

```sh
rustup target add x86_64-pc-windows-msvc
cargo clippy --target x86_64-pc-windows-msvc --release
```

`build.rs` skips the resources on non-Windows hosts and prints a warning.

## Release process

1. Increase `version` in `Cargo.toml` (SemVer) and run `cargo check` so that `Cargo.lock` is updated.
2. Add a section `## [x.y.z] – YYYY-MM-DD` to `CHANGELOG.md`; it becomes the release notes.
3. Push to `main`.

The workflow `.github/workflows/build.yml` builds on every push and PR on `windows-latest`, runs clippy with `-D warnings` and uploads the EXE as an artifact. On `main` it also creates the release `v<version>` with `WinPick.exe` and `WinPick.exe.sha256` if the tag does not exist yet. `deploy/install.ps1` always fetches `releases/latest` and verifies the checksum.

## Conventions

- Every source file and script carries a line `Version: YYYY-MM-DD HH:MM (Europe/Berlin)` in its header, updated on every change.
- UI, documentation and identifiers are in English.
- PowerShell scripts stay pure ASCII with CRLF, so Windows PowerShell 5.1 reads them correctly regardless of encoding.
- No runtime DLLs beyond Windows system libraries.

## Testing

There are no automated UI tests. Before a release, check on a real Windows machine:

- [ ] Start, tray icon, hotkey opens and closes the overlay
- [ ] Overlay on the monitor under the cursor, also with different scaling (100 %/150 %)
- [ ] Live previews, scrolling with clipping at the top edge
- [ ] Filter (non-ASCII characters, upper/lower case), counter, empty result
- [ ] Enter: order, minimized windows are restored, focus on the first tile; empty search box activates only the first tile
- [ ] Click on a tile, Esc, click outside
- [ ] After repeated use: Firefox and other programs keep repainting normally, no menu bar pops up, no stuck modifier keys
- [ ] Settings: a taken hotkey is rejected, INI is written, autostart
- [ ] A second start opens the overlay of the running instance
- [ ] Explorer restart: the tray icon reappears
