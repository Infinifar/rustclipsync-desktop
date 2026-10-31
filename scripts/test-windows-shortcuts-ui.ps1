if ($env:GITHUB_ACTIONS -ne 'true' -or [string]::IsNullOrEmpty($env:RUNNER_TEMP) -or [string]::IsNullOrEmpty($destination)) { throw 'Run only in the disposable installer test.' }
$isolatedPrefix = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
if (![IO.Path]::GetFullPath($env:LOCALAPPDATA).StartsWith($isolatedPrefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing non-fixture profile.' }
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class ShortcutInputTest {
 [StructLayout(LayoutKind.Sequential)] public struct Mouse { public int x,y;public uint data,flags,time;public UIntPtr extra; }
 [StructLayout(LayoutKind.Sequential)] public struct Keyboard { public ushort key,scan;public uint flags,time;public UIntPtr extra; }
 [StructLayout(LayoutKind.Explicit)] public struct Union { [FieldOffset(0)] public Mouse mouse;[FieldOffset(0)] public Keyboard keyboard; }
 [StructLayout(LayoutKind.Sequential)] public struct Input { public uint type; public Union value; }
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint n,Input[] input,int size);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
 public static void Press(ushort key) {
  ushort[] keys={0x11,0x12,0x10,key};var input=new Input[8];
  for(int i=0;i<4;i++) { input[i].type=1;input[i].value.keyboard.key=keys[i];input[4+i].type=1;input[4+i].value.keyboard.key=keys[3-i];input[4+i].value.keyboard.flags=2; }
  if(SendInput(8,input,Marshal.SizeOf<Input>())!=8) throw new Exception("SendInput failed");
 }
}
'@
function Find-ProductWindow([string]$title) {
    $matches = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$title))
    foreach ($candidate in $matches) {
        $process = Get-Process -Id $candidate.Current.ProcessId -ErrorAction SilentlyContinue
        if ($process -and $process.Path -eq (Join-Path $destination 'RustClipSync.exe')) { return $candidate }
    }
    return $null
}
function Wait-ProductWindow([string]$title,[bool]$present) {
    $end = (Get-Date).AddSeconds(20)
    do { $element = Find-ProductWindow $title; if (($null -ne $element) -eq $present) { return $element }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
    Save-Screen 'failed-shortcut'
    throw "Shortcut window state mismatch: $title, expected present=$present"
}
function Send-ProductShortcut([int]$key) {
    [ShortcutInputTest]::SetForegroundWindow([ShortcutInputTest]::GetShellWindow()) | Out-Null
    [ShortcutInputTest]::Press([ushort]$key)
    Start-Sleep -Milliseconds 400
}
$mainWindow = $script:window
$clipboardBeforeShortcuts = Get-Clipboard -Raw
Invoke-Control NavHome
$copy = (Get-Control CopyHomeSensitiveText).Current.BoundingRectangle
$connection = (Get-Control ConnectionSettings).Current.BoundingRectangle
$stop = (Get-Control StopBackground).Current.BoundingRectangle
if ($connection.Top -lt $copy.Bottom -or $stop.Top -lt $copy.Bottom) { throw 'Connection/quit commands were not placed below sensitive inbox' }
Invoke-Control NavShortcuts
(Get-Control HistoryShortcut).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Ctrl+Alt+Shift+F9')
(Get-Control InboxShortcut).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Ctrl+Alt+Shift+F10')
Invoke-Control SaveShortcuts
$end = (Get-Date).AddSeconds(15)
do { $stored = Get-Content "$data/config.json" -Raw | ConvertFrom-Json; if ($stored.shortcuts.history.key -eq 0x78 -and $stored.shortcuts.inbox.key -eq 0x79) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if ($stored.shortcuts.history.modifiers -ne 7 -or $stored.shortcuts.inbox.key -ne 0x79) { throw 'UI did not save shortcut preferences through Rust' }
Save-Screen 'shortcuts-settings'
# Invalid and duplicate combinations cannot replace the saved settings.
(Get-Control InboxShortcut).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Ctrl+Alt+Shift+F9')
Invoke-Control SaveShortcuts
if ((Get-Content "$data/config.json" -Raw | ConvertFrom-Json).shortcuts.inbox.key -ne 0x79) { throw 'Duplicate shortcut was accepted' }
(Get-Control InboxShortcut).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Shift+I')
Invoke-Control SaveShortcuts
if ((Get-Content "$data/config.json" -Raw | ConvertFrom-Json).shortcuts.inbox.key -ne 0x79) { throw 'Unsafe shortcut was accepted' }
Invoke-Control ReloadShortcuts
Invoke-Control NavHome
Send-ProductShortcut 0x78
$historyWindow = Wait-ProductWindow '剪贴板历史' $true
if ($historyWindow.Current.ProcessId -ne $ui.Id) { throw 'Warm shortcut created a duplicate UI process' }
$script:window = $historyWindow
Get-Control HistorySearch | Out-Null
Save-Screen 'history-shortcut-window'
Send-ProductShortcut 0x78
Wait-ProductWindow '剪贴板历史' $false | Out-Null
Send-ProductShortcut 0x79
$inboxWindow = Wait-ProductWindow '敏感收件区' $true
if ($inboxWindow.Current.ProcessId -ne $ui.Id) { throw 'Inbox shortcut created a duplicate UI process' }
$script:window = $inboxWindow
if ((Get-Control SensitiveWindowText).Current.Name -ne '••••••••') { throw 'Shortcut revealed a sensitive body automatically' }
Send-ProductShortcut 0x79
Wait-ProductWindow '敏感收件区' $false | Out-Null
$script:window = $mainWindow
if ((Get-Clipboard -Raw) -ne $clipboardBeforeShortcuts) { throw 'Opening/closing shortcut windows changed the clipboard' }
Write-Output 'PASS: shortcut settings, validation, real global input, warm activation, single UI process and repeat-to-close.'
