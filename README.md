# WinPick

**Install and start right away** (PowerShell, no admin rights required):

```powershell
irm https://raw.githubusercontent.com/schlingensiepen/winpick/main/deploy/install.ps1 | iex
```

---

WinPick is a window overview for Windows 10/11. Press a hotkey (default **Win+J**) and an overlay appears on the current screen. It shows live previews of all open windows in a grid. Type in the search box at the top to filter the windows by title, then press **Enter** to bring all matches to the front.

- A single portable EXE written in Rust, no installation, no runtime dependencies
- Live previews via the Desktop Window Manager (DWM)
- Title search, case-insensitive
- Enter: bring matches to the front · Click: one window · Esc: close
- Configurable: hotkey, number of columns, size, margin, background color, start with Windows
- Well-behaved: never injects key presses and never attaches to other programs' input queues

## Download

- Latest version: [Releases](https://github.com/schlingensiepen/winpick/releases/latest) → download `WinPick.exe` and run it
- Or use the one-liner above: it installs to `%LOCALAPPDATA%\Programs\WinPick`, verifies the SHA-256 checksum and asks whether to start WinPick right away (with the settings dialog open). The same command updates an existing installation.

Uninstall:

```powershell
irm https://raw.githubusercontent.com/schlingensiepen/winpick/main/deploy/uninstall.ps1 | iex
```

## Documentation

- [User guide](docs/user-guide.md): usage, settings, FAQ
- [Developer guide](docs/developer-guide.md): architecture, build, release process

## Project layout

```
src/        source code (Rust, Win32 via the windows crate)
res/        application manifest
build.rs    generates the icon and embeds icon, manifest and version info
deploy/     install and uninstall scripts (PowerShell)
docs/       user and developer documentation
.github/    CI: build on every push, release on a new version
```

## License

[Apache License 2.0](LICENSE), see also [NOTICE](NOTICE)
