<#
  Tests the GitHub workflow runs after the build.

  First it looks at each file: version, size and the Windows security flags.
  Then it opens the installer, the uninstaller and the settings window, does
  a silent install, and fails if Windows reports a crash along the way.

  It installs Crow FO and writes to the registry, so it only runs on GitHub
  Actions.
#>
param(
    [Parameter(Mandatory)] [string]$Version,
    [Parameter(Mandatory)] [string]$Display,
    [int]$Build = 0,
    [string]$Dir = 'target\x86_64-pc-windows-msvc\release',
    [long]$MaxSize = 2MB
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') {
    throw 'This script installs Crow FO and edits the registry. Run it on GitHub Actions only.'
}

$started = Get-Date
$failures = [System.Collections.Generic.List[string]]::new()
$rows = [System.Collections.Generic.List[string]]::new()
function Fail([string]$msg) {
    $failures.Add($msg)
    Write-Host "::error::$msg"
}
function Pass([string]$msg) {
    $rows.Add("| :white_check_mark: | $msg |")
    Write-Host "ok  $msg"
}

# ---------------------------------------------------------------- the files
$numeric = ($Version -split '[-+]')[0]
$fileVersion = "$numeric.$Build"

foreach ($name in 'CrowFO.exe', 'CrowFOSetup.exe', 'crow_menu.dll') {
    $path = Join-Path $Dir $name
    if (-not (Test-Path $path)) {
        Fail "$name was not built."
        continue
    }
    $f = Get-Item $path
    $vi = $f.VersionInfo
    if ($f.Length -gt $MaxSize) {
        Fail ("{0} is {1:N0} KB, over the {2:N0} KB limit." -f $name, ($f.Length / 1KB), ($MaxSize / 1KB))
    }
    if ($vi.FileVersion -ne $fileVersion) {
        Fail "$name has file version '$($vi.FileVersion)' instead of '$fileVersion'."
    }
    if ($vi.ProductVersion -ne $Display) {
        Fail "$name has product version '$($vi.ProductVersion)' instead of '$Display'."
    }

    # ASLR, high entropy ASLR and DEP, read from DllCharacteristics in the PE header
    $bytes = [System.IO.File]::ReadAllBytes($path)
    $pe = [BitConverter]::ToInt32($bytes, 0x3C)
    $dllChars = [BitConverter]::ToUInt16($bytes, $pe + 24 + 70)
    $missing = @()
    if (-not ($dllChars -band 0x0040)) { $missing += 'DYNAMICBASE' }
    if (-not ($dllChars -band 0x0020)) { $missing += 'HIGHENTROPYVA' }
    if (-not ($dllChars -band 0x0100)) { $missing += 'NXCOMPAT' }
    if ($missing) {
        Fail "$name is missing $($missing -join ', ')."
    }
    if (-not $failures.Where({ $_ -like "$name *" })) {
        Pass ("{0}, {1:N0} KB, version {2}" -f $name, ($f.Length / 1KB), $vi.ProductVersion)
    }
}

# ---------------------------------------------------------------- crashes
function Get-Crashes {
    Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000; StartTime = $started } -ErrorAction SilentlyContinue |
        Where-Object { $_.Message -match 'CrowFO' }
}

# Opens a program, waits for its window and makes sure it didn't crash.
function Test-Window([string]$exe, [string]$arguments, [string]$what) {
    $p = if ($arguments) { Start-Process $exe -ArgumentList $arguments -PassThru } else { Start-Process $exe -PassThru }
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline -and -not $p.HasExited) {
        $p.Refresh()
        if ($p.MainWindowHandle -ne 0) { break }
        Start-Sleep -Milliseconds 250
    }
    Start-Sleep -Seconds 2
    $p.Refresh()
    if ($p.HasExited) {
        Fail ("{0} closed on its own (exit code 0x{1:X8})." -f $what, $p.ExitCode)
    } elseif ($p.MainWindowHandle -eq 0) {
        Fail "$what never showed a window."
    } else {
        Pass "$what opens ('$($p.MainWindowTitle)')"
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
}

