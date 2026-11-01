using System.Globalization;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace RustClipSync;

public sealed partial class HistoryView : UserControl
{
    private readonly AgentClient agent = new();
    private readonly DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(2) };
    private bool busy, active = true, closed, refreshQueued;
    private int historyOffset, historyTotal, generation;
    private string query = "";
    private bool favorites;
    private string? cacheEpoch;
    private ulong cacheRevision;
    private (string Query, bool Favorites, int Offset)? cacheQuery;
    private List<HistoryRow> history = [];
    public bool ExternalPolling { get; set; }
    public HistoryView()
    {
        InitializeComponent();
        Loaded += async (_, _) => {
            agent.Scope = ExternalPolling ? "homeHistory" : "historyWindow";
            if (!active || closed) return;
            if (!ExternalPolling) timer.Start();
            await RefreshCore(null, false);
        };
        Unloaded += (_, _) => timer.Stop();
        timer.Tick += async (_, _) => { if (active && !busy && !agent.IsBusy) await RefreshCore(null, false); };
    }
    internal void SetActive(bool value)
    {
        if (closed || active == value) return;
        active = value; generation++;
        if (!active) { timer.Stop(); return; }
        if (IsLoaded && !ExternalPolling) { timer.Start(); _ = RefreshCore(null, false); }
    }
    internal void Close()
    {
        closed = true; active = false; generation++; timer.Stop(); agent.Close();
        history.Clear(); HistoryList.ItemsSource = null;
    }
    private bool Current(int token) => !closed && active && IsLoaded && token == generation;
    private void Error()
    {
        if (!active || !IsLoaded || closed) return;
        Notice.Title = "历史操作未完成"; Notice.Message = "后台暂时不可用，或该条目已经变更。请稍后重试。";
        Notice.Severity = InfoBarSeverity.Warning; Notice.IsOpen = true;
    }
    public Task Refresh() => RefreshCore(null, true);
    internal Task RefreshFrom(JsonElement status) => RefreshCore(status, false);
    private void Note(bool enabled) => HistoryNote.Text = enabled ? "本地历史已加密。敏感收件项不会进入此列表。" : "历史记录未开启，不再新增；已有记录仍可查看。可在主窗口设置中开启。";
    private async Task RefreshCore(JsonElement? status, bool force)
    {
        if (closed || !active || !IsLoaded) return;
        if (busy) { refreshQueued |= force; return; }
        busy = true;
        var token = generation;
        var requested = (Query: query, Favorites: favorites, Offset: historyOffset);
        try
        {
            var snapshot = status ?? await agent.Call("getStatus");
            if (!Current(token)) return;
            Note(snapshot.GetProperty("config").GetProperty("history").GetProperty("enabled").GetBoolean());
            if (snapshot.GetProperty("historyError").GetBoolean()) throw new IOException();
            var epoch = snapshot.GetProperty("historyEpoch").GetString() ?? "";
            var revision = snapshot.GetProperty("historyRevision").GetUInt64();
            var sameQuery = cacheQuery == requested;
            if (!force && sameQuery && epoch.Length > 0 && epoch == cacheEpoch && revision == cacheRevision) return;
            var result = await agent.Call("listHistory", new { offset = requested.Offset, query = requested.Query, favorites = requested.Favorites, ifEpoch = sameQuery ? cacheEpoch : null, ifRevision = sameQuery && cacheEpoch is not null ? (ulong?)cacheRevision : null });
            if (!Current(token)) return;
            Note(result.GetProperty("enabled").GetBoolean());
            if (!result.GetProperty("unchanged").GetBoolean())
            {
                historyOffset = result.GetProperty("offset").GetInt32();
                historyTotal = result.GetProperty("total").GetInt32();
                history = result.GetProperty("items").EnumerateArray().Select(item => new HistoryRow {
                    Id = item.GetProperty("id").GetString()!, Preview = item.GetProperty("preview").GetString()!, Favorite = item.GetProperty("favorite").GetBoolean(),
                    Detail = $"{DateTimeOffset.FromUnixTimeSeconds(item.GetProperty("timestamp").GetInt64()).ToLocalTime().ToString("g", CultureInfo.CurrentCulture)} · {(item.GetProperty("direction").GetString() == "upload" ? "发送" : "接收")}" }).ToList();
                HistoryList.ItemsSource = history;
                EmptyHistory.Visibility = history.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
                PreviousHistory.IsEnabled = historyOffset > 0; NextHistory.IsEnabled = historyOffset + 20 < historyTotal;
                HistoryCount.Text = historyTotal == 0 ? "0 条" : $"{Math.Min(historyOffset + 1, historyTotal)}–{Math.Min(historyOffset + 20, historyTotal)} / {historyTotal}";
                RuntimeDiagnostics.Record("history-render", agent.Scope, "updated");
            }
            cacheEpoch = result.GetProperty("historyEpoch").GetString();
            cacheRevision = result.GetProperty("historyRevision").GetUInt64();
            cacheQuery = (requested.Query, requested.Favorites, historyOffset);
        }
        catch { cacheEpoch = null; Error(); }
        finally
        {
            busy = false;
            if (refreshQueued) { refreshQueued = false; if (active && !closed) _ = RefreshCore(null, true); }
        }
    }
    private async Task Change(string method, object parameters)
    {
        if (busy || !active || closed) return;
        busy = true;
        try { await agent.Call(method, parameters); }
        catch { cacheEpoch = null; Error(); }
        finally { busy = false; refreshQueued = false; await RefreshCore(null, false); }
    }
    private async void SearchHistory_Click(object sender, RoutedEventArgs e) { query = HistorySearch.Text; favorites = HistoryScope.SelectedIndex == 1; historyOffset = 0; generation++; await Refresh(); }
    private async void PreviousHistory_Click(object sender, RoutedEventArgs e) { historyOffset = Math.Max(0, historyOffset - 20); generation++; await Refresh(); }
    private async void NextHistory_Click(object sender, RoutedEventArgs e) { historyOffset += 20; generation++; await Refresh(); }
    private async void CopyHistory_Click(object sender, RoutedEventArgs e) { if ((sender as Button)?.Tag is string id) await Change("copyHistory", new { id }); }
    private async void DeleteHistory_Click(object sender, RoutedEventArgs e) { if ((sender as Button)?.Tag is string id) await Change("deleteHistory", new { id }); }
    private async void FavoriteHistory_Click(object sender, RoutedEventArgs e) { if ((sender as Button)?.Tag is string id && history.FirstOrDefault(i => i.Id == id) is { } item) await Change("setFavorite", new { id, favorite = !item.Favorite }); }
    private async void ClearHistory_Click(object sender, RoutedEventArgs e)
    {
        if (busy) return;
        var dialog = new ContentDialog { XamlRoot = XamlRoot, Title = "清空本地历史？", Content = "将删除普通历史与收藏，不改变同步身份、游标、敏感收件项或系统剪贴板。", PrimaryButtonText = "清空历史", CloseButtonText = "取消", DefaultButton = ContentDialogButton.Close, RequestedTheme = ActualTheme };
        if (await dialog.ShowAsync() == ContentDialogResult.Primary) { historyOffset = 0; generation++; await Change("clearHistory", new { }); }
    }
}
