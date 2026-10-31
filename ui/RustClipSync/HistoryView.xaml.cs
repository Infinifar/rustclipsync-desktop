using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace RustClipSync;

public sealed partial class HistoryView : UserControl
{
    private readonly AgentClient agent = new();
    private readonly DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(2) };
    private bool busy;
    private int historyOffset, historyTotal;
    private List<HistoryRow> history = [];
    public HistoryView()
    {
        InitializeComponent();
        Loaded += async (_, _) => { timer.Start(); await Refresh(); };
        Unloaded += (_, _) => timer.Stop();
        timer.Tick += async (_, _) => {
            for (DependencyObject? current = this; current is not null; current = VisualTreeHelper.GetParent(current))
                if (current is FrameworkElement element && element.Visibility != Visibility.Visible) return;
            if (IsLoaded && !busy && !agent.IsBusy) await Refresh();
        };
    }
    private void Error()
    {
        if (!IsLoaded) return;
        Notice.Title = "历史操作未完成"; Notice.Message = "后台暂时不可用，或该条目已经变更。请稍后重试。";
        Notice.Severity = InfoBarSeverity.Warning; Notice.IsOpen = true;
    }
    public async Task Refresh()
    {
        if (busy || !IsLoaded) return;
        busy = true;
        try
        {
            var status = await agent.Call("getStatus");
            var enabled = status.GetProperty("config").GetProperty("history").GetProperty("enabled").GetBoolean();
            var result = await agent.Call("listHistory", new { offset = historyOffset, query = HistorySearch.Text, favorites = HistoryScope.SelectedIndex == 1 });
            if (!IsLoaded) return;
            HistoryNote.Text = enabled ? "本地历史已加密。敏感收件项不会进入此列表。" : "历史记录未开启，不再新增；已有记录仍可查看。可在主窗口设置中开启。";
            historyTotal = result.GetProperty("total").GetInt32();
            history = result.GetProperty("items").EnumerateArray().Select(item => new HistoryRow {
                Id = item.GetProperty("id").GetString()!, Preview = item.GetProperty("preview").GetString()!, Favorite = item.GetProperty("favorite").GetBoolean(),
                Detail = $"{DateTimeOffset.FromUnixTimeSeconds(item.GetProperty("timestamp").GetInt64()).ToLocalTime().ToString("g", CultureInfo.CurrentCulture)} · {(item.GetProperty("direction").GetString() == "upload" ? "发送" : "接收")}" }).ToList();
            HistoryList.ItemsSource = history;
            EmptyHistory.Visibility = history.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
            PreviousHistory.IsEnabled = historyOffset > 0; NextHistory.IsEnabled = historyOffset + 20 < historyTotal;
            HistoryCount.Text = historyTotal == 0 ? "0 条" : $"{Math.Min(historyOffset + 1, historyTotal)}–{Math.Min(historyOffset + 20, historyTotal)} / {historyTotal}";
        }
        catch { Error(); }
        finally { busy = false; }
    }
    private async Task Change(string method, object parameters)
    {
        if (busy) return;
        busy = true;
        try { await agent.Call(method, parameters); }
        catch { Error(); }
        finally { busy = false; await Refresh(); }
    }
    private async void SearchHistory_Click(object sender, RoutedEventArgs e) { historyOffset = 0; await Refresh(); }
    private async void PreviousHistory_Click(object sender, RoutedEventArgs e) { historyOffset = Math.Max(0, historyOffset - 20); await Refresh(); }
    private async void NextHistory_Click(object sender, RoutedEventArgs e) { historyOffset += 20; await Refresh(); }
    private async void CopyHistory_Click(object sender, RoutedEventArgs e) { if ((sender as Button)?.Tag is string id) await Change("copyHistory", new { id }); }
    private async void DeleteHistory_Click(object sender, RoutedEventArgs e) { if ((sender as Button)?.Tag is string id) await Change("deleteHistory", new { id }); }
    private async void FavoriteHistory_Click(object sender, RoutedEventArgs e) { if ((sender as Button)?.Tag is string id && history.FirstOrDefault(i => i.Id == id) is { } item) await Change("setFavorite", new { id, favorite = !item.Favorite }); }
    private async void ClearHistory_Click(object sender, RoutedEventArgs e)
    {
        if (busy) return;
        var dialog = new ContentDialog { XamlRoot = XamlRoot, Title = "清空本地历史？", Content = "将删除普通历史与收藏，不改变同步身份、游标、敏感收件项或系统剪贴板。", PrimaryButtonText = "清空历史", CloseButtonText = "取消", DefaultButton = ContentDialogButton.Close, RequestedTheme = ActualTheme };
        if (await dialog.ShowAsync() == ContentDialogResult.Primary) { historyOffset = 0; await Change("clearHistory", new { }); }
    }
}
