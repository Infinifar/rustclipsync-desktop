$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$bundle = Join-Path $root 'dist/bundle'
$installer = Join-Path $root 'dist/installer'
$runtimes = Get-Content "$root/packaging/windows-runtimes.json" -Raw | ConvertFrom-Json
[xml]$project = Get-Content "$root/ui/RustClipSync/RustClipSync.csproj" -Raw
$winui = $project.SelectSingleNode("//PackageReference[@Include='Microsoft.WindowsAppSDK.WinUI']")
if (!$winui -or $winui.Version -ne $runtimes.windowsAppRuntime.version) { throw 'The Windows Runtime download must match the WinUI dependency' }
if ($project.Project.PropertyGroup.TargetFramework -notlike "net$($runtimes.dotnet.major).0-*") { throw 'The .NET download must match the target framework' }
# Publishing into a clean directory prevents stale self-contained DLLs from
# silently returning to the small installer on a developer machine.
if (Test-Path $bundle) { Remove-Item $bundle -Recurse -Force }
New-Item -ItemType Directory -Force $bundle,$installer | Out-Null
& dotnet publish "$root/ui/RustClipSync/RustClipSync.csproj" -c Release -r win-x64 -p:Platform=x64 --self-contained false -p:DebugType=None -p:DebugSymbols=false -o $bundle
if ($LASTEXITCODE -ne 0) { throw 'WinUI publish failed' }
# WinUI's build tooling emits a merged application/framework index named after
# the assembly. The unpackaged runtime discovers resources.pri beside the EXE.
$pri = Join-Path $bundle 'RustClipSync.pri'
if (!(Test-Path $pri)) { throw 'The merged WinUI application resource index is missing' }
Copy-Item $pri "$bundle/resources.pri" -Force
if (!(Test-Path "$bundle/Assets/rustclipsync.ico")) { throw 'Application icon asset is missing' }
$makepri = Get-ChildItem "${env:ProgramFiles(x86)}/Windows Kits/10/bin" -Recurse -Filter 'makepri.exe' |
    Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
if (!$makepri) { throw 'Windows SDK makepri.exe is required' }
& $makepri.FullName dump /if "$bundle/resources.pri" /of "$root/dist/resources.xml" /o
if ($LASTEXITCODE -ne 0) { throw 'Application resource index inspection failed' }
foreach ($entry in @('MainWindow.xbf','HistoryWindow.xbf','HistoryView.xbf','SensitiveInboxWindow.xbf')) {
    if (!(Select-String -Path "$root/dist/resources.xml" -Pattern $entry -SimpleMatch)) { throw "Missing compiled XAML resource: $entry" }
}
Copy-Item "$root/target/release/rustclipsync-agent.exe" $bundle -Force
Copy-Item "$root/LICENSE" "$bundle/LICENSE.txt" -Force
$help = Get-Content "$root/packaging/RuntimeHelp.html" -Raw
$help = $help.Replace('@@DOTNET_RUNTIME_URL@@', $runtimes.dotnet.downloadUrl)
$help = $help.Replace('@@WINDOWS_RUNTIME_URL@@', $runtimes.windowsAppRuntime.downloadUrl)
$help = $help.Replace('@@WINDOWS_RUNTIME_VERSION@@', $runtimes.windowsAppRuntime.version)
[IO.File]::WriteAllText("$bundle/RuntimeHelp.html", $help)
foreach ($file in @('coreclr.dll','System.Private.CoreLib.dll','onnxruntime.dll','DirectML.dll','Microsoft.ui.xaml.dll')) {
    if (Test-Path "$bundle/$file") { throw "A shared runtime was accidentally bundled: $file" }
}
$runtimeConfig = Get-Content "$bundle/RustClipSync.runtimeconfig.json" -Raw | ConvertFrom-Json
if ($runtimeConfig.runtimeOptions.includedFrameworks -or $runtimeConfig.runtimeOptions.framework.name -ne 'Microsoft.NETCore.App') { throw 'Expected framework-dependent .NET deployment' }
# Bundle the redistributable app-local C/C++ runtime for machines without a
# separately installed Visual C++ Redistributable. Do not modify machine state.
$vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
$vs = & $vswhere -latest -products '*' -property installationPath
$crt = Get-ChildItem "$vs/VC/Redist/MSVC" -Directory -Recurse |
    Where-Object { $_.Name -like 'Microsoft.VC*.CRT' -and $_.FullName -match '\\x64\\' } |
    Sort-Object FullName -Descending | Select-Object -First 1
if (!$crt) { throw 'The app-local Visual C++ redistributable was not found' }
Copy-Item "$($crt.FullName)/*.dll" $bundle -Force
$compiler = "${env:ProgramFiles(x86)}/Inno Setup 6/ISCC.exe"
if (!(Test-Path $compiler)) { throw 'Inno Setup 6 is required to build the installer' }
& $compiler "/DBundleDir=$bundle" "/DInstallerOutput=$installer" "/DDotNetRuntimeUrl=$($runtimes.dotnet.downloadUrl)" "/DWindowsRuntimeUrl=$($runtimes.windowsAppRuntime.downloadUrl)" "$root/packaging/RustClipSync.iss"
if ($LASTEXITCODE -ne 0) { throw 'Installer build failed' }
$files = @(Get-ChildItem $bundle -File -Recurse)
$setup = Get-Item "$installer/RustClipSync-*-setup-x64.exe"
if ($setup.Length -gt 20MB) { throw 'The small Windows installer exceeded the 20 MiB size budget' }
$size = [ordered]@{ installerBytes = $setup.Length; installedPayloadBytes = ($files | Measure-Object Length -Sum).Sum; fileCount = $files.Count; deployment = 'framework-dependent'; runtimes = $runtimes }
$size | ConvertTo-Json -Depth 5 | Set-Content "$installer/size-report.json" -Encoding utf8
Write-Output ("Installer: {0:N2} MB; payload: {1:N2} MB; files: {2}" -f ($size.installerBytes / 1e6), ($size.installedPayloadBytes / 1e6), $size.fileCount)
Get-FileHash "$installer/*.exe" -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLower())  $([IO.Path]::GetFileName($_.Path))" } | Set-Content "$installer/SHA256SUMS" -Encoding utf8
