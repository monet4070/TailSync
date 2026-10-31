using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Threading;
using TailSync.Native.Core;

namespace TailSync.Wpf;

internal static class ClipboardOnboardingChecks
{
    internal static async Task Run(App app, string directory)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        var checks = new List<string>(); string? failure = null;
        void Check(bool value, string label) { if (!value) throw new InvalidOperationException(label); checks.Add(label); }
        async Task Until(Func<bool> predicate)
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
            while (!predicate()) await Task.Delay(25, timeout.Token);
        }
        async Task Reject(Func<Task> action, string label)
        {
            var rejected = false;
            try { await action(); } catch (Exception e) when (e is InvalidOperationException or BackendException) { rejected = true; }
            Check(rejected, label);
        }
        try
        {
            await NativeBackend.StartAsync(isolatedRoot: Path.Combine(directory, "profile-" + Guid.NewGuid().ToString("N")));
            Check(NativeBackend.IsIsolated && !NativeBackend.NetworkEnabled, "real encrypted Rust backend has no clipboard or network integration");
            await NativeBackend.InvokeAsync("seed_wpf_test_history", new { count = 65 });
            var native = new NativeHistoryClient(); var router = new TourHistoryClient(native); AppSession.UseProduction(router);
            await SettingsState.ReloadAsync(); await SettingsState.SaveAsync("language", "zh-CN");
            await SettingsState.SaveAsync("auto_save_network_images", true);
            Check((await NativeBackend.InvokeAsync<JsonElement>("get_settings")).Bool("auto_save_network_images"), "WPF settings patch persists automatic network-image saving");
            await ThemeService.LoadAsync();
            await GuidedTour.InitializeAsync(false);
            var windows = new[] { "connections", "history", "favorites", "settings" }.Select(page =>
                new MainWindow(page) { ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.Manual,
                    Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100, Top = 50 }).ToArray();
            foreach (var window in windows) window.Show();
            var history = windows.Single(w => w.Page == "history");
            await Until(() => history.ViewModel.Entries.Count == 30 && history.ViewModel.IsQueryCurrent);
            Check(history.ViewModel.VisibleEntries.Take(6).All(e => e.ShortcutNumber > 0), "actual WPF history exposes six numbered records");
            Check(QualityChecks.Descendants(windows.Single(w => w.Page == "settings")).OfType<ToggleButton>().Any(b => System.Windows.Automation.AutomationProperties.GetName(b).Contains("网络图片")), "network-image setting is present in the real WPF settings page");
            await GuidedTour.StartAsync();
            await Until(() => history.ViewModel.Entries.Count == 17 && history.ViewModel.IsQueryCurrent);
            Check(history.ViewModel.Entries.All(e => TourHistoryClient.IsVirtual(e.Id)), "starting the guide replaces production rows with shared bundled examples");
            GuidedTour.Move(4); history.Activate();
            var list = (ListBox)history.FindName("HistoryList");
            list.SelectedIndex = 0; list.ScrollIntoView(list.SelectedItem); list.UpdateLayout();
            var row = (ListBoxItem)list.ItemContainerGenerator.ContainerFromIndex(0); row.Focus();
            var key = new KeyEventArgs(Keyboard.PrimaryDevice, PresentationSource.FromVisual(row), 0, Key.D2) { RoutedEvent = Keyboard.PreviewKeyDownEvent };
            row.RaiseEvent(key); await Dispatcher.Yield();
            Check(key.Handled && history.ViewModel.SelectedEntry?.Id == 8_000_000_002, "actual focused WPF list routes number two to its committed second row");
            var search = (TextBox)history.FindName("SearchBox"); search.Focus();
            var selected = history.ViewModel.SelectedEntry?.Id;
            var typing = new KeyEventArgs(Keyboard.PrimaryDevice, PresentationSource.FromVisual(search), 0, Key.D1) { RoutedEvent = Keyboard.PreviewKeyDownEvent };
            search.RaiseEvent(typing);
            Check(history.ViewModel.SelectedEntry?.Id == selected && !typing.Handled, "typing numbers in WPF search never restores history");
            Check((await native.GetPageAsync(new(), 30, 0)).Total == 65, "tour examples never enter the production history database");
            var demo = history.ViewModel.Entries[0];
            var restored = await router.RestoreAsync(demo.Id);
            Check(restored.Success && restored.Message.Contains("未更改"), "demonstration restore reports simulation without writing the clipboard");
            await router.SetFavoriteAsync(demo.Id, true);
            Check((await router.GetPageAsync(new(Favorites: true), 30, 0)).Entries.Any(e => e.Id == demo.Id), "guide favorites are an in-memory subset");
            await Reject(() => router.RestoreAsync(1), "stale production rows cannot be restored during the guide");
            await Reject(() => NativeBackend.InvokeAsync("enable_pairing"), "guide cannot initiate real pairing");
            await Reject(() => NativeBackend.InvokeAsync("get_preview", new { id = demo.Id }), "virtual preview IDs cannot cross the native pipe");
            await Reject(() => NativeBackend.InvokeAsync("update_settings", new { settingsJson = "{\"sync_enabled\":false}" }), "guide controls cannot change production settings");
            var clipboardRejected = false; try { GuidedTour.CheckClipboard(); } catch (InvalidOperationException) { clipboardRejected = true; }
            Check(clipboardRejected, "preview copy is blocked while the guide is active");
            foreach (var locale in new[] { "zh-CN", "en" })
            {
                Localizer.SetLocale(locale);
                for (var i = 0; i < GuidedTour.Count; i++)
                {
                    GuidedTour.Move(i - GuidedTour.Step);
                    await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
                    var owner = windows.Single(w => w.Page == GuidedTour.Current.String("window"));
                    Check(owner.TourTarget(GuidedTour.Current.String("target")) is { IsVisible: true }, $"{locale}: live native anchor for step {i + 1}");
                    Check(QualityChecks.Descendants(owner).OfType<Button>().Any(b => b.Content as string == GuidedTour.Copy(i == GuidedTour.Count - 1 ? "finish" : "next")), $"{locale}: navigation remains available for step {i + 1}");
                    if (i is 0 or 4 or 10 or 13)
                        QualityChecks.Capture(owner, Path.Combine(directory, $"{locale}-step-{i + 1}.png"));
                }
            }
            foreach (var id in new long[] { 8_000_000_008, 8_000_000_010, 8_000_000_011, 8_000_000_012, 8_000_000_013 })
            {
                var payload = TourHistoryClient.Preview(id, null);
                if (payload.Renderer == "docx") Check(PreviewRenderers.Docx(payload.Data).Blocks.Count > 0, "bundled Word bytes retain the native DOCX renderer");
                else if (TourHistoryClient.Document(id).ValueKind == JsonValueKind.Object)
                    Check(PreviewRenderers.BundledDocument(TourHistoryClient.Document(id)).Blocks.Count > 0, "bundled PowerPoint/rich document uses the structured document sample");
                var preview = new PreviewWindow(new HistoryEntry { Id = id, Title = payload.Name }, () => {})
                { Owner = history, ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.Manual, Left = history.Left, Top = 50 };
                preview.Show();
                if (payload.Renderer is "pdf" or "image")
                    await Until(() => QualityChecks.Descendants(preview).OfType<Image>().Any(image => image.Source != null && image.Source.Width > 0));
                else
                    await Until(() => QualityChecks.Descendants(preview).OfType<FlowDocumentScrollViewer>().Any(view => view.Document?.Blocks.Count > 0));
                Check(true, $"sample {payload.Name}: existing native preview completes without production IPC");
                if (payload.Renderer == "docx" || payload.Name.EndsWith(".pptx")) QualityChecks.Capture(preview, Path.Combine(directory, payload.Name + ".png"));
                preview.Close();
            }
            var savedAppearance = ThemeService.Appearance;
            ThemeService.Apply(appearance: "dark");
            GuidedTour.Move(4 - GuidedTour.Step); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
            QualityChecks.Capture(history, Path.Combine(directory, "en-history-dark.png"));
            Check((await NativeBackend.InvokeAsync<JsonElement>("get_local_theme_settings")).String("appearance", "system") == savedAppearance,
                "tour theme preview changes no persisted preference");
            var oldWidth = history.Width; var oldHeight = history.Height;
            history.Width = history.MinWidth; history.Height = history.MinHeight;
            await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
            var navigation = QualityChecks.Descendants(history).OfType<Button>().Single(b => b.Content as string == GuidedTour.Copy("next"));
            var box = navigation.TransformToVisual(history).TransformBounds(new Rect(navigation.RenderSize));
            Check(box.Top >= 0 && box.Bottom <= history.ActualHeight && box.Right <= history.ActualWidth,
                "tour navigation stays inside the minimum Windows history viewport");
            QualityChecks.Capture(history, Path.Combine(directory, "minimum-layout-diagnostic.png"));
            Check(list.ActualHeight >= 60, $"minimum tour viewport retains an interactive history row (height={list.ActualHeight}, window={history.ActualHeight})");
            QualityChecks.Capture(history, Path.Combine(directory, "en-history-minimum.png"));
            history.Width = oldWidth; history.Height = oldHeight;
            var report = NativeHistoryClient.RestoreReport(JsonSerializer.SerializeToElement(new { complete = false, missing_images = 2 }));
            Check(report.Success && !report.Complete && report.Message.Contains('2'), "partial restoration preserves missing-image feedback and does not close history");
            await GuidedTour.EndAsync();
            await Until(() => history.ViewModel.Entries.Count == 30 && history.ViewModel.IsQueryCurrent);
            Check(ThemeService.Appearance == savedAppearance, "ending the guide restores the persisted appearance");
            Check(history.ViewModel.Entries.All(e => !TourHistoryClient.IsVirtual(e.Id)), "ending the tour clears the demo snapshot and reloads real rows");
            Check(!await NativeBackend.InvokeAsync<bool>("onboarding_should_show"), "ordinary dismissal survives native relaunch");
            await Reject(() => router.DeleteAsync(demo.Id, false), "expired virtual delete cannot affect production");
            await Reject(() => router.RestoreAsync(demo.Id), "expired virtual restore cannot affect production");
            await Reject(() => NativeBackend.InvokeAsync("get_image_data", new { id = demo.Id }), "expired virtual thumbnails cannot cross IPC");
            await GuidedTour.StartAsync();
            await Until(() => history.ViewModel.Entries.Count == 17);
            Check((await router.GetPageAsync(new(Favorites: true), 30, 0)).Entries.All(e => e.Id != demo.Id), "restarting the guide resets simulated mutations");
            await GuidedTour.EndAsync(true);
            Check(GuidedTour.Suppressed && await NativeBackend.InvokeAsync<bool>("onboarding_suppressed"), "permanent suppression uses the installation-scoped Rust marker");
            Check(!QualityChecks.Descendants(windows.Single(w => w.Page == "settings")).OfType<Button>().Any(b => b.Content as string == "Getting started"), "permanent suppression removes the settings restart entry");
            foreach (var window in windows) window.Close();
        }
        catch (Exception error) { failure = error.ToString(); }
        finally
        {
            if (GuidedTour.Active) try { await GuidedTour.EndAsync(); } catch { }
            foreach (var window in app.Windows.Cast<Window>().ToArray()) window.Close();
            await NativeBackend.StopAsync();
        }
        await File.WriteAllTextAsync(Path.Combine(directory, "clipboard-onboarding.json"), JsonSerializer.Serialize(new { passed = failure == null, checks, failure }, new JsonSerializerOptions { WriteIndented = true }));
        app.Shutdown(failure == null ? 0 : 1);
    }
}
