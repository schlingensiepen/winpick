# =============================================================================
#  WinPick - install / update and start
#  Version: 2026-11-04 12:50 (Europe/Berlin)
#
#  Usage (PowerShell, no admin rights required):
#    irm https://raw.githubusercontent.com/schlingensiepen/winpick/main/deploy/install.ps1 | iex
#
#  Downloads WinPick.exe from the latest GitHub release to
#  %LOCALAPPDATA%\Programs\WinPick, verifies the SHA-256 checksum and starts
#  it. A running WinPick is stopped first.
#  Optional: $env:WINPICK_VERSION = 'v1.0.0' selects a specific version.
# =============================================================================
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference    = 'SilentlyContinue'   # speeds up Invoke-WebRequest in PS 5.1
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'schlingensiepen/winpick'
    $tag  = $env:WINPICK_VERSION
    $base = if ($tag) { "https://github.com/$repo/releases/download/$tag" } else { "https://github.com/$repo/releases/latest/download" }
    $dir  = Join-Path $env:LOCALAPPDATA 'Programs\WinPick'
    $exe  = Join-Path $dir 'WinPick.exe'
    $tmp  = "$exe.download"

    Write-Host "WinPick: downloading $base/WinPick.exe ..."
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Invoke-WebRequest -UseBasicParsing -Uri "$base/WinPick.exe" -OutFile $tmp
    $expected = (Invoke-WebRequest -UseBasicParsing -Uri "$base/WinPick.exe.sha256").Content
    if ($expected -is [byte[]]) { $expected = [Text.Encoding]::ASCII.GetString($expected) }
    $expected = ($expected -split '\s+')[0].Trim().ToLowerInvariant()
    $actual   = (Get-FileHash -Algorithm SHA256 -Path $tmp).Hash.ToLowerInvariant()
    if ($expected -ne $actual) {
        Remove-Item -Force $tmp
        throw "Checksum mismatch (expected $expected, got $actual). Aborting."
    }

    Get-Process -Name 'WinPick' -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 300
    Move-Item -Force -Path $tmp -Destination $exe
    Unblock-File -Path $exe

    Start-Process -FilePath $exe
    Write-Host "WinPick installed to $dir and started. Hotkey: Win+J (change it via the tray icon)."
}
