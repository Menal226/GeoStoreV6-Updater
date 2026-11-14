; Build: ISCC.exe installer\GeoStoreV6Updater.iss [/DSourceExe=<path to exe>] [/DAppVersion=<x.y.z>]

#define AppName "GeoStoreV6 Updater"
#define AppExe "GeoStoreUpdater.exe"
#define TrayClass "GeoStoreUpdaterTray"
#define TaskName "GeoStore Updater"
#ifndef SourceExe
  #define SourceExe AddBackslash(SourcePath) + "..\target\release\" + AppExe
#endif
#ifndef AppVersion
  #define AppVersion GetVersionNumbersString(SourceExe)
#endif

[Setup]
AppId={{3FAE16EC-6CDB-4514-BF8A-DA97D9B7AA25}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Menal
AppPublisherURL=https://github.com/Menal226/GeoStoreV6-Updater
AppSupportURL=https://github.com/Menal226/GeoStoreV6-Updater
DefaultDirName={autopf}\GeoStore Updater
DisableProgramGroupPage=yes
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExe}
SetupIconFile=..\assets\icon.ico
OutputDir=..\target\installer
OutputBaseFilename=GeoStoreV6-Updater-Setup
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=force
RestartApplications=no
ShowLanguageDialog=no
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} Setup

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "cs"; MessagesFile: "compiler:Languages\Czech.isl"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"

[Run]
Filename: "{sys}\schtasks.exe"; Parameters: "/Run /TN ""{#TaskName}"""; Flags: runhidden; Check: TrayWasRunning
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\{#AppExe}"; Parameters: "--uninstall"; Flags: runhidden waituntilterminated; RunOnceId: "Cleanup"

[UninstallDelete]
Type: files; Name: "{app}\GeoStoreUpdater.old.exe"

[Code]
var
  TrayRunning: Boolean;

procedure CloseWindow(Wnd: HWND);
begin
  if Wnd <> 0 then
    PostMessage(Wnd, 16, 0, 0);
end;

{ An update closes the running app and tray so the exe can be replaced; the tray is started again afterwards. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  I: Integer;
begin
  TrayRunning := FindWindowByClassName('{#TrayClass}') <> 0;
  CloseWindow(FindWindowByClassName('{#TrayClass}'));
  CloseWindow(FindWindowByWindowName('{#AppName}'));
  for I := 1 to 50 do
  begin
    if (FindWindowByClassName('{#TrayClass}') = 0) and (FindWindowByWindowName('{#AppName}') = 0) then
      Break;
    Sleep(100);
  end;
  Result := '';
end;

function TrayWasRunning: Boolean;
begin
  Result := TrayRunning;
end;
