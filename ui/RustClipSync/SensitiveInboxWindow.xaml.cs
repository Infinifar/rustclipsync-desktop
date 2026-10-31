using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace RustClipSync;

public sealed partial class SensitiveInboxWindow : Window
{
    private readonly AgentClient agent = new();
    private readonly DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(2) };
    private string? inboxId;
    private bool busy, closed, revealed;
    private long generation;

    internal SensitiveInboxWindow(AppearanceSettings appearance)
    {
        InitializeComponent();
        var scale = Math.Max(1, GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0);
        var area = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        var width = Math.Min((int)(460 * scale), area.Width);
        var height = Math.Min((int)(360 * scale), area.Height);
        AppWindow.MoveAndResize(new Windows.Graphics.RectInt32(area.X + (area.Width - width) / 2, area.Y + (area.Height - height) / 2, width, height));
        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.PreferredMinimumWidth = (int)(340 * scale);
            presenter.PreferredMinimumHeight = (int)(300 * scale);
            presenter.IsMaximizable = false;
        }
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "rustclipsync.ico"));
        Root.ActualThemeChanged += (_, _) => ApplyCaption();
        ApplyAppearance(appearance);
        Root.Loaded += async (_, _) => { await Refresh(); if (!closed) timer.Start(); };
        timer.Tick += async (_, _) => { if (!closed && !busy && !agent.IsBusy) await Refresh(); };
        Closed += (_, _) => { closed = true; generation++; timer.Stop(); HideText(); agent.Close(); };
    }

    internal void ApplyAppearance(AppearanceSettings appearance)
    {
        if (closed) return;
        Root.RequestedTheme = appearance.Theme == "dark" ? ElementTheme.Dark : appearance.Theme == "light" ? ElementTheme.Light : ElementTheme.Default;
        Root.Resources["ControlContentThemeFontSize"] = (double)appearance.FontSize;
        SecretText.FontSize = appearance.FontSize;
        ApplyCaption();
    }
    private void ApplyCaption()
    {
        if (closed) return;
        var dark = Root.ActualTheme == ElementTheme.Dark ? 1 : 0;
        DwmSetWindowAttribute(WinRT.Interop.WindowNative.GetWindowHandle(this), 20, ref dark, sizeof(int));
    }
    private void HideText()
    {
        revealed = false; SecretText.Text = "••••••••";
        SecretText.IsTextSelectionEnabled = false; RevealButton.Content = "显示";
    }
    private void Metadata(System.Text.Json.JsonElement snapshot)
    {
        var inbox = snapshot.GetProperty("inbox");
        var id = inbox.ValueKind == System.Text.Json.JsonValueKind.Null ? null : inbox.GetProperty("id").GetString();
        if (id != inboxId) { generation++; HideText(); Notice.IsOpen = false; }
        inboxId = id;
        ReceivedTime.Text = id is null ? "暂无敏感收件项" : $"收到时间：{DateTimeOffset.FromUnixTimeSeconds(inbox.GetProperty("receivedAt").GetInt64()).ToLocalTime().ToString("g", CultureInfo.CurrentCulture)}";
        Controls();
    }
    private void Controls()
    {
        RevealButton.IsEnabled = CopyButton.IsEnabled = DeleteButton.IsEnabled = !busy && inboxId is not null;
    }
    private async Task Refresh()
    {
        try { var snapshot = await agent.Call("getStatus"); if (!closed) Metadata(snapshot); }
        catch { if (!closed) { generation++; HideText(); inboxId = null; ReceivedTime.Text = "暂时无法读取收件项"; Controls(); } }
    }
    private void Error()
    {
        if (closed) return;
        Notice.Title = "操作未完成"; Notice.Message = "收件项可能已更新、删除，或后台暂时不可用。请稍后重试。";
        Notice.Severity = InfoBarSeverity.Warning; Notice.IsOpen = true;
    }
    private void Pin_Click(object sender, RoutedEventArgs e)
    {
        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.IsAlwaysOnTop = PinButton.IsChecked == true;
            PinButton.Content = presenter.IsAlwaysOnTop ? "取消置顶" : "置顶";
        }
    }
    private async void Reveal_Click(object sender, RoutedEventArgs e)
    {
        if (busy || inboxId is null) return;
        generation++;
        if (revealed) { HideText(); return; }
        var id = inboxId; var requestGeneration = generation;
        busy = true; Controls();
        try
        {
            // Read only after the user asks to reveal. The preview is bounded;
            // copyInbox still copies the full original inside Rust.
            var content = new StringBuilder(); var offset = 0; var total = 0; var chunks = 0;
            do
            {
                var chunk = await agent.Call("readInbox", new { id, offset });
                if (closed || generation != requestGeneration) return;
                if (chunk.GetProperty("id").GetString() != id || chunk.GetProperty("offset").GetInt32() != offset) throw new IOException();
                var text = chunk.GetProperty("text").GetString() ?? throw new IOException();
                total = chunk.GetProperty("total").GetInt32();
                var next = chunk.GetProperty("nextOffset").GetInt32();
                if (total < 0 || total > 10 * 1024 * 1024 || next < offset || next > total || next - offset != Encoding.UTF8.GetByteCount(text) || (next == offset && next != total)) throw new IOException();
                content.Append(text); offset = next; chunks++;
            } while (offset < total && chunks < 4);
            var snapshot = await agent.Call("getStatus");
            if (closed || generation != requestGeneration) return;
            Metadata(snapshot);
            if (inboxId != id || generation != requestGeneration) return;
            SecretText.Text = content.ToString(); SecretText.IsTextSelectionEnabled = true;
            revealed = true; RevealButton.Content = "隐藏";
            if (offset < total) { Notice.Title = "仅显示开头部分"; Notice.Message = "内容较长。复制按钮仍会复制完整原文。"; Notice.Severity = InfoBarSeverity.Informational; Notice.IsOpen = true; }
        }
        catch { if (!closed) { HideText(); Error(); } }
        finally { busy = false; if (!closed) Controls(); }
    }
    private async void Copy_Click(object sender, RoutedEventArgs e)
    {
        if (busy || inboxId is null) return;
        var id = inboxId; busy = true; Controls();
        try
        {
            var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = "复制敏感文本？", Content = "复制后其他应用可以读取当前剪贴板。会附带敏感标记，请求排除 Windows 剪贴板历史和云同步。", PrimaryButtonText = "复制", CloseButtonText = "取消", DefaultButton = ContentDialogButton.Close, RequestedTheme = Root.RequestedTheme };
            if (await dialog.ShowAsync() != ContentDialogResult.Primary || closed) return;
            await agent.Call("copyInbox", new { id });
            if (!closed) { Notice.Title = "已复制"; Notice.Message = "已写入系统剪贴板。"; Notice.Severity = InfoBarSeverity.Success; Notice.IsOpen = true; }
        }
        catch { Error(); }
        finally { busy = false; if (!closed) { Controls(); await Refresh(); } }
    }
    private async void Delete_Click(object sender, RoutedEventArgs e)
    {
        if (busy || inboxId is null) return;
        var id = inboxId; generation++; HideText(); busy = true; Controls();
        try { await agent.Call("clearInbox", new { id }); }
        catch { Error(); }
        finally { busy = false; if (!closed) await Refresh(); }
    }
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("dwmapi.dll")] private static extern int DwmSetWindowAttribute(IntPtr window, int attribute, ref int value, int size);
}
