if ($env:GITHUB_ACTIONS -ne 'true' -or [string]::IsNullOrEmpty($env:RUNNER_TEMP) -or [string]::IsNullOrEmpty($destination)) { throw 'Run only from the isolated installer test.' }
$isolatedPrefix = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
if (![IO.Path]::GetFullPath($env:LOCALAPPDATA).StartsWith($isolatedPrefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing non-fixture profile.' }
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class InspectorWindowTest {
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls,string name);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
 [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtr(IntPtr window,int index);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window,int command);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window,uint message,IntPtr wp,IntPtr lp);
}
'@
Invoke-Control NavHome
$mainWindow = $script:window
$mainHandle = [IntPtr]$mainWindow.Current.NativeWindowHandle
Invoke-Control OpenSensitiveWindow
$end = (Get-Date).AddSeconds(15)
do { $inspectorHandle = [InspectorWindowTest]::FindWindow($null,'敏感收件区'); if ($inspectorHandle -ne [IntPtr]::Zero) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if ($inspectorHandle -eq [IntPtr]::Zero) { throw 'Sensitive inspector did not open' }
[uint32]$inspectorPid = 0
[InspectorWindowTest]::GetWindowThreadProcessId($inspectorHandle,[ref]$inspectorPid) | Out-Null
if ($inspectorPid -ne $ui.Id -or ![InspectorWindowTest]::IsIconic($mainHandle)) { throw 'Inspector is not separate from the automatically minimized main window' }
$script:window = [System.Windows.Automation.AutomationElement]::FromHandle($inspectorHandle)
$end = (Get-Date).AddSeconds(15)
do { if ((Get-Control RevealSensitiveText).Current.IsEnabled) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if (!(Get-Control RevealSensitiveText).Current.IsEnabled) { throw 'Inspector could not read inbox metadata' }
if ((Get-Control SensitiveWindowText).Current.Name -ne '••••••••') { throw 'Inspector did not start masked' }
if ((Get-Control SensitiveReceivedTime).Current.Name -notlike '收到时间：*') { throw 'Inspector receipt time missing' }
(Get-Control PinSensitiveWindow).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
Start-Sleep -Milliseconds 200
if (([InspectorWindowTest]::GetWindowLongPtr($inspectorHandle,-20).ToInt64() -band 8) -eq 0) { throw 'Pin did not set actual WS_EX_TOPMOST' }
Save-Screen 'sensitive-small-window'
Invoke-Control RevealSensitiveText
if ((Get-Control SensitiveWindowText).Current.Name -ne 'UI private fixture') { throw 'Explicit reveal did not read the actual inbox body' }
Invoke-Control RevealSensitiveText
if ((Get-Control SensitiveWindowText).Current.Name -ne '••••••••') { throw 'Hide did not remove the plaintext from UI' }
Invoke-Control CopySensitiveText
$confirm = $script:window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'复制'))
if (!$confirm) { throw 'Sensitive copy confirmation missing' }
$confirm.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
$end = (Get-Date).AddSeconds(15)
do { if ((Get-Clipboard -Raw) -eq 'UI private fixture') { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if ((Get-Clipboard -Raw) -ne 'UI private fixture') { throw 'Small window did not copy actual encrypted inbox body to clipboard' }
(Get-Control PinSensitiveWindow).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
Start-Sleep -Milliseconds 200
if (([InspectorWindowTest]::GetWindowLongPtr($inspectorHandle,-20).ToInt64() -band 8) -ne 0) { throw 'Unpin did not clear WS_EX_TOPMOST' }
# Reopening from the main window must reuse the inspector, not create duplicates.
[InspectorWindowTest]::ShowWindow($mainHandle,9) | Out-Null
$script:window = $mainWindow
Invoke-Control OpenSensitiveWindow
if ([InspectorWindowTest]::FindWindow($null,'敏感收件区') -ne $inspectorHandle -or ![InspectorWindowTest]::IsIconic($mainHandle)) { throw 'Repeated detach did not reuse inspector/minimize main' }
$script:window = [System.Windows.Automation.AutomationElement]::FromHandle($inspectorHandle)
Invoke-Control DeleteSensitiveText
$end = (Get-Date).AddSeconds(15)
do { if (!(Get-Control RevealSensitiveText).Current.IsEnabled) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if ((Get-Control RevealSensitiveText).Current.IsEnabled -or (Get-Control SensitiveWindowText).Current.Name -ne '••••••••') { throw 'Deleted inbox remains readable in inspector' }
$request = @{id=[Guid]::NewGuid().ToString();ipcVersion=1;method='getStatus';params=@{}} | ConvertTo-Json -Compress
$snapshot = $request | & "$destination/rustclipsync-agent.exe" rpc | ConvertFrom-Json
if ($null -ne $snapshot.result.inbox -or $snapshot.result.cursor -ne 42) { throw 'Inspector delete did not clear only the inbox' }
[InspectorWindowTest]::PostMessage($inspectorHandle,0x0010,[IntPtr]::Zero,[IntPtr]::Zero) | Out-Null
Start-Sleep -Milliseconds 400
if ($ui.HasExited) { throw 'Closing inspector stopped the main UI process' }
[InspectorWindowTest]::ShowWindow($mainHandle,9) | Out-Null
$script:window = $mainWindow
Write-Output 'PASS: sensitive inspector, minimized main, default masking, explicit reveal/hide, real pin/unpin, copy, delete and window reuse.'
