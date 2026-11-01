# Run only on an ephemeral GitHub-hosted Windows desktop.
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or [string]::IsNullOrEmpty($env:RUNNER_TEMP)) { throw 'Run this installation test only on a disposable GitHub Windows runner.' }
$root = Split-Path $PSScriptRoot -Parent
$setup = Get-Item "$root/dist/installer/RustClipSync-*-setup-x64.exe"
$destination = Join-Path $env:RUNNER_TEMP 'RustClipSync-installed'
$log = Join-Path $env:RUNNER_TEMP 'RustClipSync-install.log'
# Seed application-owned runtime leftovers to exercise replacement during upgrade.
New-Item -ItemType Directory -Force "$destination/zh-CN" | Out-Null
Set-Content "$destination/coreclr.dll" 'old runtime fixture'
Set-Content "$destination/zh-CN/Microsoft.ui.xaml.dll.mui" 'old language fixture'
$process = Start-Process $setup.FullName -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/TASKS=',"/DIR=`"$destination`"","/LOG=`"$log`"") -Wait -PassThru
if ($process.ExitCode -ne 0) { Get-Content $log -Tail 80; throw "Install failed: $($process.ExitCode)" }
if (!(Test-Path "$destination/RustClipSync.exe") -or !(Test-Path "$destination/rustclipsync-agent.exe")) { throw 'Installed executables are missing' }
if ((Test-Path "$destination/coreclr.dll") -or (Test-Path "$destination/zh-CN/Microsoft.ui.xaml.dll.mui")) { throw 'Old app-local runtime files were left behind' }
$help = Get-Content "$destination/RuntimeHelp.html" -Raw
$runtimes = Get-Content "$root/packaging/windows-runtimes.json" -Raw | ConvertFrom-Json
if (!$help.Contains($runtimes.dotnet.downloadUrl) -or !$help.Contains($runtimes.windowsAppRuntime.downloadUrl) -or $help.Contains('@@')) { throw 'Installed dependency help is incomplete' }
# Check icons embedded in the actual delivered files, not only loose assets.
Add-Type -AssemblyName System.Drawing
$reference = [Drawing.Icon]::new("$root/assets/rustclipsync.ico",32,32)
$expected = $reference.ToBitmap()
foreach ($name in @('RustClipSync.exe','rustclipsync-agent.exe')) {
    $icon = [Drawing.Icon]::ExtractAssociatedIcon((Join-Path $destination $name))
    if (!$icon) { throw "No icon: $name" }
    $actual = $icon.ToBitmap()
    if ($actual.Size -ne $expected.Size) { throw "Unexpected icon dimensions: $name" }
    $different = 0
    for ($y=0;$y -lt $expected.Height;$y++) {
        for ($x=0;$x -lt $expected.Width;$x++) {
            if ($actual.GetPixel($x,$y).ToArgb() -ne $expected.GetPixel($x,$y).ToArgb()) { $different++ }
        }
    }
    $actual.Dispose(); $icon.Dispose()
    if ($different -ne 0) { throw "Embedded icon differs from Omaclipsync artwork: $name ($different pixels)" }
}
$expected.Dispose(); $reference.Dispose()
# Use a private app-data root so the setup window is guaranteed to be first-run.
$originalData = $env:LOCALAPPDATA
$originalMetrics = $env:RCS_UI_METRICS_FILE
$metricsPath = Join-Path $env:RUNNER_TEMP 'RustClipSync-ui-runtime-events.jsonl'
if (Test-Path $metricsPath) { Remove-Item $metricsPath }
$env:RCS_UI_METRICS_FILE = $metricsPath
$env:LOCALAPPDATA = Join-Path $env:RUNNER_TEMP 'RustClipSync-first-run-data'
New-Item -ItemType Directory -Force $env:LOCALAPPDATA | Out-Null
try {
    # Exercise the originally broken entry path: double-clicking the agent with
    # no arguments must now open the WinUI setup window next to it.
    $launcher = Start-Process "$destination/rustclipsync-agent.exe" -PassThru
    $ui = $null
    for ($i=0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 500
        $ui = Get-Process -Name RustClipSync -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $destination 'RustClipSync.exe') } | Select-Object -First 1
        if ($ui) { $ui.Refresh(); if ($ui.MainWindowHandle -ne 0 -and $ui.MainWindowTitle -eq 'RustClipSync') { break } }
    }
    if (!$ui -or $ui.MainWindowHandle -eq 0 -or $ui.MainWindowTitle -ne 'RustClipSync' -or (Test-Path "$destination/startup-error.log")) {
        $launcher.Refresh()
        if ($launcher.HasExited) { Write-Output "Launcher exit: $($launcher.ExitCode)" }
        Get-Process -Name RustClipSync -ErrorAction SilentlyContinue | Select-Object Id,MainWindowTitle,MainWindowHandle,Path | Format-List
        Get-WinEvent -FilterHashtable @{LogName='Application';StartTime=(Get-Date).AddMinutes(-15)} -ErrorAction SilentlyContinue |
            Where-Object { $_.ProviderName -in @('.NET Runtime','Application Error','SideBySide','Windows Error Reporting') -and $_.Message -match 'RustClipSync|Microsoft.ui.Xaml|WindowsAppRuntime' } |
            Select-Object -First 10 TimeCreated,ProviderName,Message | Format-List
        if (Test-Path "$destination/startup-error.log") { Get-Content "$destination/startup-error.log"; Copy-Item "$destination/startup-error.log" "$root/dist/installer/startup-error.log" }
        Get-ChildItem "$destination/Assets" -ErrorAction SilentlyContinue | Select-Object Name,Length | Format-Table
        Get-ChildItem $destination | Select-Object Name,Length | Format-Table
        Copy-Item $log "$root/dist/installer/install.log" -ErrorAction SilentlyContinue
        throw 'Double-click did not create a visible WinUI window'
    }
    Start-Sleep -Seconds 2
    $ui.Refresh()
    if ($ui.HasExited -or (Test-Path "$destination/startup-error.log")) { throw 'WinUI failed after opening' }
    Write-Output "WinUI window visible: $($ui.MainWindowTitle)"
    # Record the actual native window on the disposable runner for visual QA.
    Add-Type @'
