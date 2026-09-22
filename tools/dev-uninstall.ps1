<#
  Removes Crow: the context menu package, the startup entry, the classic menu
  entry and the install folder. Your folders and shortcuts in Desktop\.crow are
  kept.
#>
param([string]$InstallDir = "$env:LOCALAPPDATA\Programs\Crow")
Get-Process crow -ErrorAction SilentlyContinue | Stop-Process -Force
Get-AppxPackage -Name 'Crow.FolderOrganizer' | Remove-AppxPackage
Remove-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name Crow -ErrorAction SilentlyContinue
Remove-Item 'HKCU:\Software\Classes\DesktopBackground\Shell\Crow' -Recurse -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item "$env:LOCALAPPDATA\Crow" -Recurse -Force -ErrorAction SilentlyContinue
# Give back the shared desktop permission the installer granted (asks for admin).
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$pub = [Environment]::GetFolderPath('CommonDesktopDirectory')
Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile', '-Command', "icacls '$pub' /remove:g '*$sid' | Out-Null"
Write-Host 'Crow was removed. Your folders are still in Desktop\.crow.'
