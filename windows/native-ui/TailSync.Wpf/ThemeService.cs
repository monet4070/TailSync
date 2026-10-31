using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Media;
using Microsoft.Win32;

namespace TailSync.Wpf;

// Renderer adapter for Core's resolved, data-only Theme V2 contract. Fonts are
// platform fallbacks from the theme, never an independently chosen global face.
public static class ThemeService
{
    private static readonly JsonElement Palettes = LoadPalettes();
    private static JsonElement? _resolved;
    private static int _generation;
    public static string Appearance { get; private set; } = "light";
    public static string ThemeId { get; private set; } = "canvas";
    public static string ActiveThemeId => CanonicalId(ThemeId);
    public static bool HighContrast { get; private set; }
    public static string EffectiveMode => Appearance == "dark" || Appearance == "system" && IsSystemDark() ? "dark" : "light";
    public static event EventHandler? Changed;

    private static JsonElement LoadPalettes()
    {
        using var stream = typeof(ThemeService).Assembly.GetManifestResourceStream("TailSync.Wpf.Palettes.json")!;
        using var json = JsonDocument.Parse(stream);
        return json.RootElement.Clone();
    }

    public static string CanonicalId(string id) => id.Contains(':') ? id : "builtin:" + id + "@1";
    private static string ShortId(string id) => id.StartsWith("builtin:", StringComparison.Ordinal) ? id[8..].Split('@')[0] : id;

    // Synchronous fallback for startup and isolated demo checks. Production
    // loading and persistence always go through the Core commands below.
    public static void Apply(string? appearance = null, string? id = null)
    {
        ++_generation;
        if (appearance is "system" or "light" or "dark") Appearance = appearance;
        if (id != null) ThemeId = id;
        var resources = CreateFallback(ThemeId, EffectiveMode);
        if (_resolved is { } resolved && ThemeV2.Text(resolved, "id") == ActiveThemeId && ThemeV2.Text(resolved, "mode") == EffectiveMode)
            Populate(resources, resolved);
        ApplyResources(resources);
    }

    public static async Task LoadAsync(CancellationToken token = default)
    {
        var generation = ++_generation;
        var settings = await NativeBackend.InvokeAsync<JsonElement>("get_local_theme_settings", token: token);
        var id = ThemeV2.Text(settings, "activeThemeId", "builtin:canvas@1");
        var appearance = ThemeV2.Text(settings, "appearance", "system");
        if (appearance is not ("system" or "light" or "dark")) appearance = "system";
        var highContrast = ThemeV2.Bool(settings, "highContrast");
        var mode = appearance == "dark" || appearance == "system" && IsSystemDark() ? "dark" : "light";
        var resolved = await NativeBackend.InvokeAsync<JsonElement>("resolve_theme", new { themeId = id, mode, platform = "windows", highContrast = highContrast || SystemParameters.HighContrast }, token);
        if (generation != _generation || token.IsCancellationRequested) return;
        Appearance = appearance; ThemeId = id; HighContrast = highContrast;
        ApplyResolved(resolved);
        var assets = await ThemeV2Assets.LoadAsync(resolved, null, token);
        if (generation != _generation || token.IsCancellationRequested) return;
        foreach (var (slot, image) in assets)
        {
            var key = ThemeV2Assets.ResourceKey(slot);
            Application.Current.Resources[key] = image;
            Application.Current.Resources[key + "Available"] = true;
        }
        if (assets.Count > 0) Changed?.Invoke(null, EventArgs.Empty);
    }

    public static async Task SelectAsync(string id, CancellationToken token = default)
        => await PersistAsync(Appearance, id, HighContrast, token);

    public static async Task PersistAsync(string appearance, string themeId, bool highContrast, CancellationToken token = default)
    {
        if (appearance is not ("system" or "light" or "dark")) throw new ArgumentException("Invalid appearance", nameof(appearance));
        await NativeBackend.InvokeAsync<JsonElement>("set_local_theme_settings", new { settings = new { activeThemeId = CanonicalId(themeId), appearance, highContrast } }, token);
        await LoadAsync(token);
    }

