; Fanimate's Windows installer (Inno Setup 6).
;
; Built by the release workflow:
;   iscc /DAppVersion=1.2.3 /DSourceDir=<folder with fanimate.exe> /DOutputDir=<out> packaging\windows\fanimate.iss
;
; Installs for the current user, so no administrator rights are needed.
; The self-updater runs a newer installer with /SILENT: it closes Fanimate,
; replaces the files (rolling back if anything fails) and starts Fanimate
; again through the [Run] entry below.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\fanimate"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
; never change: Windows knows the installed app by this id
AppId={{6F3C2A9E-8C1B-4E57-9B0D-2F6A1C4E8D73}
AppName=Fanimate
AppVersion={#AppVersion}
AppVerName=Fanimate {#AppVersion}
AppPublisher=The Fanimate Contributors
AppPublisherURL=https://github.com/toonittmedia/fanimate
AppSupportURL=https://github.com/toonittmedia/fanimate/issues
AppUpdatesURL=https://github.com/toonittmedia/fanimate/releases
DefaultDirName={localappdata}\Programs\Fanimate
DefaultGroupName=Fanimate
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=Fanimate-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\brand\app-icon\fanimate.ico
UninstallDisplayIcon={app}\fanimate.exe
UninstallDisplayName=Fanimate
LicenseFile=..\..\LICENSE-MIT
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; tells Explorer the .fnm association changed
ChangesAssociations=yes
CloseApplications=yes
RestartApplications=no
; an update must not leave a half-installed copy
AllowCancelDuringInstall=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\fanimate.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\fanimate-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\brand\file-icon\fnm-file.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion
; the separate program Fanimate runs for lip sync
Source: "{#SourceDir}\rhubarb\*"; DestDir: "{app}\rhubarb"; Flags: ignoreversion recursesubdirs createallsubdirs skipifsourcedoesntexist

[Registry]
; .fnm files open in Fanimate and show its file icon. Written under HKA, so it
; is the current user's (no administrator needed) unless the installer was
; run for all users. Removed again on uninstall.
Root: HKA; Subkey: "Software\Classes\.fnm"; ValueType: string; ValueName: ""; ValueData: "Fanimate.Project"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.fnm"; ValueType: string; ValueName: "Content Type"; ValueData: "application/x-fanimate-project"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Fanimate.Project"; ValueType: string; ValueName: ""; ValueData: "Fanimate project"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Fanimate.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\fnm-file.ico"
Root: HKA; Subkey: "Software\Classes\Fanimate.Project\shell\open"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "Fanimate"
Root: HKA; Subkey: "Software\Classes\Fanimate.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\fanimate.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\fanimate.exe\SupportedTypes"; ValueType: string; ValueName: ".fnm"; ValueData: ""

[InstallDelete]
; 0.1.0 shipped FFmpeg; video export now uses Windows' own encoder or the user's FFmpeg
Type: filesandordirs; Name: "{app}\ffmpeg"

[Icons]
Name: "{group}\Fanimate"; Filename: "{app}\fanimate.exe"
Name: "{group}\Uninstall Fanimate"; Filename: "{uninstallexe}"
Name: "{userdesktop}\Fanimate"; Filename: "{app}\fanimate.exe"; Tasks: desktopicon

[Run]
; also after a silent update, so Fanimate comes back once updated
Filename: "{app}\fanimate.exe"; Description: "Start Fanimate"; Flags: nowait postinstall
