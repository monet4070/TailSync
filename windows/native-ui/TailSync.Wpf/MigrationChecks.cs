using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Threading;
using TailSync.Native.Core;

namespace TailSync.Wpf;

// End-to-end checks use a fresh explicitly isolated Rust data directory. No
// clipboard, network, registry startup item, or production database is accessed.
internal static class MigrationChecks
{
    internal static async Task Run(App app, string directory)
    {
        directory = Path.GetFullPath(directory); Directory.CreateDirectory(directory);
        var data = Path.Combine(directory, "data-" + Guid.NewGuid().ToString("N"));
        var checks = new List<string>(); var captures = new List<string>();
        string? failure = null;
        void Check(bool value, string name) { if (!value) throw new InvalidOperationException(name); checks.Add(name); }
        try
        {
            checks.AddRange(await ThemeV2Checks.RunAsync());
            checks.AddRange(await HistoryPreviewChecks.RunAsync());
            Check(AutostartService.IsTailSyncCommand(@"""C:\Program Files\TailSync\TailSync.exe"" --background") && AutostartService.IsTailSyncCommand(@"""F:\TailSync\TailSync.Wpf.exe"" --background"), "autostart recognizes the existing product and WPF executable commands");
            Check(!AutostartService.IsTailSyncCommand("tailsync.exe")&&!AutostartService.IsTailSyncCommand(@"""C:\Tools\custom.exe"""), "autostart leaves unrelated or relative custom commands untouched");
            Check(new BackendException(JsonSerializer.SerializeToElement(new{code="invalid_argument",message_key="error.invalid_argument"})).Message==Localizer.Text("error.invalid_argument"), "stable backend errors use main's localized user-facing messages");
            await NativeBackend.StartAsync(isolatedRoot: data);
            Check(NativeBackend.IsConnected && NativeBackend.IsIsolated, "C# authenticates to the real isolated Rust named pipe");
            AppSession.UseProduction(new NativeHistoryClient());
            await SettingsState.ReloadAsync(); await ThemeService.LoadAsync();
            Check(SettingsState.Current.ValueKind == JsonValueKind.Object, "production settings load from Rust");
            var storage = await NativeBackend.InvokeAsync<JsonElement>("get_storage_status");
            Check(Path.GetFullPath(storage.String("root")).StartsWith(data, StringComparison.OrdinalIgnoreCase), "storage is confined to the dedicated validation data directory");
            await SettingsState.SaveAsync("history_limit", 120);
            Check(SettingsState.Current.Long("history_limit") == 120, "field-intent settings save and canonical reload round-trip");
            var rejected = false;
            try { await NativeBackend.InvokeAsync("update_settings", new { settingsJson = "{\"trusted_peer_keys\":{\"unexpected\":\"bad\"}}" }); }
            catch (BackendException) { rejected = true; }
            Check(rejected, "server-owned settings cannot be written by the frontend");
            Localizer.SetLocale("en"); Check(Localizer.Text("settings.syncEnabled").Contains("sync", StringComparison.OrdinalIgnoreCase), "main's English catalog is embedded");
            Localizer.SetLocale("zh-CN");
            await ThemeService.PersistAsync("light", "builtin:canvas@1", false);
            Check(ThemeService.ActiveThemeId == "builtin:canvas@1", "theme selection persists through the shared Core settings");
            foreach (var id in new[] { "canvas", "flux", "ledger", "aura", "mono" })
            {
                await ThemeService.PersistAsync("light", "builtin:" + id + "@1", false);
                Check(Application.Current.Resources["HistoryFont"] != null && Application.Current.Resources["SearchFont"] != null, id + " resolves main's theme font roles into live WPF resources");
            }
            await ThemeService.PersistAsync("dark", "builtin:flux@1", true);
            Check(ThemeService.HighContrast && ThemeService.EffectiveMode == "dark", "dark and high-contrast preference round-trip");
            await ThemeService.PersistAsync("light", "builtin:canvas@1", false);

            var v1 = await ThemeV2Packages.ValidateAsync(ThemePackageFixture.Create(directory, "1.0.0"));
            var installed = await ThemeV2Packages.ApplyAsync(v1);
            Check(installed.String("id") == ThemePackageFixture.ThemeId, "real theme package validates all four modes and installs into the isolated Core store");
            await ThemeService.SelectAsync(ThemePackageFixture.ThemeId);
            Check(((System.Windows.Media.FontFamily)Application.Current.Resources["HistoryFont"]).Source.Contains(ThemePackageFixture.DisplayFont) && (double)Application.Current.Resources["UiFontSize"] == 14, "custom theme font families and sizes reach the live renderer");
            Check(Application.Current.Resources["ThemeLogoImageAvailable"] is true, "theme PNG asset crosses binary IPC and decodes in WPF");
            var v2 = await ThemeV2Packages.ValidateAsync(ThemePackageFixture.Create(directory, "1.1.0"), ThemePackageFixture.ThemeId, "1.0.0");
            var updated = await ThemeV2Packages.ApplyAsync(v2);
            Check(updated.String("version") == "1.1.0", "custom theme update retains the validated digest and version");
            var rollback = await NativeBackend.InvokeAsync<JsonElement>("rollback_theme", new { themeId = ThemePackageFixture.ThemeId });
            Check(rollback.String("version") == "1.0.0", "custom theme rollback restores its prior installed version");
            await ThemeService.SelectAsync("builtin:canvas@1");
            await NativeBackend.InvokeAsync("delete_theme_v2", new { themeId = ThemePackageFixture.ThemeId, storageHandle = rollback.String("storageHandle") });
            var catalog = await NativeBackend.InvokeAsync<JsonElement>("list_themes_v2");
            Check(!catalog.Items().Any(x => x.String("id") == ThemePackageFixture.ThemeId), "custom theme deletion removes the selected storage handle only");

            await NativeBackend.InvokeAsync("seed_wpf_test_history", new { count = 65 });
            var client = new NativeHistoryClient();
            var first = await client.GetPageAsync(new(), 30, 0);
            Check(first.Entries.Count == 30 && first.HasMore, "real encrypted DB returns a bounded 30-row first page");
            var second = await client.GetPageAsync(new(), 30, 30);
            Check(second.Entries.Count == 30 && !first.Entries.Select(x => x.Id).Intersect(second.Entries.Select(x => x.Id)).Any(), "second history page has distinct records");
            var target = first.Entries[0];
            await client.SetFavoriteAsync(target.Id, true);
            Check((await client.GetPageAsync(new(Favorites: true), 30, 0)).Entries.Any(x => x.Id == target.Id), "favorite mutation is visible in the independent collection");
            rejected = false; try { await client.DeleteAsync(target.Id, false); } catch (BackendException) { rejected = true; }
            Check(rejected, "history deletion preserves the backend's favorite protection");
            var binary = await NativeBackend.InvokeBinaryAsync("get_preview", new { id = target.Id });
            Check(binary.Length >= 12 && binary.AsSpan(0, 4).SequenceEqual("TSPV"u8), "binary preview crosses the pipe without base64 JSON expansion");

            foreach (var page in new[] { "history", "favorites", "settings", "connections" })
            {
                var window = new MainWindow(page); window.Show();
                await Task.Delay(300); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
                var titleLabel=QualityChecks.Descendants((DependencyObject)window.FindName("TitleHost")).OfType<TextBlock>().FirstOrDefault(label=>System.Windows.Data.BindingOperations.GetBindingExpression(label,TextBlock.TextProperty)?.ParentBinding.Source==window);
                Check(titleLabel!=null&&titleLabel.Text==window.Title,page+": native title bar follows the current translated window title");
                if (page == "history")
                {
                    var list = (ListBox)window.FindName("HistoryList");
                    Check(list.Items.Count > 0, "production history binds real backend entries");
                    var filter = (ComboBox)window.FindName("CategoryFilter");
                    var supported = (await NativeBackend.InvokeAsync<JsonElement>("get_history_capabilities")).Object("categories").Items().Select(item => item.GetString()).ToHashSet();
                    var options = filter.Items.OfType<ComboBoxItem>().ToArray();
                    Check(options.All(item => item.IsEnabled && (item.Tag as string == "all" || supported.Contains(item.Tag as string)))
                        && options.All(item => item.Tag as string is not ("command" or "structured_data")),
                        "history categories show only supported choices, without disabled legacy aliases");
                    var search = (TextBox)window.FindName("SearchBox");
                    search.Text = "definitely-no-fixture-record"; await Task.Delay(450);
                    Check(window.ViewModel.Entries.Count == 0, "debounced production search applies its latest query");
                    search.Clear(); await Task.Delay(450);
                    Check(window.ViewModel.Entries.Count == 30, "clearing search returns to the first bounded page");
                    Check(VirtualizingPanel.GetScrollUnit(list) == ScrollUnit.Pixel && VirtualizingPanel.GetIsVirtualizing(list), "production history retains pixel scrolling and virtualization");
                }
                QualityChecks.Capture(window, Path.Combine(directory, page + ".png")); captures.Add(page + ".png");
                window.Width = window.MinWidth; window.UpdateLayout(); await Task.Delay(100);
                foreach (var row in QualityChecks.Descendants(window).OfType<AdaptiveSettingRow>().Where(x => x.IsVisible && x.Children.Count == 2))
                {
                    var action = (FrameworkElement)row.Children[1];
                    var bounds = action.TransformToAncestor(row).TransformBounds(new Rect(action.RenderSize));
                    Check(bounds.Left >= -.5 && bounds.Right <= row.ActualWidth + .5, page + ": settings controls fit at minimum window width");
                }
                QualityChecks.Capture(window, Path.Combine(directory, page + "-narrow.png")); captures.Add(page + "-narrow.png");
                window.Close();
            }
            await client.ClearHistoryAsync();
            var retained = await client.GetPageAsync(new(), 30, 0);
            Check(retained.Entries.Count == 1 && retained.Entries[0].Id == target.Id, "clearing history retains favorites");
            await client.DeleteAsync(target.Id, true);
            Check((await client.GetPageAsync(new(Favorites: true), 30, 0)).Entries.Count == 0, "removing a favorite uses the dedicated favorite mutation");
            await NativeBackend.StopAsync();
            await NativeBackend.StartAsync(isolatedRoot: data);
            await SettingsState.ReloadAsync(); await ThemeService.LoadAsync();
            Check(SettingsState.Current.Long("history_limit") == 120 && ThemeService.ActiveThemeId == "builtin:canvas@1", "settings and theme survive a backend restart");
        }
        catch (Exception error) { failure = error.ToString(); }
        finally { await NativeBackend.StopAsync(); }
        await File.WriteAllTextAsync(Path.Combine(directory, "migration.json"), JsonSerializer.Serialize(new
        {
            passed = failure == null, referenceMain = "b61a0a04b88f5828707f7ef64781389944471be8", checks, failure, captures, isolatedData = data,
            limitations = new[] { "No real clipboard mutation, device pairing, migration of user storage or update installation is performed by these checks.", "PNG files are live WPF visual-tree renders; physical multi-DPI transitions are a separate acceptance check." }
        }, new JsonSerializerOptions { WriteIndented = true }));
        app.Shutdown(failure == null ? 0 : 1);
    }
}
