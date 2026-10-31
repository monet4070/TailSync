using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Input;
using System.Windows.Threading;

namespace TailSync.Wpf;

internal static class TextRenderingChecks
{
    internal static async Task Run(App app, string directory)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        var checks = new List<string>(); var observations = new List<object>(); string? failure = null;
        void Check(bool condition, string label) { if (!condition) throw new InvalidOperationException(label); checks.Add(label); }
        async Task Until(Func<bool> predicate)
        {
            var watch = Stopwatch.StartNew();
            while (!predicate()) { if (watch.ElapsedMilliseconds > 10000) throw new TimeoutException("Text layout did not settle."); await Task.Delay(25); }
        }
        try
        {
            foreach (var scale in new[] { 1d, 1.25, 1.5, 1.75, 2, 2.5, 3 })
            foreach (var offset in new[] { .37, 17.21, 123.678, 899.13 })
            {
                var aligned = SmoothScroll.PixelOffset(offset, scale, 900.17);
                Check(Math.Abs(aligned * scale - Math.Round(aligned * scale)) < .000001 && aligned >= 0 && aligned <= 900.17,
                    $"{scale * 100}%: scroll quantization retains complete device pixels at {offset}");
            }
            await NativeBackend.StartAsync(isolatedRoot: Path.Combine(directory, "profile-" + Guid.NewGuid().ToString("N")));
            AppSession.UseProduction(new NativeHistoryClient()); await NativeBackend.InvokeAsync("seed_wpf_test_history", new { count = 65 });
            await SettingsState.ReloadAsync(); await SettingsState.SaveAsync("language", "zh-CN");
            await ThemeService.PersistAsync("light", "builtin:canvas@1", false);
            var window = new MainWindow("history") { ShowActivated = false, ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.Manual,
                Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100, Top = 50 };
            window.Show(); await Until(() => window.ViewModel.Entries.Count > 0);
            var list = (ListBox)window.FindName("HistoryList");
            var viewer = QualityChecks.Descendants(list).OfType<ScrollViewer>().First();
            var search = (TextBox)window.FindName("SearchBox");
            var dpi = VisualTreeHelper.GetDpi(window);
            var settings = new MainWindow("settings") { ShowActivated = false, ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.Manual,
                Left = window.Left, Top = 50 };
            settings.Show(); await Until(() => QualityChecks.Descendants(settings).OfType<TextBlock>()
                .SelectMany(text => text.Inlines.OfType<System.Windows.Documents.Run>()).Any(run => run.Text.StartsWith("当前版本")));
            foreach (var theme in new[] { "canvas", "flux", "ledger", "aura", "mono" })
            {
                await ThemeService.PersistAsync("light", "builtin:" + theme + "@1", false);
                window.UpdateLayout(); settings.UpdateLayout(); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
                var body = QualityChecks.Descendants(window).OfType<TextBlock>().First(text => text.Name == "EntryDescription");
                Check(body.FontFamily.Source == ((FontFamily)app.Resources["HistoryFont"]).Source && body.FontSize == (double)app.Resources["HistoryFontSize"],
                    theme + ": history family and size are the committed theme resources");
                Check(body.FontWeight == (FontWeight)app.Resources["ThemeHistoryDefaultFontWeight"], theme + ": history weight follows the resolved theme component");
                Check(search.FontFamily.Source == ((FontFamily)app.Resources["SearchFont"]).Source && search.FontSize == (double)app.Resources["SearchFontSize"],
                    theme + ": search family and size are the committed theme resources");
                Check(ReferenceEquals(app.Resources["HistoryFont"], app.Resources["ReadingFont"]) && ReferenceEquals(app.Resources["SearchFont"], app.Resources["UiFont"]),
                    theme + ": expressive title typography is separate from reading and search roles");
                if (theme == "canvas")
                    Check(body.FontFamily.Source.StartsWith("Noto Serif SC, Source Han Serif SC") && body.FontSize == 16,
                        "Canvas reading typography resolves its serif candidates and 16px size from Core");
                Check(TextOptions.GetTextFormattingMode(body) == TextFormattingMode.Display && TextOptions.GetTextRenderingMode(body) == TextRenderingMode.ClearType
                    && TextOptions.GetTextHintingMode(body) == TextHintingMode.Fixed && RenderOptions.GetClearTypeHint(body) == ClearTypeHint.Enabled,
                    theme + ": leaf text has Display, ClearType, fixed hinting and an explicit local ClearType hint");
                foreach (var offset in new[] { 17.37, 123.21, 0 })
                {
                    viewer.ScrollToVerticalOffset(offset); await Task.Delay(60); window.UpdateLayout();
                    Check(Math.Abs(viewer.VerticalOffset * dpi.DpiScaleY - Math.Round(viewer.VerticalOffset * dpi.DpiScaleY)) < .0001,
                        theme + ": real scroll viewer settles on a device pixel after a fractional request");
                }
                foreach (var text in QualityChecks.Descendants(window).OfType<TextBlock>().Where(text => text.IsVisible && text.IsEnabled))
                    for (DependencyObject? parent = text; parent != null; parent = VisualTreeHelper.GetParent(parent))
                        Check(parent is not UIElement visual || visual.Effect == null && visual.CacheMode == null, theme + ": glyphs remain outside bitmap effects and caches");
                static IEnumerable<GlyphRun> Runs(Drawing? drawing)
                {
                    if (drawing is GlyphRunDrawing glyph) yield return glyph.GlyphRun;
                    if (drawing is DrawingGroup group) foreach (var child in group.Children) foreach (var run in Runs(child)) yield return run;
                }
                var actualFaces = Runs(VisualTreeHelper.GetDrawing(body)).Select(run => new { file = run.GlyphTypeface.FontUri.ToString(),
                    weight = run.GlyphTypeface.Weight.ToOpenTypeWeight(), missingGlyph = run.GlyphIndices.Any(index => index == 0) }).ToArray();
                Check(actualFaces.Length > 0 && actualFaces.All(face => !face.missingGlyph), theme + ": actual history glyph runs contain no missing glyphs");
                if (theme == "canvas" && new FontFamily("Noto Serif SC").FamilyNames.Values.Contains("Noto Serif SC"))
                    Check(actualFaces.Any(face => face.file.Contains("NOTOSERIF", StringComparison.OrdinalIgnoreCase) && face.weight == 500), "Canvas uses real Noto Serif 500 glyphs rather than a synthetic or regular-weight substitute");
                var settingLabel = QualityChecks.Descendants(settings).OfType<TextBlock>().First(text => text.Text == Localizer.Text("settings.syncEnabled"));
                var settingFaces = Runs(VisualTreeHelper.GetDrawing(settingLabel)).Select(run => new { file = run.GlyphTypeface.FontUri.ToString(), weight = run.GlyphTypeface.Weight.ToOpenTypeWeight() }).ToArray();
                if (theme == "canvas")
                    Check(settingFaces.Length > 0 && settingFaces.All(face => face.file.Contains("NOTOSANS", StringComparison.OrdinalIgnoreCase) && face.weight == 500),
                        "Canvas Chinese setting labels resolve to real Noto Sans 500 glyphs");
                var metadata = QualityChecks.Descendants(window).OfType<TextBlock>().Where(text => text.Name is "EntrySize" or "EntrySource" or "EntryTime").ToArray();
                Check(metadata.Length > 0 && metadata.All(text => text.FontSize == (double)app.Resources["DetailFontSize"]
                    && Contrast(text.Foreground, Ui.Brush("Surface"), Ui.Brush("Surface"), Ui.Brush("Canvas")) >= 4.5),
                    theme + ": small history metadata uses the readable theme size and contrast");
                observations.Add(new { theme, family = body.FontFamily.Source, size = body.FontSize, weight = body.FontWeight.ToOpenTypeWeight(), actualFaces,
                    settingFaces, detailSize = app.Resources["DetailFontSize"], searchFamily = search.FontFamily.Source, searchSize = search.FontSize,
                    dpi = dpi.DpiScaleX, textHintInherited = RenderOptions.ClearTypeHintProperty.GetMetadata(typeof(TextBlock)) is FrameworkPropertyMetadata { Inherits: true } });
                QualityChecks.Capture(window, Path.Combine(directory, theme + ".png"));
                settings.UpdateLayout(); QualityChecks.Capture(settings, Path.Combine(directory, "settings-" + theme + ".png"));
                await ThemeService.PersistAsync("dark", "builtin:" + theme + "@1", false); window.UpdateLayout(); settings.UpdateLayout();
                Check(metadata.All(text => Contrast(text.Foreground, Ui.Brush("Surface"), Ui.Brush("Surface"), Ui.Brush("Canvas")) >= 4.5),
                    theme + ": dark metadata retains readable contrast");
                Check(body.FontFamily.Source == ((FontFamily)app.Resources["HistoryFont"]).Source && body.FontSize == (double)app.Resources["HistoryFontSize"],
                    theme + ": dark appearance retains the same reading hierarchy");
                var neutralButtons = QualityChecks.Descendants(settings).OfType<Button>().Where(button => button.IsVisible && button.IsEnabled
                    && ReferenceEquals(button.Background, app.Resources["ThemeButtonDefaultBackgroundBrush"])).ToArray();
                Check(neutralButtons.Length > 0 && neutralButtons.All(button => button.Template.FindName("Chrome", button) is Border chrome
                    && Contrast(button.Foreground, chrome.Background, Ui.Brush("Surface"), Ui.Brush("Canvas")) >= 4.5),
                    theme + ": neutral actions retain readable contrast immediately after switching appearance");
                var version = QualityChecks.Descendants(settings).OfType<TextBlock>().SelectMany(text => text.Inlines.OfType<System.Windows.Documents.Run>())
                    .First(run => run.Text.StartsWith("当前版本"));
                Check(Contrast(version.Foreground, Ui.Brush("Surface"), Ui.Brush("Surface"), Ui.Brush("Canvas")) >= 4.5,
                    theme + ": asynchronous version text follows the active theme foreground");
                QualityChecks.Capture(window, Path.Combine(directory, theme + "-dark.png"));
                QualityChecks.Capture(settings, Path.Combine(directory, "settings-" + theme + "-dark.png"));
            }
            var frameOffsets = new List<double>();
            void Frame(object? sender, EventArgs args) => frameOffsets.Add(viewer.VerticalOffset);
            CompositionTarget.Rendering += Frame;
            try
            {
                list.RaiseEvent(new MouseWheelEventArgs(Mouse.PrimaryDevice, Environment.TickCount, -120) { RoutedEvent = UIElement.PreviewMouseWheelEvent });
                await Task.Delay(450);
            }
            finally { CompositionTarget.Rendering -= Frame; }
            Check(viewer.VerticalOffset > 0 && !SmoothScroll.IsAnimating(list), "pixel alignment preserves wheel motion and releases its render subscription");
            Check(frameOffsets.All(offset => Math.Abs(offset * dpi.DpiScaleY - Math.Round(offset * dpi.DpiScaleY)) < .0001),
                "wheel animation frames also retain complete device-pixel offsets");
            var afterWheel = viewer.VerticalOffset;
            list.RaiseEvent(new MouseWheelEventArgs(Mouse.PrimaryDevice, Environment.TickCount, 120) { RoutedEvent = UIElement.PreviewMouseWheelEvent });
            await Task.Delay(450);
            Check(viewer.VerticalOffset < afterWheel && !SmoothScroll.IsAnimating(list), "reversing the wheel preserves motion and crisp final placement");
            await ThemeService.PersistAsync("light", "builtin:canvas@1", false);
            var displayWindow = new MainWindow("history") { ShowActivated = false, ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.CenterScreen };
            displayWindow.Show(); await Until(() => displayWindow.ViewModel.Entries.Count > 0); await Task.Delay(120); displayWindow.UpdateLayout();
            var displayDpi = VisualTreeHelper.GetDpi(displayWindow);
            var displayList = (ListBox)displayWindow.FindName("HistoryList");
            var displayViewer = QualityChecks.Descendants(displayList).OfType<ScrollViewer>().First();
            displayViewer.ScrollToVerticalOffset(17.37); await Task.Delay(80); displayWindow.UpdateLayout();
            Check(Math.Abs(displayViewer.VerticalOffset * displayDpi.DpiScaleY - Math.Round(displayViewer.VerticalOffset * displayDpi.DpiScaleY)) < .0001,
                "history on the actual desktop monitor retains complete physical-pixel scroll offsets");
            observations.Add(new { theme = "canvas-actual-display", dpi = displayDpi.DpiScaleX, family = ((FontFamily)app.Resources["HistoryFont"]).Source,
                size = app.Resources["HistoryFontSize"], weight = ((FontWeight)app.Resources["ThemeHistoryDefaultFontWeight"]).ToOpenTypeWeight() });
            QualityChecks.Capture(displayWindow, Path.Combine(directory, "canvas-actual-display.png")); displayWindow.Close();
            ThemeService.Apply("light", "canvas");
            var custom = JsonSerializer.SerializeToElement(new { id = "custom:text-rendering-check", mode = "light", tokens = new { typography = new {
                ui = new { families = new[] { "Segoe UI" } }, display = new { families = new[] { "Georgia" } }, reading = new { families = new[] { "Consolas" } },
                history = new { size = 17 }, search = new { size = 21, useDisplayFont = true }, section = new { size = 25 } },
                components = new { history = new { @default = new { typography = new { weight = 600 } } }, panel = new { @default = new { typography = new { weight = 700 } } } } } });
            ThemeService.ApplyResolved(custom); window.UpdateLayout();
            var customBody = QualityChecks.Descendants(window).OfType<TextBlock>().First(text => text.Name == "EntryDescription");
            Check(customBody.FontFamily.Source == "Georgia" && customBody.FontSize == 17 && search.FontFamily.Source == "Georgia" && search.FontSize == 21,
                "custom theme families and exact sizes survive the rendering optimizations");
            Check(customBody.FontWeight == FontWeights.SemiBold && QualityChecks.Descendants(settings).OfType<TextBlock>().First(text => text.Text == Localizer.Text("settings.syncEnabled")).FontWeight == FontWeights.Bold,
                "custom theme history and panel weights retain precedence over builtin typography");
            QualityChecks.Capture(window, Path.Combine(directory, "custom.png"));
            window.Close(); settings.Close();
        }
        catch (Exception error) { failure = error.ToString(); }
        finally { foreach (Window window in app.Windows.Cast<Window>().ToArray()) window.Close(); await NativeBackend.StopAsync(); }
        await File.WriteAllTextAsync(Path.Combine(directory, "text-rendering.json"), JsonSerializer.Serialize(new { passed = failure == null, checks, observations, failure,
            boundary = "Theme and WPF surface/scroll checks on the current monitor. Other DPI values test offset math, not physical monitors. Render-tree PNGs do not measure physical subpixel quality." }, new JsonSerializerOptions { WriteIndented = true }));
        app.Shutdown(failure == null ? 0 : 1);
    }

    private static double Contrast(Brush foreground, Brush background, Brush surface, Brush canvas)
    {
        static double[] Over(Brush paint, double[] behind)
        {
            var solid = (SolidColorBrush)paint; var color = solid.Color; var alpha = color.A / 255d * solid.Opacity;
            return new[] { color.R / 255d, color.G / 255d, color.B / 255d }.Select((value, i) => value * alpha + behind[i] * (1 - alpha)).ToArray();
        }
        static double Luminance(double[] rgb)
        {
            var linear = rgb.Select(value => value <= .04045 ? value / 12.92 : Math.Pow((value + .055) / 1.055, 2.4)).ToArray();
            return .2126 * linear[0] + .7152 * linear[1] + .0722 * linear[2];
        }
        var back = Over(background, Over(surface, Over(canvas, new[] { 1d, 1d, 1d })));
        var first = Luminance(Over(foreground, back)); var second = Luminance(back);
        return (Math.Max(first, second) + .05) / (Math.Min(first, second) + .05);
    }
}
