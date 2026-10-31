using System.IO;
using System.Text.Json;
using System.Windows;

namespace TailSync.Wpf;

// A live review of the production views with an isolated backend/profile.
internal static class SettingsReview
{
    internal static async Task Run(App app, string directory)
    {
        directory = Path.GetFullPath(directory); Directory.CreateDirectory(directory);
        app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        try
        {
            await NativeBackend.StartAsync(isolatedRoot: Path.Combine(directory, "profile"));
            AppSession.UseProduction(new NativeHistoryClient());
            await SettingsState.ReloadAsync();
            await SettingsState.SaveAsync("language", "zh-CN");
            await ThemeService.PersistAsync("light", "builtin:canvas@1", false);
            NativeBackend.BeginWatching();
            var settings = MainWindow.Open("settings");
            var connections = MainWindow.Open("connections");
            settings.Title = "设置 · 预览"; connections.Title = "连接与设备 · 预览";
            var area = SystemParameters.WorkArea;
            var availableWidth = area.Width - 48;
            if (settings.Width + connections.Width + 20 > availableWidth)
            {
                settings.Width = Math.Max(settings.MinWidth, (availableWidth - 20) / 2);
                connections.Width = Math.Max(connections.MinWidth, (availableWidth - 20) / 2);
            }
            var left = Math.Max(area.Left + 12, area.Left + (area.Width - settings.Width - connections.Width - 20) / 2);
            settings.Left = left; settings.Top = area.Top + 24;
            connections.Left = Math.Min(area.Right - connections.Width - 12, left + settings.Width + 20);
            connections.Top = area.Top + 24;
            await Task.Delay(600);
            QualityChecks.Descendants(connections).OfType<ConnectionsView>().FirstOrDefault()
                ?.RenderFixtureForChecks(PolishChecks.DeviceFixture);
            settings.UpdateLayout(); connections.UpdateLayout();
            QualityChecks.Capture(settings, Path.Combine(directory, "settings.png"));
            QualityChecks.Capture(connections, Path.Combine(directory, "connections.png"));
            await File.WriteAllTextAsync(Path.Combine(directory, "review.json"), JsonSerializer.Serialize(new
            {
                passed = true, pid = Environment.ProcessId, profile = "isolated", deviceRows = "synthetic",
                settings = new { settings.Width, settings.Height }, connections = new { connections.Width, connections.Height },
                dpi = System.Windows.Media.VisualTreeHelper.GetDpi(settings).DpiScaleX,
                font = app.Resources["UiFont"].ToString(), clearType = true,
            }, new JsonSerializerOptions { WriteIndented = true }));
            app.MainWindow = settings; settings.Activate();
            var remaining = 2;
            foreach (var window in new[] { settings, connections })
                window.Closed += async (_, _) => { if (--remaining == 0) { await NativeBackend.StopAsync(); app.Shutdown(); } };
        }
        catch (Exception error)
        {
            await File.WriteAllTextAsync(Path.Combine(directory, "error.txt"), error.ToString());
            await NativeBackend.StopAsync(); app.Shutdown(1);
        }
    }
}
