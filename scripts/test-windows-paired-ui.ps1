if ($env:GITHUB_ACTIONS -ne 'true' -or [string]::IsNullOrEmpty($env:RUNNER_TEMP) -or [string]::IsNullOrEmpty($destination)) { throw 'This helper must be dot-sourced by the disposable Windows installer test.' }
$isolatedPrefix = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
if (![IO.Path]::GetFullPath($env:LOCALAPPDATA).StartsWith($isolatedPrefix,[StringComparison]::OrdinalIgnoreCase) -or ![IO.Path]::GetFullPath($destination).StartsWith($isolatedPrefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing to run UI fixtures outside isolated runner directories.' }
# Synthetic offline 0.1.0-style account, with a DPAPI key and RCS1 encrypted
# cursor/private inbox. Never contact a deployed service or use real credentials.
Add-Type -AssemblyName System.Security.Cryptography.ProtectedData
$data = Join-Path $env:LOCALAPPDATA 'RustClipSync'
New-Item -ItemType Directory -Force $data | Out-Null
$server = 'https://127.0.0.1:1/'
$identity = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes("$server`0fixture"))).ToLowerInvariant()
$key = [Security.Cryptography.RandomNumberGenerator]::GetBytes(32)
$protected = [Security.Cryptography.ProtectedData]::Protect($key,$null,[Security.Cryptography.DataProtectionScope]::CurrentUser)
[IO.File]::WriteAllBytes("$data/$identity.key.dpapi",$protected)
$device = [Guid]::NewGuid().ToString()
$privateId = [Guid]::NewGuid().ToString()
$body = @{
    server=$server; token=[Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant(); paired=$true;
    state=@{epoch=[Guid]::NewGuid().ToString();deviceId=$device;cursor=42;pending=$null;queued=$null;discardPending=$false;applying=$null;ack=$null};
    inbox=@{id=$privateId;text=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('UI private fixture'));receivedAt=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds()}
}
$clear = [Text.Encoding]::UTF8.GetBytes(($body | ConvertTo-Json -Depth 6 -Compress))
$nonce = [Security.Cryptography.RandomNumberGenerator]::GetBytes(12)
$cipher = [byte[]]::new($clear.Length); $tag = [byte[]]::new(16)
$crypto = [Security.Cryptography.AesGcm]::new($key,16)
$crypto.Encrypt($nonce,$clear,$cipher,$tag,[Text.Encoding]::ASCII.GetBytes('RCS1')); $crypto.Dispose()
[IO.File]::WriteAllBytes("$data/$identity.state",([Text.Encoding]::ASCII.GetBytes('RCS1') + $nonce + $cipher + $tag + [byte[]]@(10)))
@{server=$server;user='fixture';direction='both';paused=$true} | ConvertTo-Json | Set-Content "$data/config.json" -Encoding utf8
$env:NO_PROXY = '127.0.0.1,localhost'
$ui = Start-Process "$destination/RustClipSync.exe" -PassThru
$end = (Get-Date).AddSeconds(30)
do { Start-Sleep -Milliseconds 250; $ui.Refresh() } while ($ui.MainWindowHandle -eq 0 -and !$ui.HasExited -and (Get-Date) -lt $end)
if ($ui.HasExited -or $ui.MainWindowHandle -eq 0) { throw 'Reopened paired WinUI did not start' }
$script:window = [System.Windows.Automation.AutomationElement]::FromHandle($ui.MainWindowHandle)
$end = (Get-Date).AddSeconds(20)
do { $status = Get-Control SyncState; if ($status.Current.Name -eq '已暂停自动同步') { break }; Start-Sleep -Milliseconds 200 } while ((Get-Date) -lt $end)
if ($status.Current.Name -ne '已暂停自动同步') { throw "WinUI did not read actual backend state: $($status.Current.Name)" }
$all = $script:window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
foreach ($element in $all) { if ($element.Current.Name -like '*UI private fixture*') { throw 'Private inbox plaintext leaked into UI automation' } }
Start-Sleep -Seconds 2 # let the compositor finish the reopened window's first frames
Save-Screen 'paired-home-dark'
# Appearance JSON from the preceding process must still be selected after reopen.
Invoke-Control NavAppearance
$selected = (Get-Control ThemeChoice).GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection()
if ($selected.Count -ne 1 -or $selected[0].Current.Name -ne '深色') { throw 'Theme did not survive process exit' }
$selected = (Get-Control FontChoice).GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection()
if ($selected.Count -ne 1 -or $selected[0].Current.Name -ne '大') { throw 'Font size did not survive process exit' }
Invoke-Control NavHome
Invoke-Control PauseSync
$end = (Get-Date).AddSeconds(15)
do { $stored = Get-Content "$data/config.json" -Raw | ConvertFrom-Json; if (!$stored.paused) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if ($stored.paused) { throw 'WinUI did not resume the actual backend' }
Start-Sleep -Milliseconds 600
Invoke-Control PauseSync
$end = (Get-Date).AddSeconds(15)
do { $stored = Get-Content "$data/config.json" -Raw | ConvertFrom-Json; if ($stored.paused) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if (!$stored.paused) { throw 'WinUI did not pause the actual backend' }
Invoke-Control OpenSensitiveSettings
$autoSensitive = Get-Control AutoApplySensitive
if ($autoSensitive.Current.IsOffscreen) { throw 'Sensitive settings shortcut did not scroll to its target' }
if ($autoSensitive.Current.IsEnabled -or $autoSensitive.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off) { throw 'Sensitive auto-write was not disabled by default for migrated config' }
(Get-Control AllowSensitive).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
if (!(Get-Control AutoApplySensitive).Current.IsEnabled) { throw 'Sensitive auto-write did not follow master toggle' }
$autoSensitive.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
Invoke-Control SaveSyncSettings
$end = (Get-Date).AddSeconds(15)
do { $stored = Get-Content "$data/config.json" -Raw | ConvertFrom-Json; if ($stored.allowSensitive -and $stored.autoApplySensitive) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if (!$stored.allowSensitive -or !$stored.autoApplySensitive) { throw 'WinUI sensitive auto-write setting did not reach Rust config' }
Save-Screen 'sensitive-auto-write-settings'
# Preserve the previous inbox without applying it merely because the option changed.
$request = @{id=[Guid]::NewGuid().ToString();ipcVersion=1;method='getStatus';params=@{}} | ConvertTo-Json -Compress
$snapshot = $request | & "$destination/rustclipsync-agent.exe" rpc | ConvertFrom-Json
if ($snapshot.result.cursor -ne 42 -or $snapshot.result.inbox.id -ne $privateId) { throw 'Changing sensitive policy altered an old cursor/inbox' }
(Get-Control AutoApplySensitive).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
(Get-Control AllowSensitive).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
if ((Get-Control AutoApplySensitive).Current.IsEnabled) { throw 'Sensitive auto-write stayed enabled with sensitive sync off' }
Invoke-Control SaveSyncSettings
$end = (Get-Date).AddSeconds(15)
do { $stored = Get-Content "$data/config.json" -Raw | ConvertFrom-Json; if (!$stored.allowSensitive -and !$stored.autoApplySensitive) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if ($stored.allowSensitive -or $stored.autoApplySensitive) { throw 'Disabling sensitive auto-write was not persisted' }
(Get-Control HistorySettingsSection).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
Start-Sleep -Milliseconds 300
(Get-Control HistoryEnabled).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
Invoke-Control SaveSyncSettings
$end = (Get-Date).AddSeconds(15)
do { $stored = Get-Content "$data/config.json" -Raw | ConvertFrom-Json; if ($stored.history.enabled) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $end)
if (!$stored.history.enabled -or $stored.history.days -ne 7) { throw 'WinUI settings were not saved by the backend' }
. "$PSScriptRoot/test-windows-sensitive-ui.ps1"
. "$PSScriptRoot/test-windows-shortcuts-ui.ps1"
# Closing the UI must leave exactly one background process; stop it explicitly.
$ui.CloseMainWindow() | Out-Null
if (!$ui.WaitForExit(10000) -or $ui.ExitCode -ne 0) { throw 'Paired UI did not exit cleanly' }
$background = @(Get-Process -Name rustclipsync-agent -ErrorAction SilentlyContinue | Where-Object Path -eq (Join-Path $destination 'rustclipsync-agent.exe'))
if ($background.Count -ne 1) { throw 'Closing UI did not preserve exactly one background process' }
. "$PSScriptRoot/test-windows-shortcuts-background.ps1"
$request = @{id=[Guid]::NewGuid().ToString();ipcVersion=1;method='quit';params=@{}} | ConvertTo-Json -Compress
$request | & "$destination/rustclipsync-agent.exe" rpc | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Explicit background quit failed' }
if (!$background[0].WaitForExit(10000)) { throw 'Background did not stop' }
if ((Get-Content "$data/config.json" -Raw | ConvertFrom-Json).user -ne 'fixture') { throw 'Pairing account changed unexpectedly' }
Write-Output 'PASS: legacy config migration, real C# to Rust control/settings, private inbox redaction, persistent appearance and UI-independent background lifetime.'