    public static void ApplyResolved(JsonElement resolved)
    {
        var id = ThemeV2.Text(resolved, "id", ActiveThemeId);
        if (resolved.ValueKind != JsonValueKind.Object || !resolved.TryGetProperty("tokens", out var tokens) || tokens.ValueKind != JsonValueKind.Object)
            throw new InvalidDataException("主题解析结果没有 tokens 对象");
        ThemeId = id; _resolved = resolved.Clone();
        ApplyResources(CreateResources(resolved));
    }

    internal static ResourceDictionary CreateResources(JsonElement resolved)
    {
        var resources = CreateFallback(ThemeV2.Text(resolved, "id", "canvas"), ThemeV2.Text(resolved, "mode", "light"));
        Populate(resources, resolved);
        return resources;
    }

    private static void ApplyResources(ResourceDictionary resources)
    {
        foreach (var slot in ThemeV2Assets.Slots)
        {
            var key = ThemeV2Assets.ResourceKey(slot);
            Application.Current.Resources.Remove(key);
            Application.Current.Resources[key + "Available"] = false;
        }
        foreach (var key in resources.Keys) Application.Current.Resources[key] = resources[key];
        Changed?.Invoke(null, EventArgs.Empty);
    }

    private static ResourceDictionary CreateFallback(string id, string mode)
    {
        var resources = new ResourceDictionary();
        var shortId = ShortId(id);
        if (!Palettes.TryGetProperty(shortId, out var palette)) palette = Palettes.GetProperty("canvas");
        foreach (var color in palette.GetProperty(mode).EnumerateObject()) resources[color.Name + "Brush"] = Paint(color.Value.GetString()!);
        var ui = Font(shortId == "canvas"
            ? new[] { "Noto Sans SC", "Source Han Sans SC", "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", "PingFang SC", "sans-serif" }
            : new[] { "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", "PingFang SC", "sans-serif" });
        var display = Font(shortId switch
        {
            "canvas" => new[] { "Noto Serif SC", "Source Han Serif SC", "STZhongsong", "Songti SC", "STSong", "SimSun", "Iowan Old Style", "Baskerville", "Georgia", "serif" },
            "flux" => new[] { "Bahnschrift", "DengXian", "Avenir Next", "Hiragino Sans GB", "Segoe UI Variable Display", "sans-serif" },
            "aura" => new[] { "Candara", "YouYuan", "Hiragino Maru Gothic ProN", "PingFang SC", "Microsoft YaHei UI", "sans-serif" },
            "mono" => new[] { "Cascadia Mono", "Cascadia Code", "SF Mono", "IBM Plex Mono", "monospace" },
            _ => new[] { "STZhongsong", "Songti SC", "STSong", "SimSun", "Iowan Old Style", "Baskerville", "Georgia", "serif" }
        });
        var reading = shortId switch
        {
            "canvas" => Font(new[] { "Noto Serif SC", "Source Han Serif SC", "Songti SC", "STSong", "STZhongsong", "SimSun", "Georgia", "serif" }),
            "mono" => Font(new[] { "Cascadia Code", "Cascadia Mono", "Microsoft YaHei UI", "SF Mono", "monospace" }),
            _ => ui
        };
        resources["UiFont"] = ui; resources["ReadingFont"] = reading; resources["DisplayFont"] = display;
        resources["HistoryFont"] = reading; resources["SearchFont"] = ui;
        resources["UiFontSize"] = 14d; resources["HistoryFontSize"] = shortId switch { "canvas" => 16d, "mono" => 14d, _ => 15d };
        resources["SearchFontSize"] = shortId == "mono" ? 15d : 16d;
        resources["SectionFontSize"] = shortId switch { "mono" => 21d, "flux" => 22d, "aura" => 23d, _ => 24d };
        resources["UiLineHeight"] = 22d; resources["UiFontWeight"] = FontWeights.Normal;
        resources["DetailFontSize"] = 13d;
        resources["SectionUppercase"] = shortId is "flux" or "mono";
        resources["SettingRowPadding"] = 13d; resources["HistoryRowPadding"] = 13d;
        resources["ControlRadius"] = new CornerRadius(shortId == "mono" ? 3 : 9);
        resources["SurfaceRadius"] = new CornerRadius(shortId == "mono" ? 3 : 10);
        resources["WindowRadius"] = new CornerRadius(shortId == "mono" ? 3 : 10);
        resources["MotionFastMs"] = 160d; resources["MotionSlowMs"] = 420d;
        resources["CardShadowBlur"] = shortId == "mono" ? 0d : 14d; resources["CardShadowDepth"] = shortId == "mono" ? 0d : 2d; resources["CardShadowOpacity"] = shortId == "mono" ? 0d : .04d;
        foreach (var component in new[] { "Search", "History", "Section", "Panel", "Button", "Input", "Toast" })
        foreach (var state in new[] { "Default", "Hover", "Active", "Selected", "Disabled", "Focus" })
        {
            var prefix = "Theme" + component + state;
            resources[prefix + "BackgroundBrush"] = resources[state switch
            {
                "Hover" => "HoverBrush", "Active" => "ActiveBrush", "Selected" => "AccentSoftBrush",
                _ => component is "Search" or "Input" ? "InputBrush" : "SurfaceBrush"
            }];
            resources[prefix + "ForegroundBrush"] = resources[state == "Disabled" ? "TertiaryBrush" : "PrimaryBrush"];
            resources[prefix + "SecondaryTextBrush"] = resources[state == "Disabled" ? "TertiaryBrush" : "SecondaryBrush"];
            resources[prefix + "BorderBrush"] = resources["BorderBrush"];
            resources[prefix + "FocusRingBrush"] = resources["AccentBrush"];
            resources[prefix + "IconBrush"] = resources["SecondaryBrush"];
            resources[prefix + "AccentBrush"] = resources[state == "Hover" ? "AccentHoverBrush" : "AccentBrush"];
            resources[prefix + "Radius"] = new CornerRadius(9);
            resources[prefix + "Padding"] = new Thickness(10);
            resources[prefix + "Spacing"] = 8d;
            resources[prefix + "FontSize"] = component switch { "Search" => resources["SearchFontSize"], "History" => resources["HistoryFontSize"], "Section" => resources["SectionFontSize"], "Toast" => 13d, _ => 14d };
            resources[prefix + "FontWeight"] = component == "Button" || shortId == "canvas" && (component is "History" or "Panel") ? FontWeights.Medium : FontWeights.Normal;
        }
        return resources;
    }

