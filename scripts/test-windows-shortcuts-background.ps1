if ($env:GITHUB_ACTIONS -ne 'true' -or [string]::IsNullOrEmpty($env:RUNNER_TEMP) -or [string]::IsNullOrEmpty($destination)) { throw 'Run only in the disposable installer test.' }
$isolatedPrefix = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
if (![IO.Path]::GetFullPath($env:LOCALAPPDATA).StartsWith($isolatedPrefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing non-fixture profile.' }
foreach ($entry in @(@{key=0x78;title='剪贴板历史'},@{key=0x79;title='敏感收件区'})) {
    Send-ProductShortcut $entry.key
    $popup = Wait-ProductWindow $entry.title $true
    $script:window = $popup
    if ($entry.key -eq 0x79 -and (Get-Control SensitiveWindowText).Current.Name -ne '••••••••') { throw 'Cold shortcut exposed sensitive plaintext' }
    $process = Get-Process -Id $popup.Current.ProcessId
    $null = $process.Handle
    if (Find-ProductWindow 'RustClipSync') { throw 'Cold shortcut opened the main UI instead of only its requested popup' }
    Send-ProductShortcut $entry.key
    Wait-ProductWindow $entry.title $false | Out-Null
    if (!$process.WaitForExit(10000) -or $process.ExitCode -ne 0) { throw 'Cold popup did not exit cleanly after repeated shortcut' }
}
$request = @{id=[Guid]::NewGuid().ToString();ipcVersion=1;method='getStatus';params=@{}} | ConvertTo-Json -Compress
$snapshot = $request | & "$destination/rustclipsync-agent.exe" rpc | ConvertFrom-Json
$configWithoutShortcuts = $snapshot.result.config
$configWithoutShortcuts.shortcuts = @{history=$null;inbox=$null}
$request = @{id=[Guid]::NewGuid().ToString();ipcVersion=1;method='updateSettings';params=@{config=$configWithoutShortcuts;expectedRevision=$snapshot.result.configRevision}} | ConvertTo-Json -Depth 12 -Compress
$result = $request | & "$destination/rustclipsync-agent.exe" rpc | ConvertFrom-Json
if (!$result.ok) { throw 'Unable to unbind shortcuts' }
Send-ProductShortcut 0x78
Send-ProductShortcut 0x79
if ((Find-ProductWindow '剪贴板历史') -or (Find-ProductWindow '敏感收件区')) { throw 'Disabled shortcuts still opened a window' }
Write-Output 'PASS: Rust-owned global shortcuts work with the main UI closed; both cold popup processes exit on repeat.'
