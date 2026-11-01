using System.Runtime.InteropServices;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;

namespace RustClipSync;

internal sealed class WindowActivity : IDisposable
{
    private readonly Window window;
    private readonly Action<bool> changed;
    private readonly IntPtr handle;
    private bool active, disposed;
    internal WindowActivity(Window window, Action<bool> changed)
    {
        this.window = window; this.changed = changed;
        handle = WinRT.Interop.WindowNative.GetWindowHandle(window);
        window.Activated += Activated;
        window.AppWindow.Changed += Changed;
    }
    internal bool IsActive => !disposed && window.AppWindow.IsVisible && !IsIconic(handle);
    internal void Update(bool force = false)
    {
        if (disposed) return;
        if (!window.DispatcherQueue.HasThreadAccess) { window.DispatcherQueue.TryEnqueue(() => Update(force)); return; }
        var next = IsActive;
        if (!force && next == active) return;
        active = next; changed(next);
    }
    private void Activated(object sender, WindowActivatedEventArgs args) => Update();
    private void Changed(AppWindow sender, AppWindowChangedEventArgs args) => Update();
    public void Dispose()
    {
        disposed = true;
        window.Activated -= Activated;
        window.AppWindow.Changed -= Changed;
    }
    [DllImport("user32.dll")] private static extern bool IsIconic(IntPtr window);
}
