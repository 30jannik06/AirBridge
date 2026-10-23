; Inno Setup 6 – Installer für AirBridge.
; Bauen: iscc /DAppVersion=0.1.0 installer\airbridge.iss   (Ergebnis: dist\AirBridge-Setup-0.1.0.exe)
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{6B2F4D7E-3C1A-4E58-9B0D-A17E5C2F8D41}
AppName=AirBridge
AppVersion={#AppVersion}
AppPublisher=30jannik06
AppPublisherURL=https://github.com/30jannik06/AirBridge
AppSupportURL=https://github.com/30jannik06/AirBridge/issues
DefaultDirName={autopf}\AirBridge
DefaultGroupName=AirBridge
UninstallDisplayIcon={app}\airbridge.exe
OutputDir=..\dist
OutputBaseFilename=AirBridge-Setup-{#AppVersion}
LicenseFile=..\LICENSE
SetupIconFile=..\assets\airbridge.ico
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
; A running AirBridge locks its exe: Setup finds it via the mutex and closes it before replacing the files.
AppMutex=AirBridgeSingleInstance
CloseApplications=yes
CloseApplicationsFilter=airbridge.exe
RestartApplications=no
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked
Name: "firewall"; Description: "Firewall-Regel anlegen (eingehend, nur private Netzwerke) / Create firewall rule"

[Files]
Source: "..\target\release\airbridge.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.de.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{group}\AirBridge"; Filename: "{app}\airbridge.exe"
Name: "{group}\{cm:UninstallProgram,AirBridge}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\AirBridge"; Filename: "{app}\airbridge.exe"; Tasks: desktopicon

[Run]
Filename: "netsh"; Parameters: "advfirewall firewall delete rule name=""AirBridge"""; Flags: runhidden; Tasks: firewall
Filename: "netsh"; Parameters: "advfirewall firewall add rule name=""AirBridge"" dir=in action=allow program=""{app}\airbridge.exe"" enable=yes profile=private,domain"; Flags: runhidden; Tasks: firewall
Filename: "{app}\airbridge.exe"; Description: "{cm:LaunchProgram,AirBridge}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "reg"; Parameters: "delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v AirBridge /f"; Flags: runhidden; RunOnceId: "RemoveAutostart"
Filename: "netsh"; Parameters: "advfirewall firewall delete rule name=""AirBridge"""; Flags: runhidden; RunOnceId: "RemoveFirewallRule"
