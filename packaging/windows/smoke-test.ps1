$ErrorActionPreference = 'Stop'
$installer = Get-ChildItem dist/*-setup.exe | Select-Object -First 1
if (!$installer) { throw 'Installer was not produced' }
$installDir = Join-Path $env:RUNNER_TEMP 'OpenCast install test'
$setup = Start-Process $installer.FullName -ArgumentList "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /DIR=`"$installDir`"" -Wait -PassThru
if ($setup.ExitCode -ne 0) { throw "Installer failed: $($setup.ExitCode)" }
$appPath = Join-Path $installDir 'opencast.exe'
if (!(Test-Path $appPath)) { throw 'Installed executable is missing' }
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'OpenCast.lnk'
if (!(Test-Path $shortcut)) { throw 'Start menu shortcut is missing' }
$app = Start-Process $appPath -PassThru
try {
    Start-Sleep -Seconds 4
    $app.Refresh()
    if ($app.HasExited) { throw "App exited during startup: $($app.ExitCode)" }
} finally {
    if (!$app.HasExited) { Stop-Process -Id $app.Id -Force }
}
$uninstaller = Start-Process (Join-Path $installDir 'unins000.exe') -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
if ($uninstaller.ExitCode -ne 0) { throw "Uninstall failed: $($uninstaller.ExitCode)" }
if (Test-Path $appPath) { throw 'Uninstall left the executable behind' }
if (Test-Path $shortcut) { throw 'Uninstall left the Start menu shortcut behind' }
Write-Host 'Install, launch, shortcuts and uninstall passed.'
