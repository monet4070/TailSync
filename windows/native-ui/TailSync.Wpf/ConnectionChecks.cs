using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;

namespace TailSync.Wpf;

internal static class ConnectionChecks
{
    internal static async Task Run(App app, string directory)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        var checks = new List<string>(); var modeTimings = new List<object>(); string? failure = null;
        void Check(bool value, string label) { if (!value) throw new InvalidOperationException(label); checks.Add(label); }
        async Task Until(Func<bool> predicate)
        {
            var watch = Stopwatch.StartNew();
            while (!predicate()) { if (watch.Elapsed > TimeSpan.FromSeconds(35)) throw new TimeoutException("UI operation did not settle"); await Task.Delay(25); }
        }
        try
        {
            await NativeBackend.StartAsync(isolatedRoot: Path.Combine(directory, "profile-" + Guid.NewGuid().ToString("N")), networkCheck: true);
            Check(NativeBackend.IsIsolated && NativeBackend.NetworkEnabled, "real IPC advertises explicit network acceptance");
            AppSession.UseProduction(new NativeHistoryClient()); await SettingsState.ReloadAsync();
            await SettingsState.SaveAsync("language", "zh-CN"); await ThemeService.PersistAsync("light", "builtin:canvas@1", false);
            var window = new MainWindow("connections") { ShowActivated = false, ShowInTaskbar = false,
                WindowStartupLocation = WindowStartupLocation.Manual,
                Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100,
                Top = SystemParameters.VirtualScreenTop + 50 };
            window.Show(); await Task.Delay(450);
            var view = QualityChecks.Descendants(window).OfType<ConnectionsView>().First();
            var modes = QualityChecks.Descendants(view).OfType<RadioButton>().Where(button => button.Tag is string).ToArray();
            Check(modes.Length == 4, "all four mode controls are present");
            QualityChecks.Descendants(view).OfType<Expander>().First(item => item.Content is Panel panel && panel.Children.OfType<TextBlock>().Any()).IsExpanded = true;
            foreach (var mode in new[] { "auto", "lan_only", "iroh_only", "tailscale_only", "iroh_only" })
            {
                await Until(() => view.ModeSelectionSettled && view.IsEnabled && modes.All(button => button.IsEnabled));
                var button = modes.Single(button => (string)button.Tag == mode);
                var duration = Stopwatch.StartNew();
                button.SetCurrentValue(ToggleButton.IsCheckedProperty, true);
                var feedbackMilliseconds = duration.Elapsed.TotalMilliseconds;
                Check(modes.All(control => control.IsEnabled), mode + ": selection stays interactive during the commit");
                await Until(() => view.ModeSelectionSettled && SettingsState.Current.String("connection_mode") == mode);
                modeTimings.Add(new { mode, feedbackMilliseconds, commitMilliseconds = duration.Elapsed.TotalMilliseconds });
                var persisted = await NativeBackend.InvokeAsync<JsonElement>("get_settings");
                Check(persisted.String("connection_mode") == mode && modes.Count(button => button.IsChecked == true) == 1,
                    mode + ": actual WPF radio event commits through IPC and agrees with persisted Core state");
            }
            // Hold a real save behind a controllable gate: input and selection
            // must stay responsive regardless of network/IPC latency.
            var gate = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            var writes = new List<string>(); var reject = false;
            var testOwner = new MacWindow { Width = 620, Height = 700, ShowActivated = false, ShowInTaskbar = false,
                WindowStartupLocation = WindowStartupLocation.Manual, Left = window.Left, Top = window.Top };
            var queue = new ConnectionsView(testOwner, _ => { }, async (mode, token) =>
            {
                if (reject) throw new IOException("Injected rejected save");
                writes.Add(mode); await gate.Task.WaitAsync(token); await SettingsState.SaveAsync("connection_mode", mode, token);
            }); testOwner.Content = queue; testOwner.Show(); await Task.Delay(100);
            var queueButtons = QualityChecks.Descendants(queue).OfType<RadioButton>().ToArray();
            RadioButton Mode(string mode) => queueButtons.Single(button => (string)button.Tag == mode);
            Mode("lan_only").IsChecked = true; await Until(() => writes.Count == 1);
            Mode("auto").IsChecked = true; Mode("tailscale_only").IsChecked = true;
            var heartbeat = false; await testOwner.Dispatcher.InvokeAsync(() => heartbeat = true);
            Check(heartbeat && !queue.ModeSelectionSettled && queueButtons.All(button => button.IsEnabled) && Mode("tailscale_only").IsChecked == true,
                "dispatcher and last-selected feedback remain responsive while a save is deliberately stalled");
            gate.SetResult(); await Until(() => queue.ModeSelectionSettled);
            Check(writes.SequenceEqual(new[] { "lan_only", "tailscale_only" }) && SettingsState.Current.String("connection_mode") == "tailscale_only",
                "rapid selections coalesce intermediate choices and persist the final choice in order");
            reject = true; Mode("iroh_only").IsChecked = true; await Until(() => queue.ModeSelectionSettled);
            Check(Mode("tailscale_only").IsChecked == true && SettingsState.Current.String("connection_mode") == "tailscale_only",
                "a rejected save restores the committed selection instead of pretending the mode changed");
            testOwner.Close();
            modes.Single(button => (string)button.Tag == "iroh_only").IsChecked = true;
            await Until(() => view.ModeSelectionSettled && SettingsState.Current.String("connection_mode") == "iroh_only");
            var routeFixture = JsonSerializer.SerializeToElement(new { address = "192.0.2.7", routes = new[] {
                new { address = "192.0.2.7", @interface = "lan", online = true },
                new { address = new string('a', 64), @interface = "iroh", online = true } } });
            Check(QualityChecks.Descendants(view).OfType<Expander>().Any(item => item.IsExpanded && item.Content is Panel panel && panel.Children.OfType<TextBlock>().Any()), "device identity disclosure survives mode-driven device refreshes");
            Check(ConnectionsView.PairingAddress(routeFixture) == new string('a', 64), "Iroh-only mode never pairs using an available LAN address");
            var invite = await NativeBackend.InvokeAsync<JsonElement>("create_remote_pairing_invite");
            var preview = await NativeBackend.InvokeAsync<JsonElement>("inspect_remote_pairing_link", new { link = invite.String("link") });
            Check(preview.String("endpoint_id").Length > 0, "WPF transport creates and inspects a real Iroh invite");
            await NativeBackend.InvokeAsync("cancel_remote_pairing_invite");
            var pair = new PairingWindow(window, pollStatus: false) { ShowActivated = false }; pair.Show(); await Task.Delay(420);
            Check(!window.IsEnabled && KeyboardNavigation.GetTabNavigation(pair) == KeyboardNavigationMode.Cycle, "verification dialog owns focus and disables the underlying management window");
            JsonElement Status(string phase, string code = "", bool confirmed = false) => JsonSerializer.SerializeToElement(new {
                phase, remaining_seconds = 118, pairing_enabled = true,
                peer = code.Length > 0 ? new { hostname = "Iroh verification fixture", verification_code = code, fingerprint = "TEST-ONLY-FINGERPRINT", local_confirmed = confirmed } : null });
            pair.Apply(Status("handshaking")); pair.UpdateLayout();
            Check(QualityChecks.Descendants(pair).OfType<ProgressBar>().Any(progress => progress.IsVisible), "handshake shows progress instead of an empty verification code");
            pair.Apply(Status("verification", "123456")); pair.UpdateLayout();
            var confirm = QualityChecks.Descendants(pair).OfType<Button>().Single(button => button.IsDefault);
            Check(confirm.IsEnabled && QualityChecks.Descendants(pair).OfType<UniformGrid>().Any(grid => AutomationProperties.GetName(grid).EndsWith("123456")), "verification renders exactly six digits and enables confirmation");
            QualityChecks.Capture(pair, Path.Combine(directory, "six-digit-verification.png"));
            pair.Apply(Status("waiting_for_peer", "123456", true));
            Check(!confirm.IsEnabled && confirm.Content?.ToString() == Localizer.Text("settings.confirmed"), "local confirmation waits for the other device and cannot be submitted twice");
            pair.Apply(Status("finalizing", "123456", true)); Check(!confirm.IsEnabled, "finalization disables duplicate confirmation");
            pair.Apply(Status("timed_out", "123456")); Check(!confirm.IsEnabled, "expired pairing cannot confirm a stale code");
            pair.Complete(); Check(window.IsEnabled, "closing verification restores the management window");
            QualityChecks.Capture(window, Path.Combine(directory, "connections-real-ipc.png")); window.Close();
        }
        catch (Exception error) { failure = error.ToString(); }
        finally { foreach (Window window in app.Windows.Cast<Window>().ToArray()) { if (window is PairingWindow pair) pair.Complete(); else window.Close(); } await NativeBackend.StopAsync(); }
        await File.WriteAllTextAsync(Path.Combine(directory, "connections.json"), JsonSerializer.Serialize(new { passed = failure == null, checks, modeTimings, failure,
            boundary = "Mode controls use real WPF events and real Core writes; verification layout/state fixtures complement the separate two-real-endpoint Iroh handshake test." }, new JsonSerializerOptions { WriteIndented = true }));
        app.Shutdown(failure == null ? 0 : 1);
    }
}
