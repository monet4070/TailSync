using System.Text.Json;
using TailSync.Native.Core;

namespace TailSync.Wpf;

public static class Localizer
{
    private static readonly Dictionary<string, Dictionary<string, string>> Catalogs = new();
    public static string Locale { get; private set; } = "zh-CN";
    public static event EventHandler? Changed;
    public static void SetLocale(string locale)
    {
        locale = locale == "en" ? "en" : "zh-CN";
        if (Locale == locale) return;
        Locale = locale; Changed?.Invoke(null, EventArgs.Empty);
    }
    public static string Text(string key)
    {
        if (!Catalogs.TryGetValue(Locale, out var catalog))
        {
            using var stream = typeof(Localizer).Assembly.GetManifestResourceStream("TailSync.Wpf." + Locale + ".json");
            catalog = stream == null ? new() : JsonSerializer.Deserialize<Dictionary<string, string>>(stream) ?? new();
            Catalogs[Locale] = catalog;
        }
        return catalog.GetValueOrDefault(key) ?? MacDesign.Text(key);
    }
    internal static string Choose(string chinese, string english) => Locale == "en" ? english : chinese;
}
