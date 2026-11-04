# =============================================================================
#  WinPick - install / update and start
#  Version: 2026-11-04 14:45 (Europe/Berlin)
#
#  Usage (PowerShell, no admin rights required):
#    irm https://raw.githubusercontent.com/schlingensiepen/winpick/main/deploy/install.ps1 | iex
#
#  Downloads WinPick.exe from the latest GitHub release to
#  %LOCALAPPDATA%\Programs\WinPick and verifies the SHA-256 checksum. A running
#  WinPick is stopped first. Finally it asks whether to start WinPick; if yes,
#  WinPick starts with its settings dialog open.
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

    # Report the hotkey actually configured (an existing WinPick.ini may override Win+J).
    $hotkey = 'Win+J'
    $ini = @((Join-Path $dir 'WinPick.ini'), (Join-Path $env:APPDATA 'WinPick\WinPick.ini')) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($ini) {
        $cfg = @{}
        foreach ($line in Get-Content $ini) { if ($line -match '^\s*(\w+)\s*=\s*(.*?)\s*$') { $cfg[$Matches[1]] = $Matches[2] } }
        $mods = 0; $vk = 0
        if ([int]::TryParse([string]$cfg['HotkeyMods'], [ref]$mods) -and [int]::TryParse([string]$cfg['HotkeyKey'], [ref]$vk) -and $mods -ne 0 -and $vk -ne 0) {
            $parts = @()
            if ($mods -band 8) { $parts += 'Win' }
            if ($mods -band 2) { $parts += 'Ctrl' }
            if ($mods -band 1) { $parts += 'Alt' }
            if ($mods -band 4) { $parts += 'Shift' }
            $names = @{ 0x20 = 'Space'; 0x09 = 'Tab'; 0xBE = '.'; 0xBC = ','; 0xBD = '-'; 0xBB = '+' }
            if (($vk -ge 0x30 -and $vk -le 0x39) -or ($vk -ge 0x41 -and $vk -le 0x5A)) { $key = [string][char]$vk }
            elseif ($vk -ge 0x70 -and $vk -le 0x87) { $key = 'F' + ($vk - 0x6F) }
            elseif ($names.ContainsKey($vk)) { $key = $names[$vk] }
            else { $key = '#' + $vk }
            $hotkey = (($parts + $key) -join '+')
        }
        Write-Host "Using existing settings: $ini"
    }
    Write-Host "WinPick installed to $dir. Hotkey: $hotkey"

    # Ask whether to start now (Enter = yes; J/Y = yes, N = no). Non-interactive: start.
    $start = $true
    try {
        $answer = Read-Host 'Start WinPick now? The settings dialog will open [Y/n]'
        if ($answer -match '^\s*[nN]') { $start = $false }
    } catch { }
    if ($start) {
        Start-Process -FilePath $exe -ArgumentList '--settings'
        Write-Host 'WinPick started. Check or change the hotkey in the settings dialog.'
    } else {
        Write-Host "WinPick was not started. Start it later with: & '$exe'"
    }
}
