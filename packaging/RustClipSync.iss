#define AppVersion "0.3.0"
#ifndef BundleDir
 #define BundleDir "..\dist\bundle"
#endif
#ifndef InstallerOutput
 #define InstallerOutput "..\dist\installer"
#endif
[Setup]
AppId={{CF3EA0D9-4FE3-4394-BF35-761B243CAD74}
AppName=RustClipSync
AppVersion={#AppVersion}
AppPublisher=Infinifar
AppPublisherURL=https://github.com/Infinifar/rustclipsync-desktop
DefaultDirName={localappdata}\Programs\RustClipSync
DefaultGroupName=RustClipSync
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
OutputDir={#InstallerOutput}
OutputBaseFilename=RustClipSync-{#AppVersion}-setup-x64
SetupIconFile=..\assets\rustclipsync.ico
UninstallDisplayIcon={app}\RustClipSync.exe
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimp"; MessagesFile: "ChineseSimplified.isl"
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut / 创建桌面快捷方式"; Flags: unchecked
[Files]
Source: "{#BundleDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{autoprograms}\RustClipSync"; Filename: "{app}\RustClipSync.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\RustClipSync"; Filename: "{app}\RustClipSync.exe"; WorkingDir: "{app}"; Tasks: desktopicon
[UninstallDelete]
Type: files; Name: "{userstartup}\RustClipSync.lnk"
[Run]
Filename: "{app}\RustClipSync.exe"; Description: "Open RustClipSync / 打开 RustClipSync"; Flags: nowait postinstall skipifsilent
[Code]
function StopAgent(): Boolean;
var
  Window: HWND;
  Attempt: Integer;
begin
  Result := True;
  Window := FindWindowByClassName('RustClipSync.Agent.Window');
  if Window = 0 then exit;
  PostMessage(Window, $0010, 0, 0);
  for Attempt := 1 to 450 do
  begin
    if FindWindowByClassName('RustClipSync.Agent.Window') = 0 then exit;
    Sleep(100);
  end;
  Result := False;
end;
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not StopAgent() then Result := 'RustClipSync is still stopping. Exit it from the system tray and retry.';
end;
function InitializeUninstall(): Boolean;
begin
  Result := StopAgent();
  if not Result then MsgBox('RustClipSync is still stopping. Exit it from the system tray and retry.', mbError, MB_OK);
end;
