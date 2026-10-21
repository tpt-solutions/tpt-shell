; SPDX-License-Identifier: MIT OR Apache-2.0
; Copyright (c) 2026 TPT Solutions
;
; Inno Setup 6 script for the Windows installer (todo O2: external installer tool). Built by
; tools/bundle-windows.sh --installer, which passes every define below on the ISCC command line.
; Not built or tested in this change: no installer has been produced yet.
;
; Defines (all passed by the script):
;   AppName, AppBinary, AppVersion, AppPublisher, AppId (a GUID, fixed for the life of the app),
;   SourceDir (the portable folder), OutputDir

#ifndef AppName
  #error "pass /DAppName=... (see tools/bundle-windows.sh)"
#endif
#ifndef AppId
  #error "pass /DAppId=<GUID>; generate it once per app and never change it"
#endif

[Setup]
; The double brace escapes the GUID brace for Inno Setup.
AppId={{{#AppId}}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename={#AppBinary}-{#AppVersion}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
UninstallDisplayIcon={app}\{#AppBinary}.exe
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppBinary}.exe"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppBinary}.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppBinary}.exe"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent
