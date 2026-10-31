using Microsoft.UI.Xaml;
using System.Runtime.InteropServices;
namespace RustClipSync;
public partial class App : Application
{
    private Window? window;
    private bool launched;
    public App()
    {
        UnhandledException += (_, args) => { if (!launched) StartupFailure(args.Exception); };
        try { InitializeComponent(); } catch (Exception error) { StartupFailure(error); }
    }
    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            window = new MainWindow();
            window.Activate();
            launched = true;
        }
        catch (Exception error) { StartupFailure(error); }
    }
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
