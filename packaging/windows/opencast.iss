#if GetEnv('OPENCAST_VERSION') == ""
  #define AppVersion "0.2.0"
#else
  #define AppVersion GetEnv('OPENCAST_VERSION')
#endif
[Setup]
AppId={{11E34A8A-DC59-4BD1-A426-36EC26649742}
AppName=OpenCast
AppVersion={#AppVersion}
AppPublisher=OpenCast contributors
AppPublisherURL=https://github.com/AverWasTaken/opencast
AppSupportURL=https://github.com/AverWasTaken/opencast/issues
DefaultDirName={localappdata}\Programs\OpenCast
DefaultGroupName=OpenCast
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=OpenCast-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\..\assets\opencast.ico
UninstallDisplayIcon={app}\opencast.exe
LicenseFile=..\..\LICENSE
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: checkedonce

Name: "startup"; Description: "Start OpenCast when I sign in"; GroupDescription: "Background launcher:"; Flags: checkedonce

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "OpenCast"; ValueData: """{app}\opencast.exe"" --background"; Tasks: startup; Flags: uninsdeletevalue

[Files]
Source: "..\..\target\release\opencast.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\OpenCast"; Filename: "{app}\opencast.exe"; Comment: "Search files and calculate"
Name: "{autodesktop}\OpenCast"; Filename: "{app}\opencast.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\opencast.exe"; Description: "Launch OpenCast"; Flags: nowait postinstall skipifsilent

[Code]
procedure StopResident();
var
  Resident: HWND;
  Attempts: Integer;
begin
  Resident := FindWindowByClassName('OpenCast.Resident.v2');
  if Resident <> 0 then
  begin
    PostMessage(Resident, $8003, 0, 0);
    for Attempts := 1 to 100 do
    begin
      if FindWindowByClassName('OpenCast.Resident.v2') = 0 then Break;
      Sleep(50);
    end;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopResident();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  StopResident();
  Result := True;
end;
