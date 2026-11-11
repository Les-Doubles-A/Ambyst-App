; Ambyst-App installer (Inno Setup 6). Build: iscc installer\ambyst-app.iss
#define AppName "Ambyst-App"
#ifndef AppVersion
  #define AppVersion "1.1.0"
#endif

[Setup]
AppId={{8C4E5B6A-3F1D-4A7E-9B2C-AB57A9A11E01}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Les-Doubles-A
AppPublisherURL=https://github.com/Les-Doubles-A/Ambyst-App
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=Ambyst-App-Setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\ambyst-app.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\ambyst-app.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\ambyst-app.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\ambyst-app.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ambyst-app.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
