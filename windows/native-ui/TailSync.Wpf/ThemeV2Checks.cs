using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;

namespace TailSync.Wpf;

// No IPC, file install, or global theme-setting writes: these checks exercise
// the renderer boundary and the same four-mode validation gate used by import.
internal static class ThemeV2Checks
{
    internal static async Task<IReadOnlyList<string>> RunAsync()
    {
        var checks = new List<string>();
        void Check(bool value, string label) { if (!value) throw new InvalidOperationException(label); checks.Add(label); }
        var paint = ThemeService.ParsePaint("#11223380");
        Check(paint.Color == Color.FromRgb(0x11, 0x22, 0x33) && Math.Abs(paint.Opacity - 128 / 255d) < .00001, "CSS alpha comes after RGB, rather than before it");
        Check(Math.Abs(ThemeService.ParsePaint("rgba(26, 25, 22, 0.045)").Opacity - .045) < .000001, "fractional alpha preserves palette precision");
        var resources = ThemeService.CreateResources(JsonSerializer.SerializeToElement(new
        {
            id = "custom:specimen", mode = "dark", tokens = new
            {
                colors = new { accent = new { @default = "#123456" } },
                typography = new { ui = new { families = new[] { "Segoe UI", "sans-serif" }, size = 15 }, display = new { families = new[] { "Georgia", "serif" } }, reading = new { families = new[] { "Consolas", "monospace" } }, search = new { size = 22, useDisplayFont = true } },
                shape = new { controlRadius = 6 },
                components = new { button = new { hover = new { background = "#34567880", radius = 4 } } }
            }
        }));
        Check(((SolidColorBrush)resources["AccentBrush"]).Color == Color.FromRgb(0x12, 0x34, 0x56), "resolved Core colors override fallback colors");
        Check(((FontFamily)resources["HistoryFont"]).Source == "Georgia" && ((FontFamily)resources["SearchFont"]).Source == "Georgia", "custom history and search use the resolved display font role");
        Check(((FontFamily)resources["ReadingFont"]).Source == "Consolas", "CSS generic font names map to Windows families without reordering");
        var explicitReading = ThemeService.CreateResources(JsonSerializer.SerializeToElement(new { id = "custom:reading", tokens = new { typography = new {
            display = new { families = new[] { "Georgia" } }, reading = new { families = new[] { "Consolas" } }, history = new { useDisplayFont = false } } } }));
        Check(((FontFamily)explicitReading["HistoryFont"]).Source == "Consolas", "an explicit history role takes precedence over the legacy custom theme role");
        Check((double)resources["UiFontSize"] == 15 && (double)resources["SearchFontSize"] == 22 && (CornerRadius)resources["ControlRadius"] == new CornerRadius(6), "resolved typography and shape map to native values");
        Check(((SolidColorBrush)resources["ThemeButtonHoverBackgroundBrush"]).Color == Color.FromRgb(0x34, 0x56, 0x78) && (CornerRadius)resources["ThemeButtonHoverRadius"] == new CornerRadius(4), "component hover colors and radius remain structured native resources");
        var manager = new ThemeManagerView(false);
        manager.SetDescriptors(new[] { JsonSerializer.SerializeToElement(new
        {
            id = "invalid:broken", storageHandle = "broken", source = "custom", status = "invalid", name = new { en = "Broken package" },
            resolvedLight = (object?)null, resolvedDark = (object?)null, diagnostics = Array.Empty<object>()
        }) });
        var cards = ((StackPanel)manager.Children[0]).Children.OfType<WrapPanel>().First();
        var invalidCard = (StackPanel)cards.Children[0];
        Check(!invalidCard.Children.OfType<Button>().Single().IsEnabled && invalidCard.Children.OfType<WrapPanel>().Single().Children.OfType<Button>().Any(), "invalid theme with JSON-null previews remains visible and deletable without activating it");
        Check(invalidCard.Children.OfType<Button>().Single().ToolTip == null, "an empty diagnostics array cannot create a blank hover popup");
        manager.SetDescriptors(new[] {
            JsonSerializer.SerializeToElement(new { id = "builtin:ledger@1", source = "builtin", status = "valid", name = new { en = "Ledger" }, diagnostics = Array.Empty<object>() }),
            JsonSerializer.SerializeToElement(new { id = "custom:warning", source = "custom", status = "valid", name = new { en = "Warning" }, diagnostics = new[] { new { severity = "warning", code = "fixture", message = "Tooltip diagnostic fixture" } } })
        });
        var themeButtons = ((StackPanel)manager.Children[0]).Children.OfType<WrapPanel>().First().Children.OfType<StackPanel>().Select(card => card.Children.OfType<Button>().Single()).ToArray();
        Check(themeButtons[0].ToolTip == null && themeButtons[1].ToolTip is string warning && warning.Contains("Tooltip diagnostic fixture"),
            "healthy builtin cards omit tooltips while real diagnostic messages remain available");
        Check(ThemeService.CreateResources(JsonSerializer.SerializeToElement<object?>(null))["CanvasBrush"] is SolidColorBrush, "missing resolved preview safely uses fallback resources");
        ThemeManagerView? rejectedManager = null;
        var disabledBeforePersist = false;
        rejectedManager = new ThemeManagerView(true, (_, _, _, _) =>
        {
            disabledBeforePersist = !((StackPanel)rejectedManager!.Children[0]).IsEnabled;
            return Task.FromException(new InvalidOperationException("Isolated persistence failure"));
        }, CancellationToken.None);
        RadioButton[] Modes() => ((UniformGrid)((StackPanel)rejectedManager.Children[0]).Children.OfType<Border>().First().Child).Children.OfType<RadioButton>().ToArray();
        var originalMode = ThemeService.Appearance;
        Modes()[originalMode == "dark" ? 1 : 2].IsChecked = true;
        Check(disabledBeforePersist, "theme mutations disable the full input region before waiting for persistence");
        Check(Modes()[originalMode == "system" ? 0 : originalMode == "light" ? 1 : 2].IsChecked == true, "failed appearance persistence restores the canonical radio selection");
        var contrast = ((StackPanel)rejectedManager.Children[0]).Children.OfType<CheckBox>().Single();
        contrast.IsChecked = !ThemeService.HighContrast; contrast.RaiseEvent(new RoutedEventArgs(ButtonBase.ClickEvent));
        Check(((StackPanel)rejectedManager.Children[0]).Children.OfType<CheckBox>().Single().IsChecked == ThemeService.HighContrast, "failed contrast persistence restores the canonical checkbox state");
        var detachedPersisted = false;
        var detachedManager = new ThemeManagerView(true, (_, _, _, _) => { detachedPersisted = true; return Task.CompletedTask; });
        var detachedModes = ((UniformGrid)((StackPanel)detachedManager.Children[0]).Children.OfType<Border>().First().Child).Children.OfType<RadioButton>().ToArray();
        detachedModes[originalMode == "dark" ? 1 : 2].IsChecked = true;
        Check(!detachedPersisted, "detached production theme controls cannot start an uncancelled persistence request");
        foreach (var id in new[] { "canvas", "flux", "ledger", "aura", "mono" })
        {
            var fallback = ThemeService.CreateResources(JsonSerializer.SerializeToElement(new { id = "builtin:" + id + "@1", mode = "light" }));
            Check(ReferenceEquals(fallback["HistoryFont"], fallback["ReadingFont"]), id + " fallback uses the revised builtin reading role");
        }
        foreach (var (candidate, installed, expected) in new[]
        {
            ("2.0.0", "1.9.9", "upgrade"), ("1.0.0+build2", "1.0.0+build1", "same"),
            ("1.0.0-rc.10", "1.0.0-rc.2", "upgrade"), ("1.0.0", "1.0.0-rc.9", "upgrade"),
            ("1.0.0-beta", "1.0.0", "downgrade"), ("1.0.0-1", "1.0.0-alpha", "downgrade"),
            ("1.0.0-alpha", "1.0.0-alpha.1", "downgrade"), ("1.0.0-99999999999999999999", "1.0.0-100000000000000000000", "downgrade")
        }) Check(ThemeV2Packages.CompareVersions(candidate, installed) == expected, $"SemVer {candidate} vs {installed}: {expected}");
        var invalidRejected = false;
        try { ThemeV2Packages.CompareVersions("1.0.0-01", "1.0.0"); } catch (FormatException) { invalidRejected = true; }
        Check(invalidRejected, "SemVer rejects numeric prerelease leading zeros");

        JsonElement Validation(string digest, string id, string mode, bool contrast) => JsonSerializer.SerializeToElement(new
        {
            valid = true, digest, candidateVersion = "1.1.0", preview = new { id, mode, highContrast = contrast, tokens = new { } }, diagnostics = Array.Empty<object>()
        });
        var calls = new List<(string, bool)>();
        var pending = await ThemeV2Packages.ValidateAsync("isolated-fixture.tailsync-theme", "custom:specimen", "1.0.0", validate: (_, mode, contrast, _) =>
        {
            calls.Add((mode, contrast)); return Task.FromResult(Validation("same-digest", "custom:specimen", mode, contrast));
        });
        Check(calls.Count == 4 && calls.Distinct().Count() == 4 && pending.Previews.Count == 4 && pending.VersionRelation == "upgrade", "import validates light, dark, and both high-contrast modes before applying");
        async Task<bool> Rejected(Func<string, bool, JsonElement> value)
        {
            try { await ThemeV2Packages.ValidateAsync("fixture", "custom:specimen", "1.0.0", validate: (_, mode, contrast, _) => Task.FromResult(value(mode, contrast))); return false; }
            catch (InvalidOperationException) { return true; }
        }
        Check(await Rejected((mode, contrast) => Validation(contrast ? "changed-file" : "same-file", "custom:specimen", mode, contrast)), "import rejects digest changes across preview modes");
        Check(await Rejected((mode, contrast) => Validation("same-file", "custom:other", mode, contrast)), "update rejects an unexpected package theme id");
        return checks;
    }
}
