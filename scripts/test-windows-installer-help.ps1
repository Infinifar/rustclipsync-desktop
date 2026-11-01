if ($env:GITHUB_ACTIONS -ne 'true' -or [string]::IsNullOrEmpty($env:RUNNER_TEMP) -or [string]::IsNullOrEmpty($destination)) { throw 'Run only from the disposable installer test.' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$wizard = Start-Process $setup.FullName -ArgumentList @('/SP-','/SUPPRESSMSGBOXES','/NORESTART','/TASKS=','/LANG=chinesesimp',"/DIR=`"$destination`"") -PassThru
try {
    $deadline = (Get-Date).AddSeconds(90)
    $verified = $false
    do {
        Start-Sleep -Milliseconds 300
        if ($wizard.HasExited) { throw 'Interactive setup exited before its download buttons were verified' }
        $owners = @($wizard.Id) + @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($wizard.Id)" | ForEach-Object ProcessId)
        $windows = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition)
        foreach ($window in $windows) {
            if ($window.Current.ProcessId -notin $owners -or $window.Current.ClassName -ne 'TWizardForm') { continue }
            $elements = $window.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
            $buttons = @($elements | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.IsEnabled -and !$_.Current.IsOffscreen })
            $finish = $buttons | Where-Object { $_.Current.Name.Replace('&','') -match '^完成|^Finish$' } | Select-Object -First 1
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
                $finish.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
                $verified = $true; break
            }
            $accept = $elements | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::RadioButton -and $_.Current.Name.Replace('&','') -match '^我接受|^I accept' } | Select-Object -First 1
            if ($accept) { $accept.GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select() }
            $next = $buttons | Where-Object { $_.Current.Name.Replace('&','') -match '^下一步|^安装|^Next|^Install$' } | Select-Object -First 1
            if ($next) { $next.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke() }
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
    if (!$wizard.HasExited) { $wizard.Kill(); $wizard.WaitForExit() }
    $wizard.Dispose()
}
