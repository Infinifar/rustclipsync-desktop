using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace RustClipSync;
public sealed partial class MainWindow : Window
{
    private string? savedServer;
    private string? savedUser;
    private static string AgentPath => Path.Combine(AppContext.BaseDirectory, "rustclipsync-agent.exe");
    private static string ConfigPath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "RustClipSync", "config.json");
    public MainWindow()
    {
        InitializeComponent();
        var area = Microsoft.UI.Windowing.DisplayArea.GetFromWindowId(AppWindow.Id, Microsoft.UI.Windowing.DisplayAreaFallback.Primary).WorkArea;
        var width = Math.Min(680, area.Width); var height = Math.Min(730, area.Height);
        AppWindow.MoveAndResize(new Windows.Graphics.RectInt32(area.X + (area.Width-width)/2, area.Y + (area.Height-height)/2, width, height));
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "rustclipsync.ico"));
        LoadAccount();
    }
    private void LoadAccount()
    {
        try
        {
            if (!File.Exists(ConfigPath)) return;
            var info = new FileInfo(ConfigPath);
            if (info.Length > 65536) throw new IOException();
            using var doc = JsonDocument.Parse(File.ReadAllBytes(ConfigPath));
            savedServer = doc.RootElement.GetProperty("server").GetString();
            savedUser = doc.RootElement.GetProperty("user").GetString();
            ServerBox.Text = savedServer ?? "";
            UserBox.Text = savedUser ?? "";
            StartButton.IsEnabled = true;
            PasswordBox.PlaceholderText = "已配对时可留空";
            Show("已配对", "点击启动同步。修改账户或重新输入密码会重新配对。", InfoBarSeverity.Informational);
        }
        catch { Show("无法读取配置", "请检查本地数据目录权限后重新打开应用。", InfoBarSeverity.Error); }
    }
    private void SetBusy(bool value)
    {
        Busy.IsActive = value;
        ConnectButton.IsEnabled = StartButton.IsEnabled = StopButton.IsEnabled = !value;
        ServerBox.IsEnabled = UserBox.IsEnabled = PasswordBox.IsEnabled = !value;
        if (!value) StartButton.IsEnabled = savedServer is not null;
    }
    private void Show(string title, string message, InfoBarSeverity severity)
    {
        Notice.Title = title; Notice.Message = message; Notice.Severity = severity; Notice.IsOpen = true;
    }
    private static bool SameServer(string a, string b) => Uri.TryCreate(a, UriKind.Absolute, out var x) && Uri.TryCreate(b, UriKind.Absolute, out var y) && x.AbsoluteUri.TrimEnd('/') == y.AbsoluteUri.TrimEnd('/');
    private async void Connect_Click(object sender, RoutedEventArgs e)
    {
        if (Busy.IsActive) return;
        SetBusy(true);
        try
        {
        var server = ServerBox.Text.Trim();
        var user = UserBox.Text;
        if (!Uri.TryCreate(server, UriKind.Absolute, out var uri) || uri.Scheme != "https" || !string.IsNullOrEmpty(uri.UserInfo) || !string.IsNullOrEmpty(uri.Query) || !string.IsNullOrEmpty(uri.Fragment) || string.IsNullOrWhiteSpace(user))
        { Show("检查连接信息", "请填写 HTTPS 服务端根地址和账户。", InfoBarSeverity.Warning); return; }
        if (savedServer is not null && SameServer(server, savedServer) && user == savedUser && PasswordBox.Password.Length == 0)
        { StartAgent(); return; }
        if (PasswordBox.Password.Length == 0) { Show("需要密码", "请输入该服务端账户的密码以完成配对。", InfoBarSeverity.Warning); return; }
        var reset = File.Exists(ConfigPath);
        if (reset)
        {
            var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = "重新配对此设备？", Content = "这会放弃尚未发送的本地内容，并重新建立设备身份。旧服务端设备记录可以在服务端管理页面删除。", PrimaryButtonText = "重新配对", CloseButtonText = "取消", DefaultButton = ContentDialogButton.Close };
            if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
        }
            if (!await StopAgentAsync()) { Show("后台仍在退出", "请稍后重试，或从系统托盘退出 RustClipSync。", InfoBarSeverity.Warning); return; }
            var start = new ProcessStartInfo(AgentPath) { UseShellExecute = false, CreateNoWindow = true, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true };
            foreach (var arg in new[] { "pair", server, user, "--password-stdin" }) start.ArgumentList.Add(arg);
            if (reset) start.ArgumentList.Add("--reset");
            using var process = Process.Start(start) ?? throw new IOException();
            var output = process.StandardOutput.ReadToEndAsync();
            var error = process.StandardError.ReadToEndAsync();
            await process.StandardInput.WriteLineAsync(PasswordBox.Password);
            process.StandardInput.Close();
            PasswordBox.Password = "";
            // The child has bounded HTTP timeouts. Keep its operation exclusive
            // until it exits rather than retrying an uncertain registration.
            await process.WaitForExitAsync();
            await output;
            var code = (await error).Trim();
            if (process.ExitCode != 0)
            {
                Show("连接未完成", code.Contains("credentials", StringComparison.Ordinal) ? "账户或密码不正确。" : code.Contains("storage_error", StringComparison.Ordinal) ? "无法读取或保存加密状态，请检查本地权限和密钥。" : "请检查服务端地址、网络、证书和账户信息。", InfoBarSeverity.Error);
                return;
            }
            LoadAccount();
            StartAgent();
        }
        catch { Show("启动失败", "请确认已完整安装 RustClipSync，并检查服务端和本地数据目录。", InfoBarSeverity.Error); }
        finally { PasswordBox.Password = ""; SetBusy(false); }
    }
    private void Start_Click(object sender, RoutedEventArgs e) => StartAgent();
    private void StartAgent()
    {
        try
        {
            if (!File.Exists(ConfigPath)) { Show("尚未配对", "请先填写账户信息并连接。", InfoBarSeverity.Warning); return; }
            using var running = FindAgent();
            if (running is null) { using var started=Process.Start(new ProcessStartInfo(AgentPath) { UseShellExecute = false, CreateNoWindow = true, ArgumentList = { "run" } }); }
            Show("后台已启动", "可从系统托盘查看连接状态。关闭本窗口后仍会继续同步。", InfoBarSeverity.Success);
        }
        catch { Show("无法启动后台", "请重新安装完整应用后再试。", InfoBarSeverity.Error); }
    }
    private async void Stop_Click(object sender, RoutedEventArgs e)
    {
        SetBusy(true);
        try { Show(await StopAgentAsync() ? "后台已停止" : "后台仍在退出", "同步状态已保留，可稍后重新启动。", InfoBarSeverity.Informational); }
        catch { Show("无法停止后台", "请从系统托盘退出 RustClipSync。", InfoBarSeverity.Error); }
        finally { SetBusy(false); }
    }
    private static Process? FindAgent()
    {
        using var current = Process.GetCurrentProcess();
        foreach (var process in Process.GetProcessesByName("rustclipsync-agent"))
        {
            try
            {
                if (process.SessionId == current.SessionId && string.Equals(process.MainModule?.FileName, AgentPath, StringComparison.OrdinalIgnoreCase)) return process;
            }
            catch { }
            process.Dispose();
        }
        return null;
    }
    private static async Task<bool> StopAgentAsync()
    {
        using var process = FindAgent();
        if (process is null) return true;
        var window = FindWindow("RustClipSync.Agent.Window", null);
        if (window == IntPtr.Zero) return false;
        GetWindowThreadProcessId(window, out var pid);
        if (pid != process.Id) return false;
        if (!PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero)) return false;
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(45));
        try { await process.WaitForExitAsync(timeout.Token); return true; } catch (OperationCanceledException) { return false; }
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr FindWindow(string className, string? title);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", SetLastError = true)] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
}
