#define AppVersion "0.4.0"
#ifndef BundleDir
 #define BundleDir "..\dist\bundle"
#endif
#ifndef InstallerOutput
 #define InstallerOutput "..\dist\installer"
#endif
#ifndef DotNetRuntimeUrl
 #error Build with scripts/build-windows.ps1 to provide official runtime links
#endif
#ifndef WindowsRuntimeUrl
 #error Build with scripts/build-windows.ps1 to provide official runtime links
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
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimp"; MessagesFile: "ChineseSimplified.isl"
[CustomMessages]
english.DotNetDownload=Download .NET 10
english.WindowsRuntimeDownload=Windows Runtime
english.RuntimeHint=If first launch reports a missing runtime, use the download buttons below. They open Microsoft's official websites; nothing is installed automatically.
english.OpenUrlFailed=The download link could not be opened. Open RuntimeHelp.html in the application folder.
chinesesimp.DotNetDownload=下载 .NET 10
chinesesimp.WindowsRuntimeDownload=下载 Windows 运行库
chinesesimp.RuntimeHint=首次打开时如提示缺少运行库，可使用下方按钮前往微软官方下载。应用不会自动下载或安装依赖。
chinesesimp.OpenUrlFailed=无法打开下载链接。请打开程序目录中的 RuntimeHelp.html。
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut / 创建桌面快捷方式"; Flags: unchecked
[Files]
Source: "{#BundleDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[InstallDelete]
; Replace application-owned libraries as a set, including leftover runtime files.
; Pairing, encrypted state and history live outside the program directory.
Type: files; Name: "{app}\*.dll"
Type: files; Name: "{app}\*.winmd"
Type: files; Name: "{app}\*.pri"
Type: files; Name: "{app}\*.pdb"
Type: files; Name: "{app}\createdump.exe"
[Icons]
Name: "{autoprograms}\RustClipSync"; Filename: "{app}\RustClipSync.exe"; WorkingDir: "{app}"
Name: "{autoprograms}\RustClipSync 运行环境"; Filename: "{app}\RuntimeHelp.html"
Name: "{autodesktop}\RustClipSync"; Filename: "{app}\RustClipSync.exe"; WorkingDir: "{app}"; Tasks: desktopicon
[UninstallDelete]
Type: files; Name: "{userstartup}\RustClipSync.lnk"
[Run]
Filename: "{app}\RustClipSync.exe"; Description: "Open RustClipSync / 打开 RustClipSync"; Flags: nowait postinstall skipifsilent
[Code]
var
  DotNetButton, WindowsRuntimeButton: TNewButton;
procedure OpenRuntimeUrl(Url: String);
var
  ResultCode: Integer;
begin
  if not ShellExec('open', Url, '', '', SW_SHOWNORMAL, ewNoWait, ResultCode) then
    MsgBox(CustomMessage('OpenUrlFailed'), mbInformation, MB_OK);
end;
procedure DotNetClick(Sender: TObject);
begin
  OpenRuntimeUrl('{#DotNetRuntimeUrl}');
end;
procedure WindowsRuntimeClick(Sender: TObject);
begin
  OpenRuntimeUrl('{#WindowsRuntimeUrl}');
end;
procedure InitializeWizard();
begin
  DotNetButton := TNewButton.Create(WizardForm);
  DotNetButton.Parent := WizardForm;
  DotNetButton.SetBounds(ScaleX(8), WizardForm.NextButton.Top, ScaleX(112), WizardForm.NextButton.Height);
  DotNetButton.Caption := CustomMessage('DotNetDownload');
  DotNetButton.OnClick := @DotNetClick;
  DotNetButton.Visible := False;
  WindowsRuntimeButton := TNewButton.Create(WizardForm);
  WindowsRuntimeButton.Parent := WizardForm;
  WindowsRuntimeButton.SetBounds(ScaleX(128), WizardForm.NextButton.Top, ScaleX(148), WizardForm.NextButton.Height);
  WindowsRuntimeButton.Caption := CustomMessage('WindowsRuntimeDownload');
  WindowsRuntimeButton.OnClick := @WindowsRuntimeClick;
  WindowsRuntimeButton.Visible := False;
end;
procedure CurPageChanged(CurPageID: Integer);
begin
  DotNetButton.Visible := CurPageID = wpFinished;
  WindowsRuntimeButton.Visible := CurPageID = wpFinished;
  if CurPageID = wpFinished then
    WizardForm.FinishedLabel.Caption := WizardForm.FinishedLabel.Caption + #13#10#13#10 + CustomMessage('RuntimeHint');
end;
procedure CurStepChanged(CurStep: TSetupStep);
var
  Item: TFindRec;
  Directory: String;
begin
  if (CurStep = ssInstall) and FindFirst(ExpandConstant('{app}\*'), Item) then
  begin
    try
      repeat
        if ((Item.Attributes and FILE_ATTRIBUTE_DIRECTORY) <> 0) and
           ((Item.Attributes and $400) = 0) and
           (Item.Name <> '.') and (Item.Name <> '..') then
        begin
          Directory := ExpandConstant('{app}\') + Item.Name;
          if FileExists(Directory + '\Microsoft.ui.xaml.dll.mui') or FileExists(Directory + '\Microsoft.UI.Xaml.Phone.dll.mui') then
          begin
            DeleteFile(Directory + '\Microsoft.ui.xaml.dll.mui');
            DeleteFile(Directory + '\Microsoft.UI.Xaml.Phone.dll.mui');
            RemoveDir(Directory);
          end;
        end;
      until not FindNext(Item);
    finally
      FindClose(Item);
    end;
  end;
end;
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
