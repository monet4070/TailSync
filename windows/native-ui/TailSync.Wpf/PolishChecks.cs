using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using System.Windows.Threading;

namespace TailSync.Wpf;

internal static class PolishChecks
{
    internal static async Task Run(App app, string directory)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        var checks = new List<string>(); string? failure = null;
        void Check(bool value, string message) { if (!value) throw new InvalidOperationException(message); checks.Add(message); }
        try
        {
            await NativeBackend.StartAsync(isolatedRoot: Path.Combine(directory, "data-" + Guid.NewGuid().ToString("N")));
            AppSession.UseProduction(new NativeHistoryClient());
            await NativeBackend.InvokeAsync("seed_wpf_test_history", new { count = 65 });
            await SettingsState.ReloadAsync(); await SettingsState.SaveAsync("language", "zh-CN"); await ThemeService.PersistAsync("light", "builtin:canvas@1", false);
            await ControlDesignChecks.Run(directory, Check);
            foreach (var locale in new[] { "zh-CN", "en" })
            {
                await SettingsState.SaveAsync("language", locale);
                foreach (var page in new[] { "settings", "connections" })
                {
                    var window = new MainWindow(page) { ShowActivated = false, ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.Manual, Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100, Top = SystemParameters.VirtualScreenTop + 50 }; window.Show(); await Task.Delay(400);
                    Check(!window.AllowsTransparency && window.UseLayoutRounding
                        && TextOptions.GetTextFormattingMode(window) == TextFormattingMode.Display
                        && TextOptions.GetTextRenderingMode(window) == TextRenderingMode.ClearType
                        && RenderOptions.GetClearTypeHint(window) == ClearTypeHint.Enabled,
                        $"{locale}/{page}: opaque window retains pixel-aligned ClearType text");
                    foreach (var text in QualityChecks.Descendants(window).OfType<TextBlock>().Where(x => x.IsVisible && x.IsEnabled))
                    {
                        for (DependencyObject? ancestor = text; ancestor != null; ancestor = VisualTreeHelper.GetParent(ancestor))
                            Check(ancestor is not UIElement element || element.Effect == null && element.CacheMode == null,
                                $"{locale}/{page}: text is outside effect and bitmap-cache layers");
                    }
                    var original = window.Width;
                    foreach (var width in new[] { original, window.MinWidth })
                    {
                        window.Width = width; window.UpdateLayout();
                        var cards = QualityChecks.Descendants(window).OfType<WrapPanel>().FirstOrDefault(panel => panel.Name == "ThemeCards");
                        if (cards != null)
                            Check(cards.Children.Count > 0 && cards.Children.OfType<FrameworkElement>().All(card => card.ActualWidth >= 200
                                && card.TransformToAncestor(cards).TransformBounds(new Rect(card.RenderSize)).Right <= cards.ActualWidth + 1),
                                $"{locale}/settings/{width}: theme cards fill responsive columns without horizontal clipping");
                        var connection = QualityChecks.Descendants(window).OfType<ConnectionsView>().FirstOrDefault();
                        connection?.RenderFixtureForChecks(DeviceFixture);
                        window.UpdateLayout();
                        if (page == "connections" && locale == "zh-CN" && width == original)
                            await File.WriteAllLinesAsync(Path.Combine(directory, "connection-fonts.txt"), QualityChecks.Descendants(window).OfType<TextBlock>().Where(text => text.IsVisible).Select(text => $"{text.Text}: {text.FontFamily.Source}; {text.FontSize}"));
                        foreach (var row in QualityChecks.Descendants(window).OfType<AdaptiveSettingRow>().Where(x => x.IsVisible && x.Children.Count == 2))
                        {
                            var action = (FrameworkElement)row.Children[1]; var bounds = action.TransformToAncestor(row).TransformBounds(new Rect(action.RenderSize));
                            Check(bounds.Left >= -.5 && bounds.Right <= row.ActualWidth + .5 && bounds.Bottom <= row.ActualHeight + .5, $"{locale}/{page}/{width}: row action fits");
                        }
                        foreach (var button in QualityChecks.Descendants(window).OfType<Button>().Where(x => x.IsVisible && x.Content is FrameworkElement))
                        {
                            var content = (FrameworkElement)button.Content; var bounds = content.TransformToAncestor(button).TransformBounds(new Rect(content.RenderSize));
                            Check(bounds.Left >= -.5 && bounds.Right <= button.ActualWidth + .5 && bounds.Bottom <= button.ActualHeight + .5, $"{locale}/{page}/{width}: button content fits");
                        }
                        QualityChecks.Capture(window, Path.Combine(directory, $"{page}-{locale}-{width}.png"));
                        if (page == "connections" && locale == "zh-CN" && width == original)
                        {
                            static IEnumerable<string> Faces(Drawing? drawing)
                            {
                                if (drawing is GlyphRunDrawing glyph) yield return glyph.GlyphRun.GlyphTypeface.FontUri.ToString();
                                if (drawing is DrawingGroup group) foreach (var child in group.Children) foreach (var face in Faces(child)) yield return face;
                            }
                            await File.WriteAllLinesAsync(Path.Combine(directory, "connection-glyph-fonts.txt"), QualityChecks.Descendants(window).OfType<TextBlock>().Where(text => text.IsVisible).Select(text => text.Text + ": " + string.Join(", ", Faces(VisualTreeHelper.GetDrawing(text)).Distinct())));
                        }
                    }
                    window.Width = original;
                    if (page == "settings")
                    {
                        var viewer = (ScrollViewer)window.FindName("SettingsPage"); viewer.ScrollToVerticalOffset(480); await Task.Delay(80);
                        QualityChecks.Capture(window, Path.Combine(directory, $"settings-storage-{locale}.png"));
                        var themes = QualityChecks.Descendants(window).OfType<WrapPanel>().First(panel => panel.Name == "ThemeCards");
                        themes.BringIntoView(); await Task.Delay(100);
                        QualityChecks.Capture(window, Path.Combine(directory, $"settings-themes-{locale}.png"));
                        window.Width = window.MinWidth; window.UpdateLayout(); themes.BringIntoView(); await Task.Delay(100);
                        QualityChecks.Capture(window, Path.Combine(directory, $"settings-themes-{locale}-narrow.png"));
                    }
                    else
                    {
                        var expander = QualityChecks.Descendants(window).OfType<Expander>().First(item => item.Content is Panel content && content.Children.OfType<TextBox>().Any());
                        Check(!expander.IsExpanded, "remote pairing starts collapsed"); expander.IsExpanded = true; window.UpdateLayout();
                        await Task.Delay(SystemParameters.ClientAreaAnimation ? 380 : 1);
                        Check(QualityChecks.Descendants(expander).OfType<TextBox>().Any(box => box.IsVisible && box.ActualHeight > 0), "remote pairing drawer reveals editable controls after its transition");
                        QualityChecks.Capture(window, Path.Combine(directory, $"connections-expanded-{locale}.png"));
                    }
                    window.Close();
                }
            }
            await SettingsState.SaveAsync("language", "zh-CN");
            var history = new MainWindow("history") { ShowActivated = false, ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.Manual, Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100, Top = SystemParameters.VirtualScreenTop + 50 }; history.Show(); await Task.Delay(350);
            var entry = history.ViewModel.Entries.First(x => !x.Pinned);
            var list = (ListBox)history.FindName("HistoryList"); list.ScrollIntoView(entry); history.UpdateLayout();
            history.BeginFavoritePress(entry); await Task.Delay(100); history.CancelPress(); await Task.Delay(600);
            Check(!entry.Pinned && entry.FavoriteProgress == 0, "ordinary click cancels before the 220ms grace period; no write");
            history.BeginFavoritePress(entry); await Task.Delay(400);
            var fill = QualityChecks.Descendants(list).OfType<FavoriteFeedback>().First(x => !x.Stamp && x.DataContext == entry);
            Check(entry.FavoriteProgress == 1 && !entry.Pinned, "charge starts after grace without prematurely changing favorite state");
            if (SystemParameters.ClientAreaAnimation) Check(fill.VisibleAmount > 0 && fill.VisibleAmount < 1, "full-row fill interpolates between endpoints");
            QualityChecks.Capture(history, Path.Combine(directory, "favorite-charging.png"));
            history.CancelPress(); await Task.Delay(500);
            Check(!entry.Pinned && entry.FavoriteProgress == 0 && fill.VisibleAmount == 0, "canceling the charge clears its fill and performs no write");
            history.BeginFavoritePress(entry);
            for (var i = 0; i < 25 && !entry.Pinned; i++) await Task.Delay(40);
            Check(entry.Pinned && entry.FavoriteCompletion == 1, "640ms hold commits through real backend and starts completion stamp");
            await Task.Delay(100); QualityChecks.Capture(history, Path.Combine(directory, "favorite-complete.png"));
            Check(history.ViewModel.Entries.Contains(entry), "completed row survives the stamp animation before refresh");
            var favorites = await NativeBackend.InvokeAsync<JsonElement>("get_history_page", new { collection = "favorites", limit = 30, offset = 0 });
            Check(favorites.Object("entries").Items().Any(item => item.Long("id") == entry.Id), "favorite persists before the completion animation finishes");
            history.BeginFavoritePress(entry);
            await Task.Delay(400);
            Check(entry.Pinned && entry.FavoriteProgress == 1, "unfavorite hold keeps the committed favorite until its gesture completes");
            if (SystemParameters.ClientAreaAnimation) Check(fill.VisibleAmount > 0 && fill.VisibleAmount < 1, "unfavorite hold visibly drains the existing fill instead of remaining full");
            QualityChecks.Capture(history, Path.Combine(directory, "unfavorite-charging.png"));
            for (var i = 0; i < 25 && entry.Pinned; i++) await Task.Delay(40);
            Check(!entry.Pinned && entry.FavoriteCompletion == -1, "a second hold during completion survives the deferred refresh and unfavorites");
            await Task.Delay(100); QualityChecks.Capture(history, Path.Combine(directory, "favorite-release.png"));
            await File.WriteAllTextAsync(Path.Combine(directory, "favorite-release-state.json"), JsonSerializer.Serialize(QualityChecks.Descendants(list).OfType<FavoriteFeedback>().Where(x => x.DataContext == entry)
                .Select(x => new { x.Stamp, x.Pinned, x.Charge, x.Completion, x.IsLoaded, x.IsVisible, x.VisibleAmount, x.VisibleRemoval })));
            if (SystemParameters.ClientAreaAnimation)
                Check(QualityChecks.Descendants(list).OfType<FavoriteFeedback>().Any(x => x.Stamp && x.DataContext == entry && x.VisibleAmount > 0 && x.VisibleRemoval > 0 && x.VisibleRemoval < 1),
                    "unfavorite confirmation lifts and fades its star through visible intermediate frames");
            await Task.Delay(600); Check(!history.ViewModel.Entries.First(x => x.Id == entry.Id).Pinned, "unfavorite persists after feedback finishes");
            Check((await AppSession.Client.SetFavoriteAsync(entry.Id, true)).Success, "cancelled-unfavorite fixture starts from a real persisted favorite");
            await history.ViewModel.RefreshAsync(); entry = history.ViewModel.Entries.First(x => x.Id == entry.Id); list.ScrollIntoView(entry); history.UpdateLayout(); await Task.Delay(200);
            fill = QualityChecks.Descendants(list).OfType<FavoriteFeedback>().First(x => !x.Stamp && x.DataContext == entry);
            history.BeginFavoritePress(entry); await Task.Delay(400); history.CancelPress(); await Task.Delay(250);
            Check(entry.Pinned && entry.FavoriteProgress == 0 && fill.VisibleAmount == 1, "releasing an unfinished unfavorite restores its fill without removing the favorite");
            favorites = await NativeBackend.InvokeAsync<JsonElement>("get_history_page", new { collection = "favorites", limit = 30, offset = 0 });
            Check(favorites.Object("entries").Items().Any(item => item.Long("id") == entry.Id), "cancelled unfavorite performs no backend removal");
            history.BeginFavoritePress(history.ViewModel.Entries[0]); history.Close(); await Task.Delay(700);
            Check(!history.IsVisible, "closing the window cancels an active hold");
        }
        catch (Exception error) { failure = error.ToString(); }
        finally { foreach (Window window in app.Windows.Cast<Window>().ToArray()) window.Close(); await NativeBackend.StopAsync(); }
        await File.WriteAllTextAsync(Path.Combine(directory, "polish.json"), JsonSerializer.Serialize(new { passed = failure == null, checks, failure, limitation = "Live WPF renders and dispatcher-driven gesture checks; device rows use synthetic fixtures. No physical pointer, network pairing or multi-monitor DPI acceptance." }, new JsonSerializerOptions { WriteIndented = true }));
        app.Shutdown(failure == null ? 0 : 1);
    }
    internal static JsonElement DeviceFixture => JsonSerializer.SerializeToElement(new
    {
        self = new { hostname = "DESKTOP-V62KRAF", fingerprint = "554D-5FBF-49E7-8D08-A132", iroh_endpoint_id = "4fcae1061e65b3695e8d0b14f15b054e47980c86ada519a43e4d2528eefeb409" },
        peers = new[] { new { hostname = "monet的MacBook-Air.local", fingerprint = "8150-1D2E-37B8-594B-9B4E", trusted = true, enabled = true, online = true, current_interface = "iroh", routes = new[] { new { address = "f33a6650b73f0460166e9d1586883c535622ddcd314a314ba2b65a1c672611c8", @interface = "iroh", connected = true, online = true, rtt_capable = true } } } }
    });
}