$app = (Resolve-Path (Join-Path $Dir 'CrowFO.exe')).Path
$setup = (Resolve-Path (Join-Path $Dir 'CrowFOSetup.exe')).Path

# The app draws its own icons for packaging. Make sure that still works.
$art = Join-Path $env:RUNNER_TEMP 'crow-art'
$p = Start-Process $app -ArgumentList "--export-art `"$art`"" -PassThru -Wait
if ($p.ExitCode -ne 0 -or -not (Test-Path (Join-Path $art 'crow.ico')) -or -not (Test-Path (Join-Path $art 'Assets\Logo150.png'))) {
    Fail "CrowFO.exe --export-art failed (exit code $($p.ExitCode))."
} else {
    Pass 'CrowFO.exe --export-art draws the icons'
}

Test-Window $setup '' 'The installer window'
Test-Window $app '--uninstall' 'The uninstall window'

# Install for real, the way winget and scripts do. Start-Process -Wait would
# also wait for the app the installer starts at the end, which never exits,
# so only the installer itself is waited for.
$p = Start-Process $setup -ArgumentList '/S' -PassThru
$null = $p.Handle # keeps the exit code readable after the process ends
$installed = Join-Path $env:LOCALAPPDATA 'Programs\Crow FO\CrowFO.exe'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CrowFO'
$link = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Crow FO.lnk'
if (-not $p.WaitForExit(60000)) {
    Fail 'The silent install was still running after 60 seconds.'
    Stop-Process -Id $p.Id -Force
} elseif ($p.ExitCode -ne 0) {
    Fail "The silent install failed (exit code $($p.ExitCode))."
} else {
    $entry = Get-ItemProperty $key -ErrorAction SilentlyContinue
    if (-not (Test-Path $installed)) { Fail "The silent install didn't copy CrowFO.exe." }
    elseif ((Get-Item $installed).VersionInfo.ProductVersion -ne $Display) { Fail "The installed CrowFO.exe isn't the one from this build." }
    elseif (-not $entry) { Fail "Crow FO doesn't show up in Installed apps after the silent install." }
    elseif ($entry.DisplayVersion -ne $Version) { Fail "Installed apps shows version '$($entry.DisplayVersion)' instead of '$Version'." }
    elseif (-not (Test-Path $link)) { Fail "The silent install didn't create the Start menu shortcut." }
    else { Pass "Silent install works, Installed apps shows $Version" }
}

# After a silent install the app starts in the background and creates its
# first folders.
Start-Sleep -Seconds 5
$running = Get-Process CrowFO -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $installed }
if (-not $running) {
    Fail "Crow FO isn't running after the silent install."
} else {
    Pass ("Crow FO is running after the install ({0:N0} KB of memory)" -f ($running[0].WorkingSet64 / 1KB))
    # Starting it again asks the running copy to open the settings window.
    $s = Start-Process $installed -ArgumentList '--settings' -PassThru
    if (-not $s.WaitForExit(15000)) { Stop-Process -Id $s.Id -Force }
    Start-Sleep -Seconds 3
    $running[0].Refresh()
    if ($running[0].HasExited) { Fail 'Crow FO closed after opening the settings window.' }
    else { Pass 'The settings window opens' }
}

$crashes = @(Get-Crashes)
foreach ($c in $crashes) {
    $first = ($c.Message -split "`n" | Select-Object -First 4) -join ' '
    Fail "Windows reported a crash: $first"
}
if (-not $crashes) { Pass 'Windows reported no crashes' }

Get-Process CrowFO, CrowFOSetup -ErrorAction SilentlyContinue | Stop-Process -Force

# ---------------------------------------------------------------- summary
if ($env:GITHUB_STEP_SUMMARY) {
    $out = @('### Tests', '', '| | |', '|---|---|') + $rows
    foreach ($f in $failures) { $out += "| :x: | $f |" }
    $out += ''
    $out | Add-Content $env:GITHUB_STEP_SUMMARY -Encoding utf8
}
if ($failures.Count) {
    Write-Host "$($failures.Count) test(s) failed."
    exit 1
}
Write-Host 'All tests passed.'
