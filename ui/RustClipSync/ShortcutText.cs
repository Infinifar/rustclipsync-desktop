namespace RustClipSync;

internal sealed record KeyBinding
{
    public uint Modifiers { get; set; }
    public uint Key { get; set; }
}
internal sealed class ShortcutSettings
{
    public KeyBinding? History { get; set; }
    public KeyBinding? Inbox { get; set; }
}
internal static class ShortcutText
{
    internal static KeyBinding? Parse(string value)
    {
        if (string.IsNullOrWhiteSpace(value)) return null;
        var parts = value.Split('+', StringSplitOptions.TrimEntries);
        uint modifiers = 0;
        foreach (var part in parts[..^1])
        {
            uint flag = part.ToUpperInvariant() switch { "CTRL" or "CONTROL" => 2, "ALT" => 1, "SHIFT" => 4, "WIN" => 8, _ => throw new FormatException() };
            if ((modifiers & flag) != 0) throw new FormatException();
            modifiers |= flag;
        }
        var name = parts[^1].ToUpperInvariant();
        uint key;
        if (name.Length == 1 && (name[0] is >= 'A' and <= 'Z' or >= '0' and <= '9')) key = name[0];
        else if (name.StartsWith('F') && uint.TryParse(name.AsSpan(1), out var f) && f is >= 1 and <= 24 && f != 12) key = 0x6f + f;
        else throw new FormatException();
        if ((modifiers & 11) == 0) throw new FormatException();
        return new() { Modifiers = modifiers, Key = key };
    }
    internal static string Format(KeyBinding? binding)
    {
        if (binding is null) return "";
        var parts = new List<string>();
        if ((binding.Modifiers & 2) != 0) parts.Add("Ctrl");
        if ((binding.Modifiers & 1) != 0) parts.Add("Alt");
        if ((binding.Modifiers & 4) != 0) parts.Add("Shift");
        if ((binding.Modifiers & 8) != 0) parts.Add("Win");
        parts.Add(binding.Key >= 0x70 ? $"F{binding.Key - 0x6f}" : ((char)binding.Key).ToString());
        return string.Join('+', parts);
    }
}
