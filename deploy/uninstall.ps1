# =============================================================================
#  WinPick - uninstall
#  Version: 2026-11-04 12:50 (Europe/Berlin)
#
#  Usage:
#    irm https://raw.githubusercontent.com/schlingensiepen/winpick/main/deploy/uninstall.ps1 | iex
#
#  Stops WinPick, removes the autostart entry and deletes
#  %LOCALAPPDATA%\Programs\WinPick (including WinPick.ini) and %APPDATA%\WinPick.
# =============================================================================
& {
    $ErrorActionPreference = 'Continue'
    Get-Process -Name 'WinPick' -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 300
    Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'WinPick' -ErrorAction SilentlyContinue
    foreach ($d in @((Join-Path $env:LOCALAPPDATA 'Programs\WinPick'), (Join-Path $env:APPDATA 'WinPick'))) {
        if (Test-Path $d) { Remove-Item -Recurse -Force $d; Write-Host "Removed: $d" }
    }
    Write-Host 'WinPick has been removed.'
}
