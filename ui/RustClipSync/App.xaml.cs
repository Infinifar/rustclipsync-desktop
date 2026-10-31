using Microsoft.UI.Xaml;
using System.Runtime.InteropServices;
using Microsoft.Windows.AppLifecycle;
using Microsoft.UI.Dispatching;
namespace RustClipSync;
public partial class App : Application
{
    private MainWindow? window;
    private HistoryWindow? history;
    private SensitiveInboxWindow? inbox;
    private AppInstance? instance;
    private bool launched;
    public App()
    {
        UnhandledException += (_, args) => { if (!launched) StartupFailure(args.Exception); };
        try { InitializeComponent(); } catch (Exception error) { StartupFailure(error); }
    }
    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            var dispatcher = DispatcherQueue.GetForCurrentThread();
            var key = Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(System.Text.Encoding.UTF8.GetBytes($"{AppContext.BaseDirectory}|{AgentClient.DataPath}|{System.Diagnostics.Process.GetCurrentProcess().SessionId}")));
            instance = AppInstance.FindOrRegisterForKey(key);
            if (!instance.IsCurrent)
            {
                launched = true;
                AllowSetForegroundWindow(instance.ProcessId);
                await instance.RedirectActivationToAsync(AppInstance.GetCurrent().GetActivatedEventArgs());
                Exit(); return;
            }
            instance.Activated += (_, activation) => {
                var command = (activation.Data as Windows.ApplicationModel.Activation.ILaunchActivatedEventArgs)?.Arguments ?? "";
                dispatcher.TryEnqueue(() => Show(command));
            };
            Show(string.Join(' ', Environment.GetCommandLineArgs().Skip(1)));
            launched = true;
        }
        catch (Exception error) { StartupFailure(error); }
    }
    private void Show(string command)
    {
        var args = command.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        if (args.Contains("--toggle-history"))
        {
            if (history is not null) { history.Close(); return; }
            history = new HistoryWindow(Appearance()); history.Closed += (_, _) => history = null; Focus(history); return;
        }
        if (args.Contains("--toggle-inbox")) { OpenInbox(true, true); return; }
        if (window is null) { window = new MainWindow(); window.Closed += (_, _) => window = null; }
        Focus(window);
    }
    internal void OpenInbox(bool toggle, bool minimizeMain)
    {
        if (inbox is not null && toggle) { inbox.Close(); return; }
        if (inbox is null) { inbox = new SensitiveInboxWindow(Appearance()); inbox.Closed += (_, _) => inbox = null; }
        Focus(inbox);
        if (minimizeMain && window?.AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter presenter) presenter.Minimize();
    }
    internal void ApplyPopupAppearance(AppearanceSettings appearance) { history?.ApplyAppearance(appearance); inbox?.ApplyAppearance(appearance); }
    private static AppearanceSettings Appearance() { try { return AppearanceSettings.Load(); } catch { return new(); } }
    private static void Focus(Window window)
    {
        if (window.AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter presenter) presenter.Restore();
        window.Activate(); SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(window));
    }
    [DllImport("user32.dll")] private static extern bool AllowSetForegroundWindow(uint pid);
    [DllImport("user32.dll")] private static extern bool SetForegroundWindow(IntPtr hwnd);
    private static void StartupFailure(Exception error)
    {
        // This diagnostic runs only before the account/password form is usable.
        // Never record later requests, passwords or clipboard data.
        try { File.WriteAllText(Path.Combine(AppContext.BaseDirectory, "startup-error.log"), error.ToString()); } catch { }
        MessageBox(IntPtr.Zero, $"RustClipSync 无法打开。错误代码：0x{error.HResult:X8}。\n请使用完整安装包。启动诊断保存在程序目录 startup-error.log。", "RustClipSync 启动失败", 0x10);
        Environment.Exit(1);
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int MessageBox(IntPtr window, string text, string title, uint type);
}
