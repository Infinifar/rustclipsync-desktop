# Run only on an ephemeral GitHub-hosted Windows desktop.
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or [string]::IsNullOrEmpty($env:RUNNER_TEMP)) { throw 'Run dependency tests only on a disposable GitHub Windows runner.' }
$root = Split-Path $PSScriptRoot -Parent
$bundle = Join-Path $root 'dist/bundle'
$runtimes = Get-Content "$root/packaging/windows-runtimes.json" -Raw | ConvertFrom-Json
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class DependencyWindow { [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window); }
'@
$results = [ordered]@{}
function Test-MissingRuntimeDialog([string]$Name, [string]$Expected, [hashtable]$EnvironmentOverrides) {
    $start = [Diagnostics.ProcessStartInfo]::new((Join-Path $bundle 'RustClipSync.exe'))
    $start.UseShellExecute = $false
    $start.Environment['LOCALAPPDATA'] = Join-Path $env:RUNNER_TEMP 'RustClipSync-dependency-test-data'
    foreach ($key in $EnvironmentOverrides.Keys) { $start.Environment[$key] = $EnvironmentOverrides[$key] }
    $process = [Diagnostics.Process]::Start($start)
    try {
        $condition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, $process.Id)
        $deadline = (Get-Date).AddSeconds(25)
        $found = $false
        do {
            Start-Sleep -Milliseconds 250
            $process.Refresh()
            if ($process.HasExited) { throw "$Name exited without displaying the missing-runtime prompt: $($process.ExitCode)" }
            $windows = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, $condition)
            foreach ($window in $windows) {
                $elements = $window.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
                $text = $window.Current.Name + "`n" + (($elements | ForEach-Object { $_.Current.Name }) -join "`n")
                $buttons = @($elements | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button })
                if ($text -notmatch $Expected -or !($buttons | Where-Object { $_.Current.Name -match 'Download|Install|Yes|下载|安装|是' })) { continue }
                [IO.File]::WriteAllText("$root/dist/installer/missing-$Name.txt", $text)
                $rect = $window.Current.BoundingRectangle
                if ($rect.Width -gt 0 -and $rect.Height -gt 0) {
                    [DependencyWindow]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
                    Start-Sleep -Milliseconds 200
                    $bitmap = [Drawing.Bitmap]::new([int]$rect.Width, [int]$rect.Height)
                    $graphics = [Drawing.Graphics]::FromImage($bitmap)
                    $graphics.CopyFromScreen([int]$rect.X,[int]$rect.Y,0,0,$bitmap.Size)
                    $bitmap.Save("$root/dist/installer/missing-$Name.png",[Drawing.Imaging.ImageFormat]::Png)
                    $graphics.Dispose(); $bitmap.Dispose()
                }
                # The SDK's Yes/No dialog disables its title-bar close action.
                # Cancel explicitly so this test never opens a download browser.
                $cancel = $buttons | Where-Object { $_.Current.Name.Replace('&','') -match '^(No|Cancel|否.*|取消.*)$' } | Select-Object -First 1
                if (!$cancel) { throw "$Name prompt does not provide a cancellation button" }
                $cancel.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
                $found = $true; break
            }
        } while (!$found -and (Get-Date) -lt $deadline)
        if (!$found) { throw "$Name did not display a download prompt" }
        if (!$process.WaitForExit(10000)) { throw "$Name did not exit after cancelling the prompt" }
        if ($process.ExitCode -eq 0) { throw "$Name unexpectedly reported a successful launch without its runtime" }
        Write-Output "PASS: missing $Name shows a native download prompt before application startup."
    } finally {
        if (!$process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
}
# Override discovery only for this child process. Never uninstall .NET from the runner.
$emptyDotNet = Join-Path $env:RUNNER_TEMP 'RustClipSync-empty-dotnet'
New-Item -ItemType Directory -Force $emptyDotNet | Out-Null
Test-MissingRuntimeDialog 'dotnet' '\.NET' @{ DOTNET_ROOT_X64 = $emptyDotNet; DOTNET_ROOT = $emptyDotNet }
$results.missingDotnet = 'passed'
# Hosted images normally have older runtimes. Do not remove another application's
# shared framework just to create a negative test if a 2.x runtime is preinstalled.
$installed = @(Get-AppxPackage -Name 'Microsoft.WindowsAppRuntime.2*')
if ($installed.Count -eq 0) {
    Test-MissingRuntimeDialog 'windows-runtime' 'Windows App Runtime|Windows App SDK' @{}
    $results.missingWindowsRuntime = 'passed'
} else {
    $results.missingWindowsRuntime = 'skipped: shared 2.x runtime already installed'
    Write-Output "SKIP: missing Windows Runtime dialog; a shared 2.x runtime is already installed."
}
$download = Join-Path $env:RUNNER_TEMP 'RustClipSync-WindowsAppRuntimeInstall-x64.exe'
Invoke-WebRequest -Uri $runtimes.windowsAppRuntime.downloadUrl -OutFile $download
$signature = Get-AuthenticodeSignature $download
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(?:^|, )CN=Microsoft Corporation(?:,|$)') { throw 'The runtime installer must have a valid Microsoft signature' }
$install = Start-Process $download -ArgumentList '--quiet' -Wait -PassThru
if ($install.ExitCode -notin @(0,3010)) { throw "Official Windows Runtime installation failed: $($install.ExitCode)" }
$results.windowsRuntimeInstaller = 'Microsoft signature verified; installed from official URL'
$results | ConvertTo-Json | Set-Content "$root/dist/installer/dependency-checks.json" -Encoding utf8
Get-AppxPackage -Name '*WindowsAppRuntime*' | Select-Object Name,Version,Architecture | Format-Table
Write-Output 'PASS: official shared Windows Runtime installed; continue with full WinUI and installer tests.'