    private static readonly (string Key, string Path)[] Colors =
    {
        ("Accent", "accent/default"), ("AccentHover", "accent/hover"), ("AccentSoft", "accent/soft"), ("OnAccent", "accent/onAccent"),
        ("Canvas", "background/canvas"), ("Surface", "background/surface"), ("Input", "background/input"), ("Hover", "background/hover"), ("Active", "background/active"), ("Raised", "background/raised"), ("Toast", "background/toast"),
        ("Primary", "text/primary"), ("Secondary", "text/secondary"), ("Tertiary", "text/tertiary"), ("ToastText", "text/toast"),
        ("Border", "border/default"), ("StrongBorder", "border/strong"), ("Divider", "border/divider"),
        ("Positive", "status/positive"), ("PositiveSoft", "status/positiveSoft"), ("Warning", "status/warning"), ("WarningSoft", "status/warningSoft"), ("Info", "status/info"), ("InfoSoft", "status/infoSoft")
    };

    private static void Populate(ResourceDictionary resources, JsonElement resolved)
    {
        if (resolved.ValueKind != JsonValueKind.Object || !resolved.TryGetProperty("tokens", out var tokens) || tokens.ValueKind != JsonValueKind.Object) return;
        if (ThemeV2.Number(tokens, "effects/shadow/radius") is { } shadowRadius) resources["CardShadowBlur"] = Math.Clamp(shadowRadius * .2, 0, 18);
        if (ThemeV2.Number(tokens, "effects/shadow/y") is { } shadowDepth) resources["CardShadowDepth"] = Math.Clamp(shadowDepth / 12, 0, 4);
        if (ThemeV2.Number(tokens, "effects/shadow/opacity") is { } shadowOpacity) resources["CardShadowOpacity"] = Math.Clamp(shadowOpacity * .25, 0, .08);
        foreach (var (key, path) in Colors)
            if (ThemeV2.Value(tokens, "colors/" + path) is { ValueKind: JsonValueKind.String } color) resources[key + "Brush"] = Paint(color.GetString()!);
        foreach (var (role, key) in new[] { ("ui", "UiFont"), ("display", "DisplayFont"), ("reading", "ReadingFont") })
            if (ThemeV2.Value(tokens, "typography/" + role + "/families") is { ValueKind: JsonValueKind.Array } families)
                resources[key] = Font(families.EnumerateArray().Where(x => x.ValueKind == JsonValueKind.String).Select(x => x.GetString()!));
        var id = ThemeV2.Text(resolved, "id");
        var displayHistory = ThemeV2.Value(tokens, "typography/history/useDisplayFont");
        var useDisplayHistory = displayHistory is { ValueKind: JsonValueKind.True } || displayHistory == null && id is not ("builtin:flux@1" or "builtin:aura@1" or "builtin:mono@1");
        resources["HistoryFont"] = useDisplayHistory ? resources["DisplayFont"] : resources["ReadingFont"];
        resources["SearchFont"] = ThemeV2.Value(tokens, "typography/search/useDisplayFont") is { ValueKind: JsonValueKind.True } ? resources["DisplayFont"] : resources["UiFont"];
        foreach (var (key, path) in new[] { ("UiFontSize", "typography/ui/size"), ("HistoryFontSize", "typography/history/size"), ("SearchFontSize", "typography/search/size"), ("SectionFontSize", "typography/section/size"), ("UiLineHeight", "typography/ui/lineHeight"), ("SettingRowPadding", "density/control"), ("HistoryRowPadding", "density/row"), ("MotionFastMs", "effects/motion/fast"), ("MotionSlowMs", "effects/motion/slow") })
            if (ThemeV2.Number(tokens, path) is { } number) resources[key] = number;
        if (ThemeV2.Number(tokens, "typography/ui/weight") is { } weight) resources["UiFontWeight"] = FontWeight.FromOpenTypeWeight((int)Math.Clamp(weight, 1, 999));
        resources["SectionUppercase"] = ThemeV2.Value(tokens, "typography/section/uppercase") is { ValueKind: JsonValueKind.True };
        foreach (var (key, path) in new[] { ("ControlRadius", "shape/controlRadius"), ("SurfaceRadius", "shape/surfaceRadius"), ("WindowRadius", "shape/windowRadius") })
            if (ThemeV2.Number(tokens, path) is { } radius) resources[key] = new CornerRadius(radius);
        foreach (var component in new[] { "search", "history", "section", "panel", "button", "input", "toast" })
        foreach (var state in new[] { "default", "hover", "active", "selected", "disabled", "focus" })
        {
            var prefix = "Theme" + char.ToUpperInvariant(component[0]) + component[1..] + char.ToUpperInvariant(state[0]) + state[1..];
            foreach (var field in new[] { "background", "foreground", "secondaryText", "border", "focusRing", "icon", "accent" })
                if (ThemeV2.Value(tokens, $"components/{component}/{state}/{field}") is { ValueKind: JsonValueKind.String } color)
                    resources[prefix + char.ToUpperInvariant(field[0]) + field[1..] + "Brush"] = Paint(color.GetString()!);
            if (ThemeV2.Number(tokens, $"components/{component}/{state}/radius") is { } radius) resources[prefix + "Radius"] = new CornerRadius(radius);
            if (ThemeV2.Number(tokens, $"components/{component}/{state}/padding") is { } padding) resources[prefix + "Padding"] = new Thickness(padding);
            if (ThemeV2.Number(tokens, $"components/{component}/{state}/spacing") is { } spacing) resources[prefix + "Spacing"] = spacing;
            if (ThemeV2.Number(tokens, $"components/{component}/{state}/typography/size") is { } fontSize) resources[prefix + "FontSize"] = fontSize;
            if (ThemeV2.Number(tokens, $"components/{component}/{state}/typography/weight") is { } fontWeight) resources[prefix + "FontWeight"] = FontWeight.FromOpenTypeWeight((int)Math.Clamp(fontWeight, 1, 999));
        }
        resources["DetailFontSize"] = Math.Max(1, (double)resources["UiFontSize"] - 1);
    }

