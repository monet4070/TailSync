using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using TailSync.Native.Core;

namespace TailSync.Wpf;

internal static class ThumbnailChecks
{
    internal static async Task Run(App app, string directory, bool review = false)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        var checks = new List<string>(); string? failure = null;
        void Check(bool value, string label) { if (!value) throw new InvalidOperationException(label); checks.Add(label); }
        async Task Until(Func<bool> predicate)
        {
            var watch = Stopwatch.StartNew();
            while (!predicate()) { if (watch.ElapsedMilliseconds > 15000) throw new TimeoutException("Thumbnail display did not settle."); await Task.Delay(25); }
        }
        try
        {
            var calls = 0; var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            JsonElement Pixels(string key, string pixels = "/wAA/w==") => JsonSerializer.SerializeToElement(new { thumbnail_b64 = pixels, thumbnail_width = 1, thumbnail_height = 1, cache_key = key });
            var composite = new HistoryEntry { Id = 7, Type = "composite", ThumbnailEligible = true, DisplayCacheKey = "snapshot:a" };
            var cache = new HistoryThumbnails(async (_, key, _) => { calls++; await release.Task; return Pixels(key!); });
            var first = cache.GetAsync(composite, default);
            var replacement = cache.GetAsync(new HistoryEntry { Id = 7, Type = "composite", ThumbnailEligible = true, DisplayCacheKey = "snapshot:a" }, default);
            release.SetResult(); var bytes = await first;
            Check(calls == 1 && ReferenceEquals(first, replacement) && bytes is { Length: > 0 }, "replaced composite rows share one pending thumbnail request");
            var converted = new HistoryThumbnailConverter().Convert(bytes!, typeof(ImageSource), null!, CultureInfo.InvariantCulture) as BitmapSource;
            Check(converted is { IsFrozen: true }, "thumbnail PNG reaches the WPF image converter and freezes for display");
            var mismatch = new HistoryThumbnails((_, _, _) => Task.FromResult(Pixels("snapshot:old")));
            Check(await mismatch.GetAsync(composite, default) == null, "stale snapshot responses cannot paint the current record");
            var malformed = new HistoryThumbnails((_, key, _) => Task.FromResult(Pixels(key!, "AA==")));
            Check(await malformed.GetAsync(composite, default) == null, "incomplete thumbnail pixels leave the fallback icon intact");
            var attempts = 0;
            var retry = new HistoryThumbnails((_, key, _) => ++attempts < 3
                ? Task.FromException<JsonElement>(new BackendException(JsonSerializer.SerializeToElement(new { code = "temporarily_busy", retryable = true, message_key = "error.temporarily_busy" })))
                : Task.FromResult(Pixels(key!)));
            Check(await retry.GetAsync(composite, default) != null && attempts == 3, "temporary analysis pressure retries and recovers the thumbnail");
            var gate = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            var invalidated = new HistoryThumbnails(async (_, key, _) => { await gate.Task; return Pixels(key!); });
            var pending = invalidated.GetAsync(composite, default); invalidated.Clear(); gate.SetResult();
            Check(await pending == null, "clearing a collection invalidates late thumbnail work");
            foreach (var (width, height) in new[] { (80, 20), (20, 80), (80, 80) })
            {
                var pixels = new byte[width * height * 4]; for (var i = 0; i < pixels.Length; i += 4) { pixels[i + 1] = 180; pixels[i + 3] = 255; }
                var thumbnail = new RoundedThumbnail { Width = 44, Height = 44, CornerRadius = new(8), Source = BitmapSource.Create(width, height, 96, 96, PixelFormats.Bgra32, null, pixels, width * 4) };
                thumbnail.Measure(new Size(44, 44)); thumbnail.Arrange(new Rect(0, 0, 44, 44)); thumbnail.UpdateLayout();
                var bitmap = new RenderTargetBitmap(44, 44, 96, 96, PixelFormats.Pbgra32); bitmap.Render(thumbnail);
                var rendered = new byte[44 * 44 * 4]; bitmap.CopyPixels(rendered, 44 * 4, 0);
                var bounds = thumbnail.FittedBounds; var x = (int)Math.Ceiling(bounds.Left + .5); var y = (int)Math.Ceiling(bounds.Top + .5);
                Check(rendered[(y * 44 + x) * 4 + 3] < 5 && rendered[(22 * 44 + 22) * 4 + 3] > 250,
                    $"{width}x{height}: fitted thumbnail corners are transparent while the image center remains visible");
            }

            await NativeBackend.StartAsync(isolatedRoot: Path.Combine(directory, "profile-" + Guid.NewGuid().ToString("N")));
            AppSession.UseProduction(new NativeHistoryClient());
            await NativeBackend.InvokeAsync("seed_wpf_thumbnail_history"); await SettingsState.ReloadAsync();
            await SettingsState.SaveAsync("language", "zh-CN"); await ThemeService.PersistAsync("light", "builtin:canvas@1", false);
            var window = new MainWindow("history") { ShowActivated = false, ShowInTaskbar = false,
                WindowStartupLocation = WindowStartupLocation.Manual, Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100, Top = 50 };
            window.Show(); await Until(() => window.ViewModel.Entries.Count == 4);
            var filters = new[] { (ComboBox)window.FindName("CategoryFilter"), (ComboBox)window.FindName("DateFilter") };
            Check(filters.All(filter => filter.Background is SolidColorBrush background && background.Opacity > 0 && background.Color.A > 0
                && filter.BorderBrush is SolidColorBrush border && border.Opacity > 0 && border.Color.A > 0), "unselected history filters retain a visible surface and boundary");
            var images = window.ViewModel.Entries.Where(entry => entry.ThumbnailEligible).ToArray();
            Check(images.Length == 3 && images.Any(entry => entry.Type == "image") && images.Count(entry => entry.Type == "composite") == 2,
                "Core display metadata enables ordinary images, composite images and illustrated documents");
            await Until(() => images.All(entry => entry.ThumbnailBytes is { Length: > 0 }));
            window.UpdateLayout();
            foreach (var entry in images)
            {
                Check(QualityChecks.Descendants(window).OfType<Image>().Any(image => image.Name == "ThumbnailImage" && ReferenceEquals(image.DataContext, entry) && image.Source is BitmapSource && image.ActualWidth > 0),
                    "real encrypted " + entry.Type + "/" + entry.DisplayKind + " thumbnail crosses IPC and reaches its visible image");
                Check(QualityChecks.Descendants(window).OfType<RoundedThumbnail>().Any(image => ReferenceEquals(image.DataContext, entry) && !image.FittedBounds.IsEmpty && image.CornerRadius.TopLeft > 0),
                    "real " + entry.Type + "/" + entry.DisplayKind + " thumbnail uses the fitted round-corner renderer");
            }
            foreach (var entry in new[] { images.First(item => item.Type == "image"), window.ViewModel.Entries.First(item => item.Type == "text") })
            {
                var preview = new PreviewWindow(entry, () => { }) { Owner = window, ShowActivated = false, ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.Manual, Left = window.Left, Top = 50 };
                preview.Show();
                await Until(() => entry.Type == "image"
                    ? QualityChecks.Descendants(preview).OfType<Image>().Any(image => image.Source is BitmapSource && image.IsVisible)
                    : QualityChecks.Descendants(preview).OfType<ICSharpCode.AvalonEdit.TextEditor>().Any(editor => editor.Text.Length > 0));
                await Task.Delay(60); preview.UpdateLayout();
                if (entry.Type == "image") Check(!QualityChecks.Descendants(preview).OfType<Button>().Any(button => System.Windows.Automation.AutomationProperties.GetName(button) == Localizer.Text("history.preview.wrap") && button.IsVisible), "image preview hides the inapplicable text-wrap action");
                QualityChecks.Capture(preview, Path.Combine(directory, "preview-" + entry.Type + ".png"));
                preview.Width = preview.MinWidth; preview.UpdateLayout(); QualityChecks.Capture(preview, Path.Combine(directory, "preview-" + entry.Type + "-narrow.png"));
                preview.Close();
            }
            var list = (ListBox)window.FindName("HistoryList"); var selected = images.First(entry => entry.Type == "composite");
            list.SelectedItem = selected; window.UpdateLayout();
            var container = (ListBoxItem)list.ItemContainerGenerator.ContainerFromItem(selected); container.Focus(); window.UpdateLayout();
            Check(list.FocusVisualStyle == null && container.FocusVisualStyle == null, "list and item do not add a second frame around the date header");
            var chrome = QualityChecks.Descendants(container).OfType<Border>().Single(border => border.Name == "EntryChrome");
            Check(chrome.BorderThickness == new Thickness(1) && QualityChecks.Descendants(container).OfType<Border>().Any(border => border.Name == "SelectionMark" && border.IsVisible), "selection uses a contained fill and a single side indicator");
            var description = QualityChecks.Descendants(container).OfType<TextBlock>().Single(text => text.Name == "EntryDescription");
            Check(description.FontSize == (double)app.Resources["HistoryFontSize"] && description.FontFamily.Source == ((FontFamily)app.Resources["HistoryFont"]).Source,
                "history text preserves the theme history font and size");
            QualityChecks.Capture(window, Path.Combine(directory, "history-light.png"));
            await window.ViewModel.RefreshAsync(); await Until(() => window.ViewModel.Entries.Where(entry => entry.ThumbnailEligible).All(entry => entry.ThumbnailBytes != null));
            Check(window.ViewModel.Entries.Any(entry => entry.Type == "composite" && entry.ThumbnailBytes != null), "re-querying the same page preserves thumbnail loading for replaced row objects");
            var target = QualityChecks.Descendants(window).OfType<Image>().First(image => image.Name == "ThumbnailImage");
            var current = window.ViewModel.Entries.First(entry => entry.ThumbnailEligible);
            var rebind = new HistoryEntry { Id = current.Id, Type = current.Type, ThumbnailEligible = true, DisplayCacheKey = current.DisplayCacheKey, DataHash = current.DataHash };
            target.DataContext = rebind; await Until(() => rebind.ThumbnailBytes != null);
            Check(target.Source != null, "a recycled image loads its new DataContext without a second Loaded event"); target.ClearValue(FrameworkElement.DataContextProperty);
            await ThemeService.PersistAsync("dark", "builtin:canvas@1", false); window.UpdateLayout();
            var primary = (SolidColorBrush)Ui.Brush("Primary");
            Check(QualityChecks.Descendants(window).OfType<TextBlock>().Where(text => text.Name == "EntryDescription").All(text => text.Foreground is SolidColorBrush brush && brush.Color == primary.Color),
                "dark history rows inherit the current readable foreground");
            QualityChecks.Capture(window, Path.Combine(directory, "history-dark.png"));
            window.Width = window.MinWidth; window.UpdateLayout(); QualityChecks.Capture(window, Path.Combine(directory, "history-narrow.png"));
            if (review)
            {
                await ThemeService.PersistAsync("light", "builtin:canvas@1", false);
                NativeBackend.BeginWatching();
                window.Width = 400; window.Title = "历史样例 · 隔离验证";
                var area = SystemParameters.WorkArea; window.Left = area.Left + Math.Max(24, (area.Width - window.Width) / 2); window.Top = area.Top + 40;
                window.ShowInTaskbar = true; window.Activate();
                var closed = new TaskCompletionSource(); window.Closed += (_, _) => closed.TrySetResult();
                await File.WriteAllTextAsync(Path.Combine(directory, "review-ready.json"), JsonSerializer.Serialize(new { passed = true, pid = Environment.ProcessId, sampleRows = 4, isolated = true }));
                await closed.Task;
            }
            else window.Close();
        }
        catch (Exception error) { failure = error.ToString(); }
        finally { foreach (Window window in app.Windows.Cast<Window>().ToArray()) window.Close(); await NativeBackend.StopAsync(); }
        await File.WriteAllTextAsync(Path.Combine(directory, "thumbnails.json"), JsonSerializer.Serialize(new { passed = failure == null, checks, failure,
            boundary = "Real encrypted fixture rows, actual Core image/composite analysis, authenticated IPC and WPF Image.Source. Render-tree captures do not prove monitor ClearType quality." }, new JsonSerializerOptions { WriteIndented = true }));
        app.Shutdown(failure == null ? 0 : 1);
    }
}
