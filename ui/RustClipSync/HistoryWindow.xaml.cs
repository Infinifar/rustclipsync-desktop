using System.Runtime.InteropServices;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
namespace RustClipSync;
public sealed partial class HistoryWindow : Window
{
    internal HistoryWindow(AppearanceSettings appearance)
    {
        InitializeComponent();
        var scale = Math.Max(1, GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0);
        var area = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        var width = Math.Min((int)(620 * scale), area.Width); var height = Math.Min((int)(600 * scale), area.Height);
        AppWindow.MoveAndResize(new Windows.Graphics.RectInt32(area.X + (area.Width-width)/2, area.Y + (area.Height-height)/2, width, height));
        if (AppWindow.Presenter is OverlappedPresenter presenter) { presenter.PreferredMinimumWidth = (int)(420*scale); presenter.PreferredMinimumHeight = (int)(320*scale); }
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "rustclipsync.ico"));
        Root.ActualThemeChanged += (_, _) => Caption(); ApplyAppearance(appearance);
    }
    internal void ApplyAppearance(AppearanceSettings appearance)
    {
        Root.RequestedTheme = appearance.Theme == "dark" ? ElementTheme.Dark : appearance.Theme == "light" ? ElementTheme.Light : ElementTheme.Default;
        Root.Resources["ControlContentThemeFontSize"] = (double)appearance.FontSize; Caption();
    }
    private void Caption() { var dark = Root.ActualTheme == ElementTheme.Dark ? 1 : 0; DwmSetWindowAttribute(WinRT.Interop.WindowNative.GetWindowHandle(this),20,ref dark,sizeof(int)); }
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("dwmapi.dll")] private static extern int DwmSetWindowAttribute(IntPtr hwnd,int attribute,ref int value,int size);
}
