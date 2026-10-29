using System.Text.Json;

namespace TailSync.Native.Core;

/// Presentation reference shared by the native prototype and the inspectable preview.
/// Generated from the Mac source; this is not a backend contract.
public static class MacDesign
{
    public static JsonElement Root { get; } = Load();
    private static JsonElement Load()
    {
        var assembly = typeof(MacDesign).Assembly;
        using var stream = assembly.GetManifestResourceStream(assembly.GetManifestResourceNames()
            .Single(n => n.EndsWith("mac-ui-manifest.generated.json", StringComparison.Ordinal)))!;
        using var document = JsonDocument.Parse(stream);
        return document.RootElement.Clone();
    }
    public static string Text(string key) => Root.GetProperty("strings").TryGetProperty(key, out var value) ? value.GetString() ?? "" : "";
    public static (int Width, int Height) WindowSize(string page)
    {
        var size = Root.GetProperty("windows").GetProperty(page);
        return (size.GetProperty("width").GetInt32(), size.GetProperty("height").GetInt32());
    }
}
