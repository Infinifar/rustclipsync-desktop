using System.Text.Json;

namespace RustClipSync;

// Metadata-only instrumentation for the disposable native CI desktop. Disabled
// in ordinary use; never record request parameters, IDs or clipboard content.
internal static class RuntimeDiagnostics
{
    private static readonly string? path = OutputPath();
    private static readonly object gate = new();
    private static string? OutputPath()
    {
        try
        {
            if (Environment.GetEnvironmentVariable("GITHUB_ACTIONS") != "true") return null;
            var root = Environment.GetEnvironmentVariable("RUNNER_TEMP");
            var target = Environment.GetEnvironmentVariable("RCS_UI_METRICS_FILE");
            if (string.IsNullOrEmpty(root) || string.IsNullOrEmpty(target)) return null;
            root = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            target = Path.GetFullPath(target);
            return target.StartsWith(root, StringComparison.OrdinalIgnoreCase) ? target : null;
        }
        catch { return null; }
    }
    internal static void Record(string kind, string scope, string detail)
    {
        if (path is null) return;
        try
        {
            lock (gate)
            {
                if (File.Exists(path) && new FileInfo(path).Length > 2 * 1024 * 1024) return;
                File.AppendAllText(path, JsonSerializer.Serialize(new { process = Environment.ProcessId, time = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(), kind, scope, detail }) + "\n");
            }
        }
        catch { }
    }
    internal static void Pages(bool appearance, bool settings, bool shortcuts, bool history)
    {
        if (path is null) return;
        Record("pages", "main", string.Join(',', new[] { "home", appearance ? "appearance" : "", settings ? "settings" : "", shortcuts ? "shortcuts" : "", history ? "history" : "" }.Where(s => s.Length > 0)));
    }
}
