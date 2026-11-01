if ($env:GITHUB_ACTIONS -ne 'true' -or [string]::IsNullOrEmpty($env:RUNNER_TEMP) -or [string]::IsNullOrEmpty($destination)) { throw 'Run only from the disposable installer test.' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class InstallerInput {
    [StructLayout(LayoutKind.Sequential)] struct Mouse { public int x,y; public uint data,flags,time; public UIntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct Union { [FieldOffset(0)] public Mouse mouse; }
    [StructLayout(LayoutKind.Sequential)] struct Input { public uint type; public Union value; }
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
    [DllImport("user32.dll")] static extern uint SendInput(uint count,Input[] input,int size);
    public static void Click(int x,int y) {
        if(!SetCursorPos(x,y)) throw new Exception("Cannot position installer test input");
        var input=new[] {new Input {value=new Union {mouse=new Mouse {flags=2}}},new Input {value=new Union {mouse=new Mouse {flags=4}}}};
        if(SendInput(2,input,Marshal.SizeOf<Input>())!=2) throw new Exception("Cannot send installer test input");
    }
}
'@
function Invoke-InstallerControl($Control) {
    $rect = $Control.Current.BoundingRectangle
    if ($rect.Width -le 0 -or $rect.Height -le 0) { throw 'Installer control has no visible hit target' }
    [InstallerInput]::Click([int]($rect.X + $rect.Width/2),[int]($rect.Y + $rect.Height/2))
}
$wizard = Start-Process $setup.FullName -ArgumentList @('/SP-','/SUPPRESSMSGBOXES','/NORESTART','/TASKS=','/LANG=chinesesimp',"/DIR=`"$destination`"") -PassThru
try {
    $deadline = (Get-Date).AddSeconds(90)
    $verified = $false
    do {
        Start-Sleep -Milliseconds 300
        if ($wizard.HasExited) { throw 'Interactive setup exited before its download buttons were verified' }
        $owners = [Collections.Generic.HashSet[int]]::new()
        $owners.Add($wizard.Id) | Out-Null
        $processTree = @(Get-CimInstance Win32_Process)
        do {
            $added = $false
            foreach ($child in $processTree) {
                if ($owners.Contains([int]$child.ParentProcessId) -and $owners.Add([int]$child.ProcessId)) { $added = $true }
            }
        } while ($added)
        $windows = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition)
        foreach ($window in $windows) {
            if (!$owners.Contains($window.Current.ProcessId) -or $window.Current.ClassName -ne 'TWizardForm') { continue }
            [InstallerInput]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
            $elements = $window.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
            # Inno's owner-drawn controls are exposed as UIA Pane, not Button /
            # RadioButton. Locate their accessible names and use real input.
            $buttons = @($elements | Where-Object { $_.Current.IsEnabled -and !$_.Current.IsOffscreen })
            $finish = $buttons | Where-Object { $_.Current.Name.Replace('&','') -match '^完成(?:\([A-Z]\))?$|^Finish$' } | Select-Object -First 1
            if ($finish) {
                $net = $buttons | Where-Object { $_.Current.Name -eq '下载 .NET 10' } | Select-Object -First 1
                $sdk = $buttons | Where-Object { $_.Current.Name -eq '下载 Windows 运行库' } | Select-Object -First 1
                if (!$net -or !$sdk) { throw 'The completed installer is missing official download buttons' }
                if ($net.Current.BoundingRectangle.Right -gt $sdk.Current.BoundingRectangle.Left -or $sdk.Current.BoundingRectangle.Right -gt $finish.Current.BoundingRectangle.Left) { throw 'Dependency buttons overlap the finish button' }
                $text = ($elements | ForEach-Object { $_.Current.Name }) -join "`n"
                if ($text -notmatch '微软官方下载') { throw 'The completed installer does not explain when the downloads are needed' }
                $rect = $window.Current.BoundingRectangle
                $bitmap = [Drawing.Bitmap]::new([int]$rect.Width,[int]$rect.Height)
                $graphics = [Drawing.Graphics]::FromImage($bitmap)
                $graphics.CopyFromScreen([int]$rect.X,[int]$rect.Y,0,0,$bitmap.Size)
                $bitmap.Save("$root/dist/installer/runtime-download-buttons.png",[Drawing.Imaging.ImageFormat]::Png)
                $graphics.Dispose(); $bitmap.Dispose()
                # Leave the default post-install choice intact; close a newly
                # launched first-run UI below using only this test installation.
                Invoke-InstallerControl $finish
                $verified = $true; break
            }
            $accept = $buttons | Where-Object { $_.Current.Name.Replace('&','') -match '^我(?:接受|同意)|^I accept' } | Select-Object -First 1
            if ($accept) { Invoke-InstallerControl $accept }
            $next = $buttons | Where-Object { $_.Current.Name.Replace('&','') -match '^(?:下一步|安装)(?:\([A-Z]\))?\s*>?$|^Next\b|^Install$' } | Select-Object -First 1
            if ($next) { Invoke-InstallerControl $next }
        }
    } while (!$verified -and (Get-Date) -lt $deadline)
    if (!$verified -or !$wizard.WaitForExit(10000) -or $wizard.ExitCode -ne 0) { throw 'Interactive installer verification did not complete' }
    Start-Sleep -Seconds 3
    foreach ($opened in @(Get-Process -Name RustClipSync -ErrorAction SilentlyContinue | Where-Object Path -eq (Join-Path $destination 'RustClipSync.exe'))) {
        $opened.CloseMainWindow() | Out-Null
        if (!$opened.WaitForExit(10000)) { throw 'The post-install UI did not close' }
    }
    Write-Output 'PASS: actual installer finish page shows two non-overlapping official download buttons and dependency guidance.'
} finally {
    if (!$verified) {
        # Preserve the actual wizard and control names when a hosted desktop's
        # language or window ownership differs from the expected image.
        try {
            $snapshot = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition)
            $diagnostic = foreach ($window in $snapshot) {
                "WINDOW $($window.Current.ProcessId) $($window.Current.ClassName) $($window.Current.Name)"
                if ($window.Current.ClassName -eq 'TWizardForm') {
                    $window.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) | ForEach-Object { "  $($_.Current.ControlType.ProgrammaticName) enabled=$($_.Current.IsEnabled) $($_.Current.Name)" }
                }
            }
            $diagnostic | Set-Content "$root/dist/installer/installer-help-controls.txt" -Encoding utf8
            $diagnostic | Write-Output
            $bitmap = [Drawing.Bitmap]::new([ScreenSize]::GetSystemMetrics(0),[ScreenSize]::GetSystemMetrics(1))
            $graphics = [Drawing.Graphics]::FromImage($bitmap)
            $graphics.CopyFromScreen(0,0,0,0,$bitmap.Size)
            $bitmap.Save("$root/dist/installer/installer-help-failure.png",[Drawing.Imaging.ImageFormat]::Png)
            $graphics.Dispose(); $bitmap.Dispose()
        } catch { Write-Output "Unable to capture installer diagnostics: $_" }
    }
    if (!$wizard.HasExited) { $wizard.Kill(); $wizard.WaitForExit() }
    $wizard.Dispose()
}
