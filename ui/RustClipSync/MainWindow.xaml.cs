using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
namespace RustClipSync;
public sealed partial class MainWindow : Window
{
    private readonly AgentClient agent = new();
    private readonly DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(2) };
    private AgentSettings? config;
    private ulong revision, formRevision;
    private bool initialized, updating, busy, closed, running, paused, formLoaded;
    private string state = "stopped";
    private string? inboxId;
    private long lastSync;
    private ulong lastHistoryRevision;
    private int historyOffset, historyTotal;
    private List<HistoryRow> history = [];
    private AppearanceSettings appearance = new();
    private static string ConfigPath => Path.Combine(AgentClient.DataPath, "config.json");
    public MainWindow()
    {
        InitializeComponent();
        var area = Microsoft.UI.Windowing.DisplayArea.GetFromWindowId(AppWindow.Id, Microsoft.UI.Windowing.DisplayAreaFallback.Primary).WorkArea;
        var scale = Math.Max(1, GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0);
        var width = Math.Min((int)(860 * scale), area.Width); var height = Math.Min((int)(820 * scale), area.Height);
        AppWindow.MoveAndResize(new Windows.Graphics.RectInt32(area.X + (area.Width-width)/2, area.Y + (area.Height-height)/2, width, height));
        SetMinimumWindowSize(scale);
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "rustclipsync.ico"));
        try { appearance = AppearanceSettings.Load(); } catch { Show("外观设置无法读取", "暂时采用系统主题。可以在外观页重新保存。", InfoBarSeverity.Warning); }
        ThemeChoice.SelectedIndex = appearance.Theme == "dark" ? 2 : appearance.Theme == "light" ? 1 : 0;
        FontChoice.SelectedIndex = appearance.FontSize == 18 ? 2 : appearance.FontSize == 16 ? 1 : 0;
        Root.ActualThemeChanged += (_, _) => ApplyCaptionTheme();
        ApplyAppearance();
        ReadConfig(); LoadForm(); ReadStartup(); Navigation.SelectedItem = HomeNav;
        Root.Loaded += async (_, _) =>
        {
            if (initialized) return;
            initialized = true;
            SetMinimumWindowSize(Root.XamlRoot.RasterizationScale);
            var minimumScale = Root.XamlRoot.RasterizationScale;
            Root.XamlRoot.Changed += (xamlRoot, _) => {
                if (!closed && Math.Abs(xamlRoot.RasterizationScale - minimumScale) > 0.01)
                {
                    minimumScale = xamlRoot.RasterizationScale;
                    SetMinimumWindowSize(minimumScale);
                }
            };
            if (config is not null) { try { AgentClient.Start(); } catch { Show("无法启动后台", "请确认完整安装了 RustClipSync。", InfoBarSeverity.Error); } }
            await Refresh(); timer.Start();
        };
        timer.Tick += async (_, _) => { if (!busy && !agent.IsBusy && !closed) await Refresh(); };
        Closed += (_, _) => { closed = true; timer.Stop(); agent.Close(); };
    }
    private void SetMinimumWindowSize(double scale)
    {
        if (AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter presenter)
        {
            presenter.PreferredMinimumWidth = (int)Math.Ceiling(480 * scale);
            presenter.PreferredMinimumHeight = (int)Math.Ceiling(420 * scale);
        }
    }
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr window);
    private void Show(string title, string message, InfoBarSeverity severity = InfoBarSeverity.Informational)
    { if (closed) return; Notice.Title = title; Notice.Message = message; Notice.Severity = severity; Notice.IsOpen = true; }
    private void Error(Exception error)
    {
        var code = error is AgentError known ? known.Code : "unavailable";
        Show("操作未完成", code switch {
            "credentials" => "设备凭据无效，请在设置中输入密码重新配对。",
            "re_pair" => "服务端状态或设备身份已变更，请重新配对。",
            "conflict" => "另一端已经有更新。请明确选择发送本机内容或接收远端内容。",
            "settings_changed" => "设置已被托盘或其他窗口更改，请重新载入后再保存。",
            "expired" => "待发送内容已经过期，请主动重新发送或接收。",
            "storage_error" or "needs_restart" => "本地状态不可用，已停止自动操作。请退出后台并检查本地数据，再重新启动。",
            "clipboard_error" => "剪贴板暂时不可用或被其他应用占用，请稍后重试。",
            "invalid_filter" => "正则表达式无效，或超过 32 条、每条 512 字节的限制。",
            "invalid_request" => "请检查数值范围、过滤规则、条目是否仍然存在，以及收藏是否超过 50 条。",
            "history_error" => "本地历史无法读取或保存。同步状态没有被清空，可备份数据后手动清空历史。",
            "operation_unconfirmed" => "暂时无法确认操作结果。请刷新状态，避免重复发送同一操作。",
            "busy" => "后台正在处理其他操作，请稍后再试。",
            _ => "请检查后台是否已启动，以及服务端地址、网络、证书与本地权限。"
        }, InfoBarSeverity.Error);
    }
    private void SetBusy(bool value)
    {
        busy = value; ConnectButton.IsEnabled = !value; SaveSettingsButton.IsEnabled = !value && running;
        PushButton.IsEnabled = PullButton.IsEnabled = !value && running;
        PauseButton.IsEnabled = !value; StopButton.IsEnabled = !value && running;
        DirectionBox.IsEnabled = !value && running;
        HistoryPanel.IsEnabled = !value && running;
    }
    private async Task<bool> Confirm(string title, string message, string action)
    {
        var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = title, Content = message, PrimaryButtonText = action, CloseButtonText = "取消", DefaultButton = ContentDialogButton.Close, RequestedTheme = Root.RequestedTheme };
        return await dialog.ShowAsync() == ContentDialogResult.Primary;
    }
    private void Navigation_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (HomePage is null || AppearancePage is null || SettingsPage is null) return;
        var page = (args.SelectedItem as NavigationViewItem)?.Tag as string;
        HomePage.Visibility = page == "home" ? Visibility.Visible : Visibility.Collapsed;
        AppearancePage.Visibility = page == "appearance" ? Visibility.Visible : Visibility.Collapsed;
        SettingsPage.Visibility = page == "settings" ? Visibility.Visible : Visibility.Collapsed;
        if (page == "settings" && initialized) { LoadForm(); ReadStartup(); }
        if (page != "settings") PasswordBox.Password = "";
        if (initialized) DispatcherQueue.TryEnqueue(() => {
            if (closed) return;
            PageScroll.ChangeView(null, 0, null, true);
            if (Notice.Severity is InfoBarSeverity.Success or InfoBarSeverity.Informational) Notice.IsOpen = false;
        });
    }
    private void OpenSettings_Click(object sender, RoutedEventArgs e) => Navigation.SelectedItem = SettingsNav;
    private void ReadConfig()
    {
        try
        {
            if (!File.Exists(ConfigPath)) return;
            if (new FileInfo(ConfigPath).Length > 65536) throw new IOException();
            config = JsonSerializer.Deserialize<AgentSettings>(File.ReadAllBytes(ConfigPath), AgentClient.Json) ?? throw new IOException();
        }
        catch { Show("无法读取配对配置", "请检查本地目录权限；不要删除加密状态来掩盖错误。", InfoBarSeverity.Error); }
    }
    private void LoadForm()
    {
        updating = true;
        try
        {
            var c = config ?? new AgentSettings();
            ServerBox.Text = c.Server; UserBox.Text = c.User; PasswordBox.Password = "";
            SensitiveToggle.IsOn = c.AllowSensitive; AutoSensitiveToggle.IsOn = c.AutoApplySensitive;
            AutoSensitiveToggle.IsEnabled = c.AllowSensitive; IntervalBox.Value = c.IntervalSec;
            FilterBox.SelectedIndex = c.Filter.Mode == "whitelist" ? 2 : c.Filter.Mode == "blacklist" ? 1 : 0;
            PatternsBox.Text = string.Join("\n", c.Filter.Patterns); FilterDownloads.IsOn = c.Filter.Downloads;
            HistoryToggle.IsOn = c.History.Enabled; HistoryLimitBox.Value = c.History.Limit; HistoryDaysBox.Value = c.History.Days;
            DirectionBox.SelectedIndex = c.Direction == "download" ? 2 : c.Direction == "upload" ? 1 : 0;
            formLoaded = config is not null; formRevision = revision;
        }
        finally { updating = false; }
    }
    private async Task Refresh()
    {
        if (closed) return;
        using var process = AgentClient.Find(); running = process is not null;
        if (!running)
        {
            state = "stopped"; StateText.Text = config is null ? "尚未配置连接" : "后台未启动";
            PauseButton.Content = config is null ? "配置连接" : "启动同步";
            ServerText.Text = $"服务端：{config?.Server ?? "尚未配置"}";
            SetBusy(busy); return;
        }
        try
        {
            var snapshot = await agent.Call("getStatus"); if (closed) return;
            state = snapshot.GetProperty("state").GetString() ?? "connecting";
            paused = snapshot.GetProperty("paused").GetBoolean();
            config = snapshot.GetProperty("config").Deserialize<AgentSettings>(AgentClient.Json)!;
            revision = snapshot.GetProperty("configRevision").GetUInt64();
            if (formRevision == 0) formRevision = revision;
            StateText.Text = state switch { "connected" => "已连接 · 实时同步", "connecting" => "正在连接", "paused" => "已暂停自动同步", "offline" => "网络未连接", "credentials" => "设备凭据无效", "conflict" => "同步存在冲突", "expired" => "待发送内容已过期", "re_pair" => "需要重新配对", "storage_error" => "本地加密状态不可用", "clipboard_error" => "剪贴板暂时不可用", "stopping" => "后台正在退出", _ => "正在启动" };
            ServerText.Text = $"服务端：{config.Server}";
            var device = snapshot.GetProperty("deviceId").GetString(); DeviceText.Text = $"设备：{(string.IsNullOrEmpty(device) ? "正在读取" : device)}";
            var timestamp = snapshot.GetProperty("lastSync").GetInt64();
            LastSyncText.Text = timestamp == 0 ? "最近同步：本次运行尚未同步" : $"最近同步：{Time(timestamp)}";
            PendingText.Visibility = snapshot.GetProperty("pending").GetBoolean() ? Visibility.Visible : Visibility.Collapsed;
            var inbox = snapshot.GetProperty("inbox"); inboxId = inbox.ValueKind == JsonValueKind.Null ? null : inbox.GetProperty("id").GetString();
            InboxCard.Visibility = inboxId is null ? Visibility.Collapsed : Visibility.Visible;
            if (inboxId is not null) InboxTime.Text = $"收到时间：{Time(inbox.GetProperty("receivedAt").GetInt64())}";
            PauseButton.Content = paused ? "恢复" : "暂停";
            updating = true; DirectionBox.SelectedIndex = config.Direction == "download" ? 2 : config.Direction == "upload" ? 1 : 0; updating = false;
            HistoryNote.Text = config.History.Enabled ? $"本地历史已加密，普通记录保留 {config.History.Days} 天；收藏保留至手动删除。" : "历史记录未开启。已有记录仍按期限清理，可在设置中开启或手动清空。";
            if (snapshot.GetProperty("historyError").GetBoolean()) HistoryNote.Text = "历史记录不可用；同步独立运行。请备份数据后检查权限或手动清空历史。";
            if (!formLoaded) LoadForm();
            var historyRevision = snapshot.TryGetProperty("historyRevision", out var historyVersion) ? historyVersion.GetUInt64() : snapshot.GetProperty("cursor").GetUInt64();
            if (HistoryPanel.IsExpanded && (timestamp != lastSync || historyRevision != lastHistoryRevision)) await LoadHistory();
            lastSync = timestamp; lastHistoryRevision = historyRevision;
        }
        catch { if (!closed) StateText.Text = "后台正在启动或处理操作"; }
        finally { if (!closed) SetBusy(busy); }
    }
    private static string Time(long timestamp) => DateTimeOffset.FromUnixTimeSeconds(timestamp).ToLocalTime().ToString("g", CultureInfo.CurrentCulture);
    private async Task<bool> Action(string method, object? parameters = null)
    {
        if (busy) return false; SetBusy(true);
        var success = false;
        try { var result = await agent.Call(method, parameters); success = true; if (!closed) { Show("操作完成", result.TryGetProperty("outcome", out var outcome) ? outcome.GetString() switch { "filtered" => "内容已被敏感策略或过滤规则阻止。", "current" or "skipped" => "没有需要应用的新内容；不会重放已接收的剪贴板。", "stored" => "已存入敏感收件区，未写入系统剪贴板。", _ => "同步操作已完成。" } : "设置或本地操作已完成。", InfoBarSeverity.Success); } }
        catch (Exception error) { Error(error); }
        finally { SetBusy(false); await Refresh(); }
        return success;
    }
    private async void Pause_Click(object sender, RoutedEventArgs e)
    {
        if (busy) return;
        if (config is null) { Navigation.SelectedItem = SettingsNav; return; }
        if (!running) { try { AgentClient.Start(); await Task.Delay(500); await Refresh(); } catch (Exception error) { Error(error); } return; }
        await Action("setPaused", new { paused = !paused });
    }
    private async void Push_Click(object sender, RoutedEventArgs e)
    {
        if (busy) return;
        if (state is "conflict" or "expired" && !await Confirm("发送本机内容？", "这会明确选择当前系统剪贴板，替换服务端的最新内容。其他设备可能接收到它。", "发送本机内容")) return;
        await Action("pushNow");
    }
    private async void Pull_Click(object sender, RoutedEventArgs e)
    {
        if (busy) return;
        if (state is "conflict" or "expired" && !await Confirm("接收远端内容？", "这会放弃尚未发送的本机内容，并接收尚未消费的远端最新消息。", "接收远端内容")) return;
        await Action("pullNow");
    }
    private async void Direction_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!initialized || updating || busy || config is null || !running) return;
        var updated = config.Copy(); updated.Direction = (DirectionBox.SelectedItem as ComboBoxItem)?.Tag as string ?? "both";
        await Action("updateSettings", new { config = updated, expectedRevision = revision });
    }
    private async void Stop_Click(object sender, RoutedEventArgs e)
    {
        if (busy) return; SetBusy(true);
        try { if (!await agent.Stop()) throw new IOException(); Show("后台已退出", "配对身份与同步状态已保留。关闭自启开关可避免下次登录时再次启动。", InfoBarSeverity.Success); }
        catch (Exception error) { Error(error); }
        finally { SetBusy(false); await Refresh(); }
    }
    private static bool SameServer(string a, string b) => Uri.TryCreate(a, UriKind.Absolute, out var x) && Uri.TryCreate(b, UriKind.Absolute, out var y) && x.AbsoluteUri.TrimEnd('/') == y.AbsoluteUri.TrimEnd('/');
    private async void Connect_Click(object sender, RoutedEventArgs e)
    {
        if (busy) return; SetBusy(true);
        try
        {
            var server = ServerBox.Text.Trim(); var user = UserBox.Text;
            if (!Uri.TryCreate(server, UriKind.Absolute, out var uri) || uri.Scheme != "https" || !string.IsNullOrEmpty(uri.UserInfo) || !string.IsNullOrEmpty(uri.Query) || !string.IsNullOrEmpty(uri.Fragment) || string.IsNullOrWhiteSpace(user)) { Show("检查连接信息", "请填写 HTTPS 服务端根地址和有效账户。", InfoBarSeverity.Warning); return; }
            if (config is not null && SameServer(server, config.Server) && user == config.User && PasswordBox.Password.Length == 0) { AgentClient.Start(); await Task.Delay(500); await Refresh(); Navigation.SelectedItem = HomeNav; return; }
            if (PasswordBox.Password.Length == 0) { Show("需要密码", "首次连接或更改账户时，请输入账户密码。", InfoBarSeverity.Warning); return; }
            var reset = File.Exists(ConfigPath);
            if (reset && !await Confirm("重新配对此设备？", "这会放弃尚未发送的内容和当前敏感收件项，并重新建立设备身份。旧服务端设备记录可在服务端管理页面删除。", "重新配对")) return;
            if (!await agent.Stop()) { Show("后台仍在退出", "请稍后重试，或从托盘退出后台。", InfoBarSeverity.Warning); return; }
            var start = new ProcessStartInfo(AgentClient.AgentPath) { UseShellExecute = false, CreateNoWindow = true, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true };
            foreach (var argument in new[] { "pair", server, user, "--password-stdin" }) start.ArgumentList.Add(argument);
            if (reset) start.ArgumentList.Add("--reset");
            using var process = Process.Start(start) ?? throw new IOException();
            var output = process.StandardOutput.ReadToEndAsync(); var error = process.StandardError.ReadToEndAsync();
            await process.StandardInput.WriteLineAsync(PasswordBox.Password); process.StandardInput.Close(); PasswordBox.Password = "";
            await process.WaitForExitAsync(); await output; var code = (await error).Trim();
            if (process.ExitCode != 0) { Show("连接未完成", code.Contains("credentials", StringComparison.Ordinal) ? "账户或密码不正确。" : "请检查服务端地址、网络、证书、设备状态和本地权限。", InfoBarSeverity.Error); return; }
            ReadConfig(); formLoaded = false; AgentClient.Start(); await Task.Delay(500); await Refresh(); Navigation.SelectedItem = HomeNav;
            Show("配对完成", "设备身份已保存，日常同步无需再次输入账户密码。", InfoBarSeverity.Success);
        }
        catch (Exception error) { Error(error); }
        finally { PasswordBox.Password = ""; SetBusy(false); }
    }
    private FilterSettings FilterDraft() => new() { Mode = (FilterBox.SelectedItem as ComboBoxItem)?.Tag as string ?? "off", Patterns = PatternsBox.Text.Split('\n').Select(s => s.TrimEnd('\r')).Where(s => s.Length > 0).ToArray(), Downloads = FilterDownloads.IsOn };
    private async void SaveSettings_Click(object sender, RoutedEventArgs e)
    {
        if (busy || config is null) return;
        if (!double.IsFinite(IntervalBox.Value) || !double.IsFinite(HistoryLimitBox.Value) || !double.IsFinite(HistoryDaysBox.Value)) { Show("检查数值", "间隔、历史条数与天数必须填写有效整数。", InfoBarSeverity.Warning); return; }
        if (IntervalBox.Value != Math.Truncate(IntervalBox.Value) || HistoryLimitBox.Value != Math.Truncate(HistoryLimitBox.Value) || HistoryDaysBox.Value != Math.Truncate(HistoryDaysBox.Value)) { Show("检查数值", "请填写整数。", InfoBarSeverity.Warning); return; }
        var updated = config.Copy(); updated.AllowSensitive = SensitiveToggle.IsOn; updated.AutoApplySensitive = AutoSensitiveToggle.IsOn; updated.IntervalSec = (int)IntervalBox.Value; updated.Filter = FilterDraft(); updated.History = new() { Enabled = HistoryToggle.IsOn, Limit = (int)HistoryLimitBox.Value, Days = (int)HistoryDaysBox.Value };
        if (await Action("updateSettings", new { config = updated, expectedRevision = formRevision })) LoadForm();
    }
    private async void ReloadSettings_Click(object sender, RoutedEventArgs e) { if (busy) return; await Refresh(); LoadForm(); ReadStartup(); }
    private void SensitiveToggle_Toggled(object sender, RoutedEventArgs e)
    {
        if (AutoSensitiveToggle is not null) AutoSensitiveToggle.IsEnabled = SensitiveToggle.IsOn;
    }
    private void OpenPrivacySettings_Click(object sender, RoutedEventArgs e)
    {
        Navigation.SelectedItem = SettingsNav; PrivacySettingsSection.IsExpanded = true;
        PrivacySettingsSection.StartBringIntoView();
    }
    private async void TestFilter_Click(object sender, RoutedEventArgs e)
    {
        if (busy) return;
        try { var result = await agent.Call("testFilter", new { filter = FilterDraft(), text = FilterTestText.Text }); FilterTestResult.Text = $"规则{(result.GetProperty("matched").GetBoolean() ? "匹配" : "未匹配")}；发送：{(result.GetProperty("uploadAllowed").GetBoolean() ? "允许" : "阻止")}，接收：{(result.GetProperty("downloadAllowed").GetBoolean() ? "允许" : "阻止")}"; }
        catch (Exception error) { Error(error); }
    }
    private void ApplyAppearance()
    {
        Root.RequestedTheme = appearance.Theme == "dark" ? ElementTheme.Dark : appearance.Theme == "light" ? ElementTheme.Light : ElementTheme.Default;
        Navigation.FontSize = appearance.FontSize; Root.Resources["ControlContentThemeFontSize"] = (double)appearance.FontSize;
        ApplyCaptionTheme();
    }
    private void ApplyCaptionTheme()
    {
        if (closed) return;
        var dark = Root.ActualTheme == ElementTheme.Dark ? 1 : 0;
        DwmSetWindowAttribute(WinRT.Interop.WindowNative.GetWindowHandle(this), 20, ref dark, sizeof(int));
    }
    [DllImport("dwmapi.dll")] private static extern int DwmSetWindowAttribute(IntPtr window, int attribute, ref int value, int size);
    private void Appearance_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!initialized || updating) return;
        try { appearance.Theme = (ThemeChoice.SelectedItem as ComboBoxItem)?.Tag as string ?? "system"; appearance.FontSize = int.Parse((FontChoice.SelectedItem as ComboBoxItem)?.Tag as string ?? "14", CultureInfo.InvariantCulture); appearance.Save(); ApplyAppearance(); }
        catch { Show("无法保存外观", "请检查本地数据目录权限。", InfoBarSeverity.Error); }
    }
    private void ReadStartup()
    {
        updating = true;
        try { StartupToggle.IsOn = StartupSettings.Enabled(); StartupState.Text = StartupToggle.IsOn ? "已创建当前用户的登录启动项。" : "当前未启用登录启动。"; }
        catch { StartupState.Text = "启动项无法读取或被其他目标占用，未修改现有文件。"; }
        finally { updating = false; }
    }
    private void Startup_Toggled(object sender, RoutedEventArgs e)
    {
        if (!initialized || updating) return;
        try { StartupSettings.Set(StartupToggle.IsOn); ReadStartup(); Show("自启设置已更新", "此设置作用于当前 Windows 用户，下次登录时生效。", InfoBarSeverity.Success); }
        catch { Show("自启设置未完成", "启动项可能被其他目标占用，或系统限制了写入权限。现有启动项未被覆盖。", InfoBarSeverity.Error); ReadStartup(); }
    }
    private async void WindowsStartup_Click(object sender, RoutedEventArgs e) { try { await Windows.System.Launcher.LaunchUriAsync(new Uri("ms-settings:startupapps")); } catch { Show("无法打开系统设置", "可从 Windows 设置 → 应用 → 启动查看。", InfoBarSeverity.Warning); } }
    private async void History_Expanding(Expander sender, ExpanderExpandingEventArgs args) { if (initialized && !busy) await LoadHistory(); }
    private async Task LoadHistory()
    {
        try
        {
            var result = await agent.Call("listHistory", new { offset = historyOffset, query = HistorySearch.Text, favorites = HistoryScope.SelectedIndex == 1 }); if (closed) return;
            historyTotal = result.GetProperty("total").GetInt32(); history = result.GetProperty("items").EnumerateArray().Select(item => new HistoryRow { Id = item.GetProperty("id").GetString()!, Preview = item.GetProperty("preview").GetString()!, Favorite = item.GetProperty("favorite").GetBoolean(), Detail = $"{Time(item.GetProperty("timestamp").GetInt64())} · {(item.GetProperty("direction").GetString() == "upload" ? "发送" : "接收")}" }).ToList();
            HistoryList.ItemsSource = history; EmptyHistory.Visibility = history.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
            PreviousHistory.IsEnabled = historyOffset > 0; NextHistory.IsEnabled = historyOffset + 20 < historyTotal;
            HistoryCount.Text = historyTotal == 0 ? "0 条" : $"{Math.Min(historyOffset + 1, historyTotal)}–{Math.Min(historyOffset + 20, historyTotal)} / {historyTotal}";
        }
        catch (Exception error) { Error(error); }
    }
    private async void SearchHistory_Click(object sender, RoutedEventArgs e) { if (busy) return; historyOffset = 0; await LoadHistory(); }
    private async void PreviousHistory_Click(object sender, RoutedEventArgs e) { if (busy) return; historyOffset = Math.Max(0, historyOffset - 20); await LoadHistory(); }
    private async void NextHistory_Click(object sender, RoutedEventArgs e) { if (busy) return; historyOffset += 20; await LoadHistory(); }
    private async void CopyHistory_Click(object sender, RoutedEventArgs e) { if ((sender as Button)?.Tag is string id) await Action("copyHistory", new { id }); }
    private async void DeleteHistory_Click(object sender, RoutedEventArgs e) { if (busy) return; if ((sender as Button)?.Tag is string id) { await Action("deleteHistory", new { id }); await LoadHistory(); } }
    private async void FavoriteHistory_Click(object sender, RoutedEventArgs e) { if (busy) return; if ((sender as Button)?.Tag is string id) { var item = history.FirstOrDefault(i => i.Id == id); if (item is not null) { await Action("setFavorite", new { id, favorite = !item.Favorite }); await LoadHistory(); } } }
    private async void ClearHistory_Click(object sender, RoutedEventArgs e)
    {
        if (busy || !await Confirm("清空本地历史？", "将删除本机全部普通历史和收藏。不会清空系统剪贴板、敏感收件区、同步游标或设备身份。", "清空历史")) return;
        await Action("clearHistory"); historyOffset = 0; await LoadHistory();
    }
    private async void CopyInbox_Click(object sender, RoutedEventArgs e)
    {
        var id = inboxId;
        if (busy || id is null || !await Confirm("复制敏感文本？", "此操作会将敏感文本写入系统剪贴板，其他应用可以读取。会附带敏感标记，请求排除 Windows 剪贴板历史和云同步。", "复制")) return;
        await Action("copyInbox", new { id });
    }
    private async void ClearInbox_Click(object sender, RoutedEventArgs e) { if (!busy && inboxId is not null) await Action("clearInbox", new { id = inboxId }); }
    private async void Diagnostics_Click(object sender, RoutedEventArgs e)
    {
        if (busy) return;
        try { var data = await agent.Call("exportDiagnostics"); Directory.CreateDirectory(AgentClient.DataPath); var path = Path.Combine(AgentClient.DataPath, "diagnostics.json"); await File.WriteAllTextAsync(path, JsonSerializer.Serialize(data, new JsonSerializerOptions { WriteIndented = true })); DiagnosticsResult.Text = $"已保存到 {path}。文件不含正文、账户密码、设备令牌或过滤规则。"; }
        catch (Exception error) { Error(error); }
    }
}
public sealed class HistoryRow
{
    public string Id { get; set; } = ""; public string Preview { get; set; } = ""; public string Detail { get; set; } = ""; public bool Favorite { get; set; } public string FavoriteLabel => Favorite ? "取消收藏" : "收藏";
}
