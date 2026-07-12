#define MyAppName "NovaType"
#define MyAppVersion "0.3.0"
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
; Build prerequisites:
;   cargo build --release -p novatype-server -p novatype-desktop -p novatype-tsf
;   cd apps\desktop && npm run build
;
; TSF DLL: registered during install via regserver flag, unregistered on uninstall
; via [UninstallRun] regsvr32 /u.
Source: "..\..\target\release\novatype-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\novatype-desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\novatype_tsf.dll"; DestDir: "{app}"; Flags: ignoreversion regserver

[Icons]
Name: "{group}\NovaType 设置"; Filename: "{app}\{#MyAppExeName}"
Name: "{autoprograms}\NovaType"; Filename: "{app}\{#MyAppExeName}"

[Run]
; Launch the daemon engine (auto-starts with Windows, but also launch now).
Filename: "{app}\novatype-server.exe"; Description: "启动 NovaType 输入法引擎"; \
    Flags: nowait postinstall skipifsilent
; Open settings (optional).
Filename: "{app}\{#MyAppExeName}"; Description: "打开 NovaType 设置"; \
    Flags: nowait postinstall skipifsilent skipifsilent

[UninstallRun]
; Unregister the TSF DLL before removing it.
Filename: "regsvr32"; Parameters: "/u /s ""{app}\novatype_tsf.dll"""; Flags: runhidden
; Kill the daemon so the file can be deleted.
Filename: "taskkill"; Parameters: "/IM novatype-server.exe /F"; Flags: runhidden skipifdoesntexist

[Registry]
; Autostart the daemon on login. TSF auto-spawn can also trigger it,
; but a persistent daemon avoids cold-start latency on first keystroke.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; \
    ValueType: string; ValueName: "NovaType"; ValueData: """{app}\novatype-server.exe"""; \
    Flags: uninsdeletevalue
