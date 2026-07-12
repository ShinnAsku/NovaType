#define MyAppName "NovaType"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "NovaType"
#define MyAppExeName "novatype-desktop.exe"

[Setup]
AppId={{A7AC51BB-177D-4F64-9A77-6B1999F9D07A}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\NovaType
DefaultGroupName=NovaType
DisableProgramGroupPage=yes
OutputDir=dist
OutputBaseFilename=NovaTypeSetup-{#MyAppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64
PrivilegesRequired=admin
WizardStyle=modern

[Files]
; Build these first:
;   cargo build --release -p novatype-server -p novatype-desktop -p novatype-tsf
;   cd apps\desktop && npm run build
Source: "..\..\target\release\novatype-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\novatype-desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\novatype_tsf.dll"; DestDir: "{app}"; Flags: ignoreversion regserver

[Icons]
Name: "{group}\NovaType 设置"; Filename: "{app}\{#MyAppExeName}"
Name: "{autoprograms}\NovaType"; Filename: "{app}\{#MyAppExeName}"

[Run]
Filename: "{app}\novatype-server.exe"; Description: "启动 NovaType 输入引擎"; Flags: nowait postinstall skipifsilent
Filename: "{app}\{#MyAppExeName}"; Description: "打开 NovaType 设置"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "regsvr32"; Parameters: "/u /s ""{app}\novatype_tsf.dll"""; Flags: runhidden
Filename: "taskkill"; Parameters: "/IM novatype-server.exe /F"; Flags: runhidden skipifdoesntexist

[Registry]
; User-facing autostart for the daemon. TSF layer can also auto-spawn it.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "NovaType"; ValueData: """{app}\novatype-server.exe"""; Flags: uninsdeletevalue
