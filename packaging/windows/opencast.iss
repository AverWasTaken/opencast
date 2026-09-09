#if GetEnv('OPENCAST_VERSION') == ""
  #define AppVersion "0.1.0"
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

[Files]
Source: "..\..\target\release\opencast.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\OpenCast"; Filename: "{app}\opencast.exe"; Comment: "Search files and calculate"; HotKey: "ctrl+alt+o"
Name: "{autodesktop}\OpenCast"; Filename: "{app}\opencast.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\opencast.exe"; Description: "Launch OpenCast"; Flags: nowait postinstall skipifsilent