    // Keep the complete theme fallback order, replacing CSS generic families
    // with their Windows equivalents. WPF chooses the first installed family.
    internal static FontFamily Font(IEnumerable<string> families)
    {
        var names = families.Select(x => x.Trim(' ', '\'', '"')).Where(x => x.Length > 0).Select(x => x switch
        {
            "sans-serif" or "system-ui" => "Segoe UI",
            "serif" => "Georgia",
            "monospace" => "Consolas",
            _ => x
        }).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        return new FontFamily(names.Length == 0 ? "Segoe UI, Microsoft YaHei UI" : string.Join(", ", names));
    }

    internal static SolidColorBrush Paint(string value)
    {
        var (color, opacity) = ParsePaint(value);
        return new SolidColorBrush(color) { Opacity = opacity };
    }

    public static (Color Color, double Opacity) ParsePaint(string text)
    {
        if (text == "system") return (SystemColors.HighlightColor, 1);
        if (text.StartsWith('#'))
        {
            // Core uses CSS #RRGGBBAA; WPF ColorConverter expects #AARRGGBB.
            if (text.Length == 9)
                return ((Color)ColorConverter.ConvertFromString(text[..7]), byte.Parse(text[7..], NumberStyles.HexNumber, CultureInfo.InvariantCulture) / 255d);
            return ((Color)ColorConverter.ConvertFromString(text), 1);
        }
        var start = text.IndexOf('(');
        if (start < 0 || !text.EndsWith(')')) throw new FormatException("无效主题颜色: " + text);
        var values = text[(start + 1)..^1].Split(',').Select(x => double.Parse(x.Trim(), CultureInfo.InvariantCulture)).ToArray();
        if (values.Length != 4 || values.Any(double.IsNaN) || values.Take(3).Any(x => x is < 0 or > 255) || values[3] is < 0 or > 1)
            throw new FormatException("无效主题颜色: " + text);
        return (Color.FromRgb((byte)values[0], (byte)values[1], (byte)values[2]), values[3]);
    }

    private static bool IsSystemDark()
    {
        using var key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        return key?.GetValue("AppsUseLightTheme") is int value && value == 0;
    }
}

internal static class ThemeV2
{
    internal static JsonElement? Value(JsonElement root, string path)
    {
        foreach (var part in path.Split('/'))
        {
            if (root.ValueKind != JsonValueKind.Object || !root.TryGetProperty(part, out var value)) return null;
            root = value;
        }
        return root;
    }
    internal static string Text(JsonElement root, string path, string fallback = "") => Value(root, path) is { ValueKind: JsonValueKind.String } value ? value.GetString()! : fallback;
    internal static bool Bool(JsonElement root, string path) => Value(root, path) is { ValueKind: JsonValueKind.True };
    internal static double? Number(JsonElement root, string path) => Value(root, path) is { ValueKind: JsonValueKind.Number } value && value.TryGetDouble(out var number) ? number : null;
}
