using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Windows;

namespace TailSync.Wpf;

// A separate synthetic foreground process models the input privilege held by
// the tray process. No user clipboard, tray, registry, or other apps are driven.
internal static class ActivationChecks
{
    internal static async Task Fixture(App app, string directory)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        var window = new Window { Title = "前置验证 · 合成窗口", Width = 340, Height = 150, ShowInTaskbar = true,
            WindowStartupLocation = WindowStartupLocation.CenterScreen,
            Content = new System.Windows.Controls.TextBlock { Text = "窗口前置验证 · 独立合成进程", FontSize = 16,
                VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center } };
        window.Closed += (_, _) => app.Shutdown(); window.Show(); WindowActivation.Raise(window);
        var revision = 0;
        async Task State(bool granted)
        {
            var temporary = Path.Combine(directory, "state.tmp");
            await File.WriteAllTextAsync(temporary, JsonSerializer.Serialize(new { revision = ++revision, pid = Environment.ProcessId,
                foreground = WindowActivation.IsForeground(window), granted }));
            File.Move(temporary, Path.Combine(directory, "state.json"), true);
        }
        await Task.Delay(150); await State(false);
        while (await Console.In.ReadLineAsync() is { } command)
        {
            if (command == "exit") break;
            if (!command.StartsWith("show:") || !uint.TryParse(command[5..], out var process) || process == 0) continue;
            WindowActivation.Raise(window); await Task.Delay(100);
            // An unattended process has no right to steal foreground from the
            // user's app. The first run waits for an explicit QA-window click;
            // later cases yield foreground from the active test history.
            var activation = Stopwatch.StartNew();
            while (!WindowActivation.IsForeground(window) && activation.ElapsedMilliseconds < 55000) await Task.Delay(25);
            await State(WindowActivation.GrantForeground(process));
        }
        window.Close(); app.Shutdown();
    }

    internal static async Task Run(App app, string directory)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        var checks = new List<string>(); string? failure = null; Process? fixture = null;
        void Check(bool value, string name) { if (!value) throw new InvalidOperationException(name); checks.Add(name); }
        async Task Until(Func<bool> condition, int timeoutMilliseconds = 8000)
        {
            var watch = Stopwatch.StartNew();
            while (!condition()) { if (watch.ElapsedMilliseconds > timeoutMilliseconds) throw new TimeoutException("Foreground fixture did not settle."); await Task.Delay(25); }
        }
        try
        {
            await NativeBackend.StartAsync(isolatedRoot: Path.Combine(directory, "profile"));
            AppSession.UseProduction(new NativeHistoryClient()); await SettingsState.ReloadAsync(); await ThemeService.LoadAsync();
            await NativeBackend.InvokeAsync("seed_wpf_test_history", new { count = 3 });
            NativeBackend.NavigationChanged += app.Navigate; NativeBackend.BeginWatching();
            var history = new MainWindow("history") { ShowInTaskbar = false }; history.Show();
            var fixtureDirectory = Path.Combine(directory, "foreground-fixture"); Directory.CreateDirectory(fixtureDirectory);
            var start = new ProcessStartInfo(Environment.ProcessPath!) { UseShellExecute = false, RedirectStandardInput = true, CreateNoWindow = true };
            start.ArgumentList.Add("--activation-fixture-dir=" + fixtureDirectory);
            fixture = Process.Start(start) ?? throw new IOException("Foreground fixture did not start."); fixture.StandardInput.AutoFlush = true;
            var revision = 1;
            JsonElement State()
            {
                try { using var document = JsonDocument.Parse(File.ReadAllText(Path.Combine(fixtureDirectory, "state.json"))); return document.RootElement.Clone(); }
                catch (IOException) { return default; } catch (JsonException) { return default; }
            }
            await Until(() => State().Long("revision") >= revision);
            async Task CoverAndGrant()
            {
                WindowActivation.GrantForeground((uint)fixture.Id);
                ++revision; await fixture.StandardInput.WriteLineAsync("show:" + NativeBackend.ProcessId);
                await Until(() => State().Long("revision") >= revision, 60000);
                Check(State().Bool("foreground") && State().Bool("granted"), "independent foreground process delegates activation only to this test backend");
            }
            async Task OpenThroughBackend()
            {
                await NativeBackend.InvokeAsync("open_history_window");
                await Until(() => app.Windows.OfType<MainWindow>().Any(window => window.Page == "history" && window.IsVisible && WindowActivation.IsForeground(window)));
            }
            await CoverAndGrant(); await OpenThroughBackend();
            Check(WindowActivation.IsForeground(history) && !history.Topmost, "real backend navigation raises an existing covered history window without pinning it");
            history.WindowState = WindowState.Minimized; await CoverAndGrant(); await OpenThroughBackend();
            Check(history.WindowState != WindowState.Minimized && WindowActivation.IsForeground(history), "tray-style navigation restores minimized history and gives it foreground focus");
            history.Hide(); await CoverAndGrant(); await OpenThroughBackend();
            Check(history.IsVisible && WindowActivation.IsForeground(history), "tray-style navigation also reveals a hidden history window");
            history.Close(); await CoverAndGrant(); await OpenThroughBackend();
            var reopened = app.Windows.OfType<MainWindow>().Single(window => window.Page == "history");
            Check(!ReferenceEquals(reopened, history) && WindowActivation.IsForeground(reopened), "closed history is recreated and placed in front through the same backend event");
            reopened.Topmost = true; await NativeBackend.InvokeAsync("open_history_window"); await Task.Delay(100);
            Check(reopened.Topmost, "activation preserves an explicitly pinned window"); reopened.Topmost = false;
        }
        catch (Exception error) { failure = error.ToString(); }
        finally
        {
            NativeBackend.NavigationChanged -= app.Navigate;
            foreach (Window window in app.Windows.Cast<Window>().ToArray()) window.Close();
            if (fixture != null)
            {
                try { if (!fixture.HasExited) { await fixture.StandardInput.WriteLineAsync("exit"); using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(4)); await fixture.WaitForExitAsync(timeout.Token); } }
                catch { if (!fixture.HasExited) fixture.Kill(entireProcessTree: true); }
                fixture.Dispose();
            }
            await NativeBackend.StopAsync();
        }
        await File.WriteAllTextAsync(Path.Combine(directory, "activation.json"), JsonSerializer.Serialize(new { passed = failure == null, checks, failure,
            boundary = "Real Windows foreground ownership in a second synthetic process and the actual authenticated Rust navigation channel. The physical notification-area click is a separate check." }, new JsonSerializerOptions { WriteIndented = true }));
        app.Shutdown(failure == null ? 0 : 1);
    }
}
