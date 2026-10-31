# Exercise actual WinUI controls on a disposable runner.
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
function Get-Control([string]$id) {
    $end = (Get-Date).AddSeconds(10)
    do {
        $element = $script:window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$id))
        if ($element) { return $element }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $end)
    throw "UI control missing: $id"
}
function Invoke-Control([string]$id) {
    $element = Get-Control $id
    $pattern = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$pattern)) { $pattern.Select() }
    elseif ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) { $pattern.Invoke() }
    else { throw "UI control cannot be selected: $id" }
    Start-Sleep -Milliseconds 400
}
function Select-Combo([string]$id,[string]$label) {
    $combo = Get-Control $id
    $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Start-Sleep -Milliseconds 250
    $item = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$label),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ListItem)))
    if (!$item) { throw "Combo item missing: $label" }
    $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Collapse()
    Start-Sleep -Milliseconds 400
}
function Save-Screen([string]$name) {
    $image = New-Object Drawing.Bitmap([ScreenSize]::GetSystemMetrics(0),[ScreenSize]::GetSystemMetrics(1))
    $drawing = [Drawing.Graphics]::FromImage($image)
    $drawing.CopyFromScreen(0,0,0,0,$image.Size)
    $image.Save("$root/dist/installer/$name.png",[Drawing.Imaging.ImageFormat]::Png)
    $drawing.Dispose(); $image.Dispose()
}
$script:window = [System.Windows.Automation.AutomationElement]::FromHandle($ui.MainWindowHandle)
Invoke-Control NavAppearance
Select-Combo ThemeChoice '深色'
Select-Combo FontChoice '大'
$appearance = Get-Content "$env:LOCALAPPDATA/RustClipSync/appearance.json" -Raw | ConvertFrom-Json
if ($appearance.theme -ne 'dark' -or $appearance.fontSize -ne 18) { throw 'Appearance choice was not saved' }
Save-Screen 'appearance-dark'
Invoke-Control NavSettings
$startup = Join-Path ([Environment]::GetFolderPath('Startup')) 'RustClipSync.lnk'
if (Test-Path $startup) { throw 'Unexpected existing startup entry in disposable runner' }
$toggle = (Get-Control StartAtLogin).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
$toggle.Toggle(); Start-Sleep -Milliseconds 600
if (!(Test-Path $startup)) { throw 'UI did not create the actual startup shortcut' }
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($startup)
if ($link.TargetPath -ne (Join-Path $destination 'rustclipsync-agent.exe') -or $link.Arguments -ne 'autostart') { throw 'Incorrect startup target/arguments' }
[Runtime.InteropServices.Marshal]::FinalReleaseComObject($link) | Out-Null
[Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell) | Out-Null
Save-Screen 'settings-startup'
$toggle.Toggle(); Start-Sleep -Milliseconds 400
if (Test-Path $startup) { throw 'UI did not remove startup shortcut when disabled' }
Invoke-Control NavHome
Save-Screen 'home-dark'
foreach ($id in @('PushNow','PullNow','PauseSync','SyncDirection')) {
    $control = Get-Control $id
    $bounds = $control.Current.BoundingRectangle
    $container = $script:window.Current.BoundingRectangle
    Write-Output "Layout $id : left=$($bounds.Left) right=$($bounds.Right) width=$($bounds.Width) offscreen=$($control.Current.IsOffscreen)"
    if ($bounds.Width -le 0 -or $control.Current.IsOffscreen) { continue }
    if ($bounds.Right -gt ($container.Right-4) -or $bounds.Left -lt ($container.Left+4)) { throw "Control extends beyond the native window: $id" }
}
Save-Screen 'home-dark'
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class ResizeTestWindow { [DllImport("user32.dll")] [return:MarshalAs(UnmanagedType.Bool)] public static extern bool SetWindowPos(IntPtr window,IntPtr after,int x,int y,int width,int height,uint flags); }
'@
$original = $script:window.Current.BoundingRectangle
if (![ResizeTestWindow]::SetWindowPos($ui.MainWindowHandle,[IntPtr]::Zero,0,0,520,700,6)) { throw 'Cannot resize test window' }
Start-Sleep -Milliseconds 600
foreach ($id in @('PushNow','PullNow','PauseSync')) {
    $bounds = (Get-Control $id).Current.BoundingRectangle
    $container = $script:window.Current.BoundingRectangle
    if ($bounds.Width -gt 0 -and ($bounds.Right -gt ($container.Right-4) -or $bounds.Left -lt ($container.Left+4))) { throw "Narrow layout overflow: $id" }
}
Save-Screen 'home-narrow'
[ResizeTestWindow]::SetWindowPos($ui.MainWindowHandle,[IntPtr]::Zero,0,0,[int]$original.Width,[int]$original.Height,6) | Out-Null
Start-Sleep -Milliseconds 300
Write-Output 'PASS: Home/Appearance/Settings navigation, persistent theme/font and actual startup shortcut toggle.'
