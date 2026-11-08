<#
  Gets a release ready. Sets the version in Cargo.toml, updates Cargo.lock
  and commits. Nothing gets pushed, so you can look over the commit first.

  Usage:  powershell -ExecutionPolicy Bypass -File tools\release.ps1 1.2.0
  Then:   git push origin main

  Pushing doesn't publish anything. When you want the release out, click
  "Run workflow" in the Actions tab on GitHub. It builds, tests, creates the
  tag v1.2.0 and publishes the release. Versions with a suffix, like
  1.2.0-beta.1, come out as pre-releases.
#>
param(
    [Parameter(Mandatory, Position = 0)] [string]$Version
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Set-Location $repo

if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    throw "'$Version' doesn't look like a version. Use something like 1.2.0 or 1.2.0-beta.1."
}
foreach ($n in ($Version -split '[-+]')[0].Split('.')) {
    if ([int]$n -gt 65535) { throw "Each number in the version has to be 65535 or less. That's a Windows limit." }
}
if (git status --porcelain) { throw 'There are uncommitted changes. Commit or stash them first.' }
$branch = git rev-parse --abbrev-ref HEAD
if ($branch -ne 'main') { throw "Switch to main first. You're on '$branch'." }
if (git tag --list "v$Version") { throw "Version $Version was already released (the tag v$Version exists)." }

$cargo = Get-Command cargo -ErrorAction SilentlyContinue
$cargo = if ($cargo) { $cargo.Source } else { Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
if (-not (Test-Path $cargo)) { throw "Couldn't find cargo. Install Rust from https://rustup.rs." }

# Only the version under [workspace.package] changes.
$toml = Get-Content Cargo.toml -Raw
$re = [regex]::new('(?ms)(^\[workspace\.package\]\s*$.*?^version\s*=\s*)"([^"]*)"')
$m = $re.Match($toml)
if (-not $m.Success) { throw "Couldn't find the version under [workspace.package] in Cargo.toml." }
$old = $m.Groups[2].Value
$toml = $re.Replace($toml, "`${1}`"$Version`"", 1)
[System.IO.File]::WriteAllText((Join-Path $repo 'Cargo.toml'), $toml)

& $cargo update --workspace --quiet
if ($LASTEXITCODE -ne 0) { throw 'cargo update failed.' }

git add Cargo.toml Cargo.lock
git commit --quiet -m "Version $Version"

Write-Host "Done. $old is now $Version, in a new commit."
Write-Host "Push it with:  git push origin main"
Write-Host "Then click 'Run workflow' in the Actions tab when you want the release out."
