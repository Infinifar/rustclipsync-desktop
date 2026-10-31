using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;
namespace RustClipSync;
internal sealed class AgentClient
{
    internal static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
    private readonly SemaphoreSlim gate = new(1, 1);
    private readonly CancellationTokenSource lifetime = new();
    internal void Close() => lifetime.Cancel();
    internal static string AgentPath => Path.Combine(AppContext.BaseDirectory, "rustclipsync-agent.exe");
    internal static string DataPath => Path.Combine(Environment.GetEnvironmentVariable("LOCALAPPDATA") ?? Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "RustClipSync");
    private static int CurrentSession { get { using var process = Process.GetCurrentProcess(); return process.SessionId; } }
    private static string PipeName
    {
        get
        {
            var sid = WindowsIdentity.GetCurrent().User?.Value ?? throw new IOException();
            var fingerprint = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(sid))).ToLowerInvariant()[..16];
            return $"RustClipSync.{fingerprint}.{CurrentSession}";
        }
    }
    internal async Task<JsonElement> Call(string method, object? parameters = null)
    {
        await gate.WaitAsync();
        try
        {
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(lifetime.Token);
            timeout.CancelAfter(TimeSpan.FromSeconds(65));
            using var pipe = new NamedPipeClientStream(".", PipeName, PipeDirection.InOut, PipeOptions.Asynchronous, TokenImpersonationLevel.Anonymous);
            await pipe.ConnectAsync(1500, timeout.Token);
            if (!GetNamedPipeServerProcessId(pipe.SafePipeHandle, out var pid)) throw new IOException("peer_unavailable");
            using var peer = Process.GetProcessById(checked((int)pid));
            if (peer.SessionId != CurrentSession || !string.Equals(peer.MainModule?.FileName, AgentPath, StringComparison.OrdinalIgnoreCase)) throw new IOException("invalid_peer");
            var id = Guid.NewGuid().ToString();
            var request = JsonSerializer.SerializeToUtf8Bytes(new { id, ipcVersion = 1, method, @params = parameters ?? new { } }, Json);
            if (request.Length > 256 * 1024) throw new IOException("request_limit");
            await pipe.WriteAsync(BitConverter.GetBytes(request.Length), timeout.Token);
            await pipe.WriteAsync(request, timeout.Token);
            var header = new byte[4]; await pipe.ReadExactlyAsync(header, timeout.Token);
            var size = BitConverter.ToInt32(header);
            if (size <= 0 || size > 256 * 1024) throw new IOException("response_limit");
            var data = new byte[size]; await pipe.ReadExactlyAsync(data, timeout.Token);
            await pipe.WriteAsync(new byte[] { 1 }, timeout.Token);
            using var result = JsonDocument.Parse(data);
            var root = result.RootElement;
            if (root.GetProperty("id").GetString() != id) throw new IOException("invalid_response");
            if (!root.GetProperty("ok").GetBoolean()) throw new AgentError(root.GetProperty("error").GetString() ?? "unavailable");
            return root.GetProperty("result").Clone();
        }
        finally { gate.Release(); }
    }
    internal bool IsBusy => gate.CurrentCount == 0;
    internal static Process? Find()
    {
        using var current = Process.GetCurrentProcess();
        foreach (var process in Process.GetProcessesByName("rustclipsync-agent"))
        {
            try { if (process.SessionId == current.SessionId && string.Equals(process.MainModule?.FileName, AgentPath, StringComparison.OrdinalIgnoreCase)) return process; }
            catch { }
            process.Dispose();
        }
        return null;
    }
    internal static void Start()
    {
        using var running = Find();
        if (running is null) { using var started = Process.Start(new ProcessStartInfo(AgentPath) { UseShellExecute = false, CreateNoWindow = true, ArgumentList = { "run" } }); }
    }
    internal async Task<bool> Stop()
    {
        using var process = Find(); if (process is null) return true;
        try { await Call("quit"); }
        catch
        {
            var window = FindWindow("RustClipSync.Agent.Window", null);
            if (window == IntPtr.Zero) return false;
            GetWindowThreadProcessId(window, out var pid);
            if (pid != process.Id || !PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero)) return false;
        }
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(45));
        try { await process.WaitForExitAsync(timeout.Token); return true; } catch (OperationCanceledException) { return false; }
    }
    [DllImport("kernel32.dll", SetLastError = true)] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool GetNamedPipeServerProcessId(SafePipeHandle pipe, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr FindWindow(string className, string? title);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
}
internal sealed class AgentError(string code) : IOException(code) { internal string Code { get; } = code; }
internal sealed class FilterSettings { public string Mode { get; set; } = "off"; public string[] Patterns { get; set; } = []; public bool Downloads { get; set; } }
internal sealed class HistorySettings { public bool Enabled { get; set; } public int Limit { get; set; } = 50; public int Days { get; set; } = 7; }
internal sealed class AgentSettings
{
    public string Server { get; set; } = ""; public string User { get; set; } = "";
    public string Direction { get; set; } = "both"; public bool Paused { get; set; }
    public bool AllowSensitive { get; set; } public FilterSettings Filter { get; set; } = new();
    public HistorySettings History { get; set; } = new(); public int IntervalSec { get; set; } = 60;
    internal AgentSettings Copy() => JsonSerializer.Deserialize<AgentSettings>(JsonSerializer.Serialize(this, AgentClient.Json), AgentClient.Json)!;
}
