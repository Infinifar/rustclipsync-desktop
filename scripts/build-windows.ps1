$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$bundle = Join-Path $root 'dist/bundle'
$installer = Join-Path $root 'dist/installer'
New-Item -ItemType Directory -Force $bundle,$installer | Out-Null
& dotnet publish "$root/ui/RustClipSync/RustClipSync.csproj" -c Release -r win-x64 -p:Platform=x64 --self-contained true -o $bundle
if ($LASTEXITCODE -ne 0) { throw 'WinUI publish failed' }
# An unpackaged app needs its own resource index, including compiled XAML.
# See https://learn.microsoft.com/windows/apps/windows-app-sdk/mrtcore/localize-strings#loading-strings-in-unpackaged-applications
$app = Get-ChildItem "$root/ui/RustClipSync/bin" -Recurse -Filter 'RustClipSync.exe' | Select-Object -First 1
if (!$app) { throw 'The WinUI build output is missing' }
$resources = Join-Path $root 'dist/app-resources'
New-Item -ItemType Directory -Force "$resources/Assets" | Out-Null
$xbf = @(Get-ChildItem $app.DirectoryName -Recurse -Filter '*.xbf')
if ($xbf.Count -lt 2) { throw 'Compiled App and MainWindow XAML files are missing' }
foreach ($file in $xbf) {
    $relative = [IO.Path]::GetRelativePath($app.DirectoryName,$file.FullName)
    foreach ($base in @($bundle,$resources)) {
        $target = Join-Path $base $relative
        New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
        Copy-Item $file.FullName $target -Force
    }
}
Copy-Item "$root/assets/rustclipsync.png","$root/assets/rustclipsync.ico" "$resources/Assets" -Force
# Merge WinUI's embedded theme/control resources into the unpackaged app index.
# Keeping only our XBF entries makes XamlControlsResources fail at startup.
Get-ChildItem $bundle -Filter '*.pri' | Where-Object Name -ne 'resources.pri' | Copy-Item -Destination $resources -Force
if (!(Test-Path "$bundle/Assets/rustclipsync.ico")) { throw 'Application icon asset is missing' }
$makepri = Get-ChildItem "${env:ProgramFiles(x86)}/Windows Kits/10/bin" -Recurse -Filter 'makepri.exe' |
    Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
if (!$makepri) { throw 'Windows SDK makepri.exe is required' }
$config = Join-Path $root 'dist/priconfig.xml'
& $makepri.FullName createconfig /cf $config /dq en-US /o
if ($LASTEXITCODE -ne 0) { throw 'PRI configuration generation failed' }
[xml]$xml = Get-Content $config -Raw
@($xml.SelectNodes('//packaging')) | ForEach-Object { $_.ParentNode.RemoveChild($_) | Out-Null }
$xml.Save($config)
& $makepri.FullName new /pr $resources /cf $config /of "$bundle/resources.pri" /in Application /o
if ($LASTEXITCODE -ne 0) { throw 'Application resource index generation failed' }
& $makepri.FullName dump /if "$bundle/resources.pri" /of "$root/dist/resources.xml" /o
if ($LASTEXITCODE -ne 0 -or !(Select-String -Path "$root/dist/resources.xml" -Pattern 'MainWindow.xbf' -SimpleMatch)) { throw 'MainWindow is absent from the application resource index' }
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
