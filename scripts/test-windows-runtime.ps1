if ($env:GITHUB_ACTIONS -ne 'true' -or [string]::IsNullOrEmpty($env:RUNNER_TEMP) -or [string]::IsNullOrEmpty($metricsPath)) { throw 'Run only from the isolated native UI tests.' }
function Wait-HistoryCount([string]$Expected) {
    $deadline = (Get-Date).AddSeconds(12)
    do {
        $count = Get-Control HistoryCount
        if ($count.Current.Name -eq $Expected) { return }
        Start-Sleep -Milliseconds 150
    } while ((Get-Date) -lt $deadline)
    throw "History page did not converge to $Expected; actual $($count.Current.Name)"
}
function History-Rpc([string]$Method,[hashtable]$Parameters) {
    $request = @{id=[Guid]::NewGuid().ToString();ipcVersion=1;method=$Method;params=$Parameters} | ConvertTo-Json -Compress
    # The CLI's pipe open is immediate, while the real WinUI client waits for
    # an available instance. Retry only ERROR_PIPE_BUSY, before any request
    # was sent; never replay a mutation after an ambiguous response failure.
    $errorsPath = Join-Path $env:RUNNER_TEMP 'runtime-history-rpc-error.txt'
    for ($attempt=0;$attempt -lt 15;$attempt++) {
        # This is a Windows-subsystem EXE. Keep a downstream pipeline consumer
        # so PowerShell waits for its redirected stdout and exit code.
        $raw = $request | & "$destination/rustclipsync-agent.exe" rpc 2>$errorsPath | Out-String
        $rpcExit = $LASTEXITCODE
        if ($rpcExit -eq 0) { break }
        if ((Get-Content $errorsPath -Raw) -notmatch 'os error 231') { throw "History test transport failed: $Method" }
        Start-Sleep -Milliseconds 100
    }
    $response = $raw | ConvertFrom-Json
    if ($rpcExit -ne 0 -or !$response.ok) { throw "History test operation failed: $Method (exit=$rpcExit error=$($response.error) responseChars=$($raw.Length))" }
    return $response.result
}
function Count-RuntimeEvents([string]$Kind,[string]$Scope,[string]$Detail) {
    @(Get-RuntimeEvents | Where-Object { $_.kind -eq $Kind -and $_.scope -eq $Scope -and (!$Detail -or $_.detail -eq $Detail) }).Count
}
$mainState = $script:window.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern)
$mainState.SetWindowVisualState([Windows.Automation.WindowVisualState]::Normal)
Start-Sleep -Milliseconds 400
Invoke-Control NavHome
$expander = (Get-Control HistoryPanel).GetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern)
$expander.Expand()
Wait-HistoryCount '1–20 / 25'
$pages = @(Get-RuntimeEvents | Where-Object kind -eq 'pages')
if ($pages[-1].detail -notmatch 'history') { throw 'Opening history did not realize the deferred content' }
$requests = Count-RuntimeEvents 'rpc' 'homeHistory' 'listHistory'
$renders = Count-RuntimeEvents 'history-render' 'homeHistory' ''
$polls = Count-RuntimeEvents 'rpc' 'main' 'getStatus'
Start-Sleep -Seconds 5
$idleRequests = (Count-RuntimeEvents 'rpc' 'homeHistory' 'listHistory') - $requests
$idleRenders = (Count-RuntimeEvents 'history-render' 'homeHistory' '') - $renders
if ($idleRequests -ne 0 -or $idleRenders -ne 0 -or (Count-RuntimeEvents 'rpc' 'main' 'getStatus') -le $polls) { throw 'Unchanged history was fetched or rebuilt instead of reusing the page' }
$listed = History-Rpc 'listHistory' @{offset=0;query='';favorites=$false}
$favoriteId = $listed.items[0].id
History-Rpc 'setFavorite' @{id=$favoriteId;favorite=$true} | Out-Null
$deadline = (Get-Date).AddSeconds(10)
do { if ((Count-RuntimeEvents 'history-render' 'homeHistory' '') -gt $renders) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $deadline)
if ((Count-RuntimeEvents 'history-render' 'homeHistory' '') -le $renders) { throw 'A changed history revision did not refresh the visible page' }
Invoke-Control NextHistory
Wait-HistoryCount '21–25 / 25'
Invoke-Control PreviousHistory
Wait-HistoryCount '1–20 / 25'
(Get-Control HistorySearch).GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue('runtime history fixture 0024')
Invoke-Control SearchHistory
Wait-HistoryCount '1–1 / 1'
(Get-Control HistorySearch).GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue('')
Select-Combo HistoryScope '收藏'
Invoke-Control SearchHistory
Wait-HistoryCount '1–1 / 1'
Select-Combo HistoryScope '全部'
Invoke-Control SearchHistory
Wait-HistoryCount '1–20 / 25'
$ui.Refresh()
$script:runtimeUiReport.historyWorkingSetBytes = $ui.WorkingSet64
$script:runtimeUiReport.historyPrivateBytes = $ui.PrivateMemorySize64
$script:runtimeUiReport.idleHistoryRequests = $idleRequests
$script:runtimeUiReport.idleHistoryRenders = $idleRenders
# Opening the independent inbox minimizes the main window. Its own refresh must
# continue while both the main window and its embedded history stop polling.
Invoke-Control OpenSensitiveWindow
$deadline = (Get-Date).AddSeconds(10)
do { $inspectors = @(Get-InspectorWindows); if ($inspectors.Count -eq 1 -and [InspectorWindowTest]::IsIconic($mainHandle)) { break }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $deadline)
if ($inspectors.Count -ne 1 -or ![InspectorWindowTest]::IsIconic($mainHandle)) { throw 'Independent inbox did not minimize the main window' }
Start-Sleep -Seconds 1
$mainRequests = Count-RuntimeEvents 'rpc' 'main' ''
$historyRequests = Count-RuntimeEvents 'rpc' 'homeHistory' ''
$inboxRequests = Count-RuntimeEvents 'rpc' 'inbox' 'getStatus'
History-Rpc 'setFavorite' @{id=$favoriteId;favorite=$false} | Out-Null
Start-Sleep -Seconds 5
$mainDelta = (Count-RuntimeEvents 'rpc' 'main' '') - $mainRequests
$historyDelta = (Count-RuntimeEvents 'rpc' 'homeHistory' '') - $historyRequests
$inboxDelta = (Count-RuntimeEvents 'rpc' 'inbox' 'getStatus') - $inboxRequests
if ($mainDelta -ne 0 -or $historyDelta -ne 0 -or $inboxDelta -lt 2) { throw "Minimized polling was incorrect: main=$mainDelta history=$historyDelta inbox=$inboxDelta" }
$script:runtimeUiReport.minimizedMainRequests = $mainDelta
$script:runtimeUiReport.minimizedHistoryRequests = $historyDelta
$script:runtimeUiReport.inboxRequestsWhileMainMinimized = $inboxDelta
$inspectors[0].GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern).Close()
$resume = [Diagnostics.Stopwatch]::StartNew()
$mainState.SetWindowVisualState([Windows.Automation.WindowVisualState]::Normal)
$deadline = (Get-Date).AddSeconds(2)
do { if ((Count-RuntimeEvents 'rpc' 'main' '') -gt $mainRequests) { break }; Start-Sleep -Milliseconds 50 } while ((Get-Date) -lt $deadline)
$resume.Stop()
if ((Count-RuntimeEvents 'rpc' 'main' '') -le $mainRequests) { throw 'Restore did not immediately refresh main status' }
$script:runtimeUiReport.restoreRefreshMilliseconds = $resume.ElapsedMilliseconds
$expander.Collapse()
Start-Sleep -Milliseconds 500
$historyRequests = Count-RuntimeEvents 'rpc' 'homeHistory' ''
Start-Sleep -Seconds 3
if ((Count-RuntimeEvents 'rpc' 'homeHistory' '') -ne $historyRequests) { throw 'Collapsed history kept polling the backend' }
$script:runtimeUiReport | ConvertTo-Json | Set-Content "$root/dist/installer/runtime-ui-report.json" -Encoding utf8
Save-Screen 'runtime-history-restored'
Write-Output 'PASS: deferred pages/history, revision-cached paging/search/favorites, no main/history polling when minimized, live inbox and immediate restore.'
