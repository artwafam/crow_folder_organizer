<#
  Developer install. End users should use CrowSetup.exe instead.

  Installs Crow and registers its entry in the Windows 11 desktop context menu.

  The script copies crow.exe and crow_menu.dll to the install folder, builds a
  sparse package from packaging\AppxManifest.xml, signs it with a single-use
  self-signed certificate and registers it. Windows asks for admin rights once
  per install so the certificate can be trusted.

  Usage:  cargo build --release --workspace
          powershell -ExecutionPolicy Bypass -File tools\dev-install.ps1
#>
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\Crow",
    [switch]$NoContextMenu
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$rel  = Join-Path $repo 'target\x86_64-pc-windows-msvc\release'
$out  = Join-Path $PSScriptRoot '.out'
$sdk  = Join-Path $PSScriptRoot '.sdk'

foreach ($f in 'crow.exe', 'crow_menu.dll') {
    if (-not (Test-Path (Join-Path $rel $f))) { throw "$f is missing. Run: cargo build --release --workspace" }
}

Write-Host '1/5 Copying files'
Get-Process crow -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
New-Item -ItemType Directory -Force $InstallDir | Out-Null
Copy-Item (Join-Path $rel 'crow.exe'), (Join-Path $rel 'crow_menu.dll') $InstallDir -Force
$p = Start-Process (Join-Path $InstallDir 'crow.exe') -ArgumentList "--export-art `"$InstallDir`"" -PassThru -Wait
if ($p.ExitCode -ne 0) { throw 'Could not generate the icons.' }

if (-not $NoContextMenu) {
    Write-Host '2/5 Getting MakeAppx and SignTool'
    $makeappx = Get-ChildItem $sdk -Recurse -Filter makeappx.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match '\\x64\\' } | Select-Object -First 1
    if (-not $makeappx) {
        New-Item -ItemType Directory -Force $sdk | Out-Null
        $zip = Join-Path $sdk 'buildtools.zip'
        Invoke-WebRequest 'https://www.nuget.org/api/v2/package/Microsoft.Windows.SDK.BuildTools' -OutFile $zip
        Expand-Archive $zip (Join-Path $sdk 'pkg') -Force
        Remove-Item $zip
        $makeappx = Get-ChildItem $sdk -Recurse -Filter makeappx.exe | Where-Object { $_.FullName -match '\\x64\\' } | Select-Object -First 1
    }
    $signtool = Join-Path $makeappx.DirectoryName 'signtool.exe'
    # Only run the tools if Microsoft really signed them.
    foreach ($t in $makeappx.FullName, $signtool) {
        $sig = Get-AuthenticodeSignature $t
        if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
            throw "Invalid signature on $t"
        }
    }

    Write-Host '3/5 Building the package'
    $layout = Join-Path $out 'layout'
    Remove-Item $layout -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force (Join-Path $layout 'Assets') | Out-Null
    Copy-Item (Join-Path $repo 'packaging\AppxManifest.xml') $layout
    Copy-Item (Join-Path $InstallDir 'Assets\*') (Join-Path $layout 'Assets')
    $msix = Join-Path $out 'crow.msix'
    & $makeappx.FullName pack /d $layout /p $msix /nv /o | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'MakeAppx failed.' }

    Write-Host '4/5 Signing'
    # The signing key is created, used once and destroyed. No private key is left
    # on the machine, so nothing can reuse it to sign packages Windows would trust.
    # The trade-off is one admin prompt on every install.
    Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq 'CN=Crow' } |
        ForEach-Object { Remove-Item -Path "Cert:\CurrentUser\My\$($_.Thumbprint)" -DeleteKey }
    $cert = New-SelfSignedCertificate -Type Custom -Subject 'CN=Crow' -FriendlyName 'Crow (local package)' `
        -KeyUsage DigitalSignature -KeyExportPolicy NonExportable -NotAfter (Get-Date).AddYears(5) `
        -CertStoreLocation Cert:\CurrentUser\My `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
    try {
        & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $msix | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'SignTool failed.' }
        $cer = Join-Path $out 'crow.cer'
        Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    } finally {
        Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)" -DeleteKey
    }
    Write-Host '   Trusting the certificate (please accept the admin prompt)'
    # One admin prompt does two things. It trusts only the new public certificate
    # (dropping any older Crow one), and it lets this user change the shared
    # desktop, so shortcuts installed for all users can be moved into Crow too.
    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $pub = [Environment]::GetFolderPath('CommonDesktopDirectory')
    $cmd = "Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object { `$_.Subject -eq 'CN=Crow' } | Remove-Item; " +
           "Import-Certificate -FilePath '$cer' -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null; " +
           "icacls '$pub' /grant '*${sid}:(OI)(CI)M' | Out-Null"
    $p = Start-Process powershell -Verb RunAs -Wait -PassThru -ArgumentList '-NoProfile', '-Command', $cmd
    if ($p.ExitCode -ne 0) { throw 'The certificate could not be trusted.' }

    Write-Host '5/5 Registering the context menu'
    Get-AppxPackage -Name 'Crow.FolderOrganizer' | Remove-AppxPackage
    Add-AppxPackage -Path $msix -ExternalLocation $InstallDir
}

Start-Process (Join-Path $InstallDir 'crow.exe')
Write-Host "Done. Crow is installed in $InstallDir"
