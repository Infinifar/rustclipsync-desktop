$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$bundle = Join-Path $root 'dist/bundle'
$installer = Join-Path $root 'dist/installer'
New-Item -ItemType Directory -Force $bundle,$installer | Out-Null
& dotnet publish "$root/ui/RustClipSync/RustClipSync.csproj" -c Release -r win-x64 -p:Platform=x64 --self-contained true -o $bundle
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
foreach ($entry in @('MainWindow.xbf','themeresources.xbf')) {
    if (!(Select-String -Path "$root/dist/resources.xml" -Pattern $entry -SimpleMatch)) { throw "Missing compiled XAML resource: $entry" }
}
Copy-Item "$root/target/release/rustclipsync-agent.exe" $bundle -Force
Copy-Item "$root/LICENSE" "$bundle/LICENSE.txt" -Force
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
& $compiler "/DBundleDir=$bundle" "/DInstallerOutput=$installer" "$root/packaging/RustClipSync.iss"
if ($LASTEXITCODE -ne 0) { throw 'Installer build failed' }
Get-FileHash "$installer/*.exe" -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLower())  $([IO.Path]::GetFileName($_.Path))" } | Set-Content "$installer/SHA256SUMS" -Encoding utf8
