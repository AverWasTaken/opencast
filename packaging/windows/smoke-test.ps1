$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class LauncherTest {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hwnd,uint msg,IntPtr w,IntPtr l);
  [DllImport("user32.dll")] public static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
}
'@
function Press-Keys([byte[]]$keys) {
    foreach ($key in $keys) { [LauncherTest]::keybd_event($key,0,0,[UIntPtr]::Zero) }
    Start-Sleep -Milliseconds 80
    for ($i=$keys.Length-1;$i -ge 0;$i--) { [LauncherTest]::keybd_event($keys[$i],0,2,[UIntPtr]::Zero) }
    Start-Sleep -Milliseconds 600
}
function Wait-Visibility($handle,[bool]$expected) {
    for ($i=0;$i -lt 30;$i++) {
        if ([LauncherTest]::IsWindowVisible($handle) -eq $expected) { return }
        Start-Sleep -Milliseconds 200
    }
    throw "Window visibility did not become $expected"
}
$installer = Get-ChildItem dist/*-setup.exe | Select-Object -First 1
if (!$installer) { throw 'Installer was not produced' }
$installDir = Join-Path $env:RUNNER_TEMP 'OpenCast install test'
$setup = Start-Process $installer.FullName -ArgumentList "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /DIR=`"$installDir`"" -Wait -PassThru
if ($setup.ExitCode -ne 0) { throw "Installer failed: $($setup.ExitCode)" }
$appPath = Join-Path $installDir 'opencast.exe'
if (!(Test-Path $appPath)) { throw 'Installed executable is missing' }
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'OpenCast.lnk'
if (!(Test-Path $shortcut)) { throw 'Start menu shortcut is missing' }
$errorLog = Join-Path $env:RUNNER_TEMP 'opencast-startup.log'
$app = Start-Process $appPath -PassThru -RedirectStandardError $errorLog
try {
    Start-Sleep -Seconds 5
    $app.Refresh()
    if ($app.HasExited) { Get-Content $errorLog; throw "App exited during startup: $($app.ExitCode)" }
    $window=[LauncherTest]::FindWindow($null,'OpenCast')
    if ($window -eq [IntPtr]::Zero) { throw 'Launcher window is missing' }
    $resident=[LauncherTest]::FindWindow('OpenCast.Resident.v2',$null)
    if ($resident -eq [IntPtr]::Zero) { throw 'Resident message window is missing' }
    [LauncherTest]::SetForegroundWindow($window) | Out-Null
    Press-Keys @(0x12,0x20) # Default Alt+Space hides the focused launcher.
    Wait-Visibility $window $false
    Press-Keys @(0x12,0x20)
    Wait-Visibility $window $true
    Press-Keys @(0x1B) # Escape hides, with the process still alive.
    Wait-Visibility $window $false
    $second=Start-Process $appPath -PassThru
    if (!$second.WaitForExit(5000)) { throw 'Second invocation created another running instance' }
    Wait-Visibility $window $true

    # Exercise the actual shortcut recorder through Windows accessibility.
    Press-Keys @(0x11,0xBC) # Ctrl+comma opens Settings.
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($window)
    $record=$null
    for ($attempt=0;$attempt -lt 20;$attempt++) {
        $controls=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
        foreach($control in $controls) { if($control.Current.Name -like '*Change shortcut*') { $record=$control; break } }
        if($record) { break }; Start-Sleep -Milliseconds 300
    }
    if (!$record) { throw 'Shortcut recorder is missing from the accessibility tree' }
    $rect=$record.Current.BoundingRectangle
    [LauncherTest]::SetCursorPos([int]($rect.X+$rect.Width/2),[int]($rect.Y+$rect.Height/2)) | Out-Null
    [LauncherTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    [LauncherTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 300
    Press-Keys @(0x11,0x10,0x4B) # Record Ctrl+Shift+K.
    $configPath=Join-Path $env:LOCALAPPDATA 'OpenCast\OpenCast\data\config.json'
    $config=Get-Content $configPath -Raw | ConvertFrom-Json
    if($config.shortcut.modifiers -ne 6 -or $config.shortcut.key -ne 75) { throw 'Custom shortcut was not saved' }
    Press-Keys @(0x1B) # Close Settings.
    Press-Keys @(0x11,0x10,0x4B)
    Wait-Visibility $window $false
    Press-Keys @(0x11,0x10,0x4B)
    Wait-Visibility $window $true
    $quit=Start-Process $appPath -ArgumentList '--quit' -PassThru -Wait
    if (!$app.WaitForExit(5000)) { throw 'Quit did not stop the resident app' }

    $app=Start-Process $appPath -ArgumentList '--background' -PassThru
    Start-Sleep -Seconds 4
    $window=[LauncherTest]::FindWindow($null,'OpenCast')
    Wait-Visibility $window $false
    Press-Keys @(0x11,0x10,0x4B)
    Wait-Visibility $window $true
    Write-Host 'Default/custom hotkeys, recorder, persistence, background launch and single-instance activation passed.'
} finally {
    if (!$app.HasExited) {
        Start-Process $appPath -ArgumentList '--quit' -Wait
        if (!$app.WaitForExit(5000)) { Stop-Process -Id $app.Id -Force }
    }
}
$uninstaller = Start-Process (Join-Path $installDir 'unins000.exe') -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
if ($uninstaller.ExitCode -ne 0) { throw "Uninstall failed: $($uninstaller.ExitCode)" }
if (Test-Path $appPath) { throw 'Uninstall left the executable behind' }
if (Test-Path $shortcut) { throw 'Uninstall left the Start menu shortcut behind' }
Write-Host 'Install, launch, shortcuts and uninstall passed.'
