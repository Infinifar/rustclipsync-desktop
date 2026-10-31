using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;
using System.Text.Json;
namespace RustClipSync;
internal sealed class AppearanceSettings
{
    public string Theme { get; set; } = "system";
    public int FontSize { get; set; } = 14;
    internal static AppearanceSettings Load()
    {
        var path = Path.Combine(AgentClient.DataPath, "appearance.json");
        if (!File.Exists(path)) return new();
        if (new FileInfo(path).Length > 4096) throw new IOException();
        var result = JsonSerializer.Deserialize<AppearanceSettings>(File.ReadAllBytes(path), AgentClient.Json) ?? throw new IOException();
        if (result.Theme is not ("system" or "light" or "dark") || result.FontSize is not (14 or 16 or 18)) throw new IOException();
        return result;
    }
    internal void Save()
    {
        Directory.CreateDirectory(AgentClient.DataPath);
        var path = Path.Combine(AgentClient.DataPath, "appearance.json");
        var temporary = Path.Combine(AgentClient.DataPath, $".{Guid.NewGuid():N}.tmp");
        try
        {
            using (var file = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None)) { file.Write(JsonSerializer.SerializeToUtf8Bytes(this, AgentClient.Json)); file.Flush(true); }
            File.Move(temporary, path, true);
        }
        finally { if (File.Exists(temporary)) File.Delete(temporary); }
    }
}
internal static class StartupSettings
{
    private static string ShortcutPath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Startup), "RustClipSync.lnk");
    internal static bool Enabled()
    {
        if (!File.Exists(ShortcutPath)) return false;
        Verify(); return true;
    }
    private static void Verify()
    {
        if ((File.GetAttributes(ShortcutPath) & FileAttributes.ReparsePoint) != 0 || new FileInfo(ShortcutPath).Length > 65536) throw new IOException("startup_conflict");
        var raw = new ShellLink();
        try
        {
            ((IPersistFile)raw).Load(ShortcutPath, 0);
            var link = (IShellLink)raw; var target = new StringBuilder(32768); var args = new StringBuilder(1024);
            link.GetPath(target, target.Capacity, IntPtr.Zero, 4); link.GetArguments(args, args.Capacity);
            if (!string.Equals(Path.GetFullPath(target.ToString()), AgentClient.AgentPath, StringComparison.OrdinalIgnoreCase) || args.ToString() != "autostart") throw new IOException("startup_conflict");
        }
        finally { Marshal.FinalReleaseComObject(raw); }
    }
    internal static void Set(bool enabled)
    {
        if (File.Exists(ShortcutPath)) { Verify(); if (!enabled) File.Delete(ShortcutPath); return; }
        if (!enabled) return;
        var raw = new ShellLink();
        try
        {
            var link = (IShellLink)raw;
            link.SetPath(AgentClient.AgentPath); link.SetArguments("autostart"); link.SetWorkingDirectory(AppContext.BaseDirectory);
            link.SetDescription("RustClipSync clipboard synchronization"); link.SetIconLocation(Path.Combine(AppContext.BaseDirectory, "RustClipSync.exe"), 0);
            ((IPersistFile)raw).Save(ShortcutPath, true);
        }
        finally { Marshal.FinalReleaseComObject(raw); }
    }
    [ComImport, Guid("00021401-0000-0000-C000-000000000046")] private class ShellLink { }
    [ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellLink
    {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder file, int max, IntPtr data, uint flags);
        void GetIDList(out IntPtr list); void SetIDList(IntPtr list);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder text, int max); void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string text);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder text, int max); void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string text);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder text, int max); void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string text);
        void GetHotkey(out short hotkey); void SetHotkey(short hotkey); void GetShowCmd(out int show); void SetShowCmd(int show);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder text, int max, out int index); void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string text, int index);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string path, uint reserved); void Resolve(IntPtr window, uint flags); void SetPath([MarshalAs(UnmanagedType.LPWStr)] string file);
    }
}
