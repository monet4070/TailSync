using System.Globalization;
using System.Text.Json;
using System.Windows;
using System.Windows.Media;
using Microsoft.Win32;
namespace TailSync.Wpf;

public static class ThemeService
{
    private static readonly JsonElement Palettes = Load();
    public static string Appearance { get; private set; } = "light";
    public static string ThemeId { get; private set; } = "canvas";
    public static event EventHandler? Changed;
    private static JsonElement Load()
    {
        using var stream = typeof(ThemeService).Assembly.GetManifestResourceStream("TailSync.Wpf.Palettes.json")!;
        using var json = JsonDocument.Parse(stream); return json.RootElement.Clone();
    }
    public static void Apply(string? appearance = null, string? id = null)
    {
        if (appearance is "system" or "light" or "dark") Appearance = appearance;
        if (id != null && Palettes.TryGetProperty(id, out _)) ThemeId = id;
        var dark = Appearance == "dark" || (Appearance == "system" && IsSystemDark());
        foreach (var color in Palettes.GetProperty(ThemeId).GetProperty(dark ? "dark" : "light").EnumerateObject())
        {
            var key = color.Name + "Brush"; var (value, opacity) = ParsePaint(color.Value.GetString()!);
            if (Application.Current.Resources[key] is SolidColorBrush brush && !brush.IsFrozen) { brush.Color = value; brush.Opacity = opacity; }
            else Application.Current.Resources[key] = new SolidColorBrush(value) { Opacity = opacity };
        }
        Application.Current.Resources["DisplayFont"] = new FontFamily(ThemeId switch {
            "flux" => "Bahnschrift, DengXian, Segoe UI", "aura" => "Candara, YouYuan, Microsoft YaHei UI",
            "mono" => "Cascadia Mono, Cascadia Code, Consolas", _ => "STZhongsong, Songti SC, STSong, SimSun, Georgia"
        });
        Changed?.Invoke(null, EventArgs.Empty);
    }
    public static (Color Color, double Opacity) ParsePaint(string text)
    {
        if (text.StartsWith('#')) return ((Color)ColorConverter.ConvertFromString(text),1);
        var c = text[5..^1].Split(',').Select(s => double.Parse(s.Trim(), CultureInfo.InvariantCulture)).ToArray();
        return (Color.FromRgb((byte)c[0], (byte)c[1], (byte)c[2]),c[3]);
    }
    private static bool IsSystemDark()
    {
        using var key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        return key?.GetValue("AppsUseLightTheme") is int value && value == 0;
    }
}