using System; using System.Runtime.InteropServices;
public static class ScreenSize { [DllImport("user32.dll")] public static extern int GetSystemMetrics(int n); }
'@
    $bitmap = New-Object Drawing.Bitmap([ScreenSize]::GetSystemMetrics(0),[ScreenSize]::GetSystemMetrics(1))
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen(0,0,0,0,$bitmap.Size)
    $bitmap.Save("$root/dist/installer/first-run.png",[Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
    . "$PSScriptRoot/test-windows-ui.ps1"
    $null = $ui.Handle # retain a real handle so ExitCode remains available after WM_CLOSE
    $ui.CloseMainWindow() | Out-Null
    if (!$ui.WaitForExit(10000)) { throw 'WinUI did not close within the deadline' }
    Write-Output "WinUI exit code: $($ui.ExitCode)"
    if ($ui.ExitCode -ne 0) {
        Get-WinEvent -FilterHashtable @{LogName='Application';StartTime=(Get-Date).AddMinutes(-5)} -ErrorAction SilentlyContinue | Where-Object { $_.ProviderName -in @('.NET Runtime','Application Error') -and $_.Message -match 'RustClipSync|Microsoft.UI.Xaml' } | Select-Object -First 5 TimeCreated,Message | Format-List
        throw 'WinUI did not close cleanly'
    }
    . "$PSScriptRoot/test-windows-paired-ui.ps1"
} finally {
    $env:LOCALAPPDATA = $originalData
    $env:RCS_UI_METRICS_FILE = $originalMetrics
    if (Test-Path $metricsPath) { Copy-Item $metricsPath "$root/dist/installer/runtime-ui-events.jsonl" -Force }
}
. "$PSScriptRoot/test-windows-installer-help.ps1"
$remove = Start-Process "$destination/unins000.exe" -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART') -Wait -PassThru
if ($remove.ExitCode -ne 0 -or (Test-Path "$destination/RustClipSync.exe")) { throw 'Uninstall verification failed' }
Write-Output 'PASS: per-user install, embedded icons, double-click first-run WinUI and uninstall.'
