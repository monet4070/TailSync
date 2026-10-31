using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Threading;

namespace TailSync.Wpf;

// Exercises this process's real HWNDs, with an isolated backend and synthetic
// records. Native damage/resize traces complement visual-tree renders; neither
// is a recording of the monitor's compositor or its pixel response.
internal static class ResizeChecks
{
    internal static async Task Run(App app, string directory, bool review)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        var checks = new List<string>(); var observations = new List<object>(); string? failure = null;
        void Check(bool value, string name) { if (!value) throw new InvalidOperationException(name); checks.Add(name); }
        async Task Settle() { await app.Dispatcher.InvokeAsync(() => { }, DispatcherPriority.ContextIdle); await Task.Delay(40); }
        try
        {
            await NativeBackend.StartAsync(isolatedRoot: Path.Combine(directory, "profile-" + Guid.NewGuid().ToString("N")));
            AppSession.UseProduction(new NativeHistoryClient()); await SettingsState.ReloadAsync();
            await NativeBackend.InvokeAsync("seed_wpf_test_history", new { count = 65 });
            foreach (var mode in new[] { "light", "dark" })
            {
                await ThemeService.PersistAsync(mode, "builtin:canvas@1", false);
                foreach (var page in new[] { "history", "settings", "connections" })
                {
                    var window = new MainWindow(page) { ShowActivated = false, ShowInTaskbar = false,
                        WindowStartupLocation = WindowStartupLocation.Manual,
                        Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100, Top = 50 };
                    window.Show(); await Task.Delay(450); await Settle();
                    var handle = new WindowInteropHelper(window).Handle;
                    var source = HwndSource.FromHwnd(handle)!; var target = source.CompositionTarget!;
                    var renderMode = target.RenderMode;
                    var trace = new NativeTrace(handle); source.AddHook(trace.Observe);
                    SendMessage(handle, 0x0231, 0, 0); // Same resize loop used by native edge dragging.
                    var heights = new[] { 540, 610, 565, 630, 570 };
                    var widths = new[] { (int)window.MinWidth, 700, (int)window.MinWidth + 31, 610, 480 };
                    for (var index = 0; index < widths.Length; ++index)
                    {
                        SetWindowPos(handle, 0, 0, 0, widths[index], heights[index], 0x0016); // NOSIZE is deliberately absent.
                        await Settle();
                        Check(window.IsMeasureValid && window.IsArrangeValid, $"{mode}/{page}/{widths[index]}: native resize completes layout");
                        Check(target.RenderMode == renderMode && !window.AllowsTransparency,
                            $"{mode}/{page}/{widths[index]}: opaque hardware rendering remains available");
                    }
                    SendMessage(handle, 0x0232, 0, 0); await Settle();
                    Check(trace.Sizes.Count >= 5 && trace.Sizes.All(size => (size.flags & 0x0100) != 0),
                        $"{mode}/{page}: actual native resizes discard the previous client pixels");
                    Check(trace.FullPaints > 0, $"{mode}/{page}: layout is followed by full-client native repaint");
                    Check(window.Background is SolidColorBrush brush && target.BackgroundColor == brush.Color,
                        $"{mode}/{page}: newly exposed pixels use the active canvas color");
                    Check(TextOptions.GetTextRenderingMode(window) == TextRenderingMode.ClearType
                        && TextOptions.GetTextFormattingMode(window) == TextFormattingMode.Display,
                        $"{mode}/{page}: resize retains ClearType and display text layout");
                    if (page == "history")
                    {
                        var list = (ListBox)window.FindName("HistoryList");
                        var viewer = QualityChecks.Descendants(list).OfType<ScrollViewer>().First();
                        viewer.ScrollToTop(); await Settle();
                        list.RaiseEvent(new MouseWheelEventArgs(Mouse.PrimaryDevice, Environment.TickCount, -120) { RoutedEvent = UIElement.PreviewMouseWheelEvent });
                        await Task.Delay(25);
                        if (SystemParameters.ClientAreaAnimation) Check(SmoothScroll.IsAnimating(list), mode + ": wheel fixture starts before resizing");
                        SendMessage(handle, 0x0231, 0, 0); await Settle();
                        Check(!SmoothScroll.IsAnimating(list), mode + ": native resize stops pending scroll motion");
                        var offset = viewer.VerticalOffset; await Task.Delay(100);
                        Check(Math.Abs(viewer.VerticalOffset - offset) < .001, mode + ": content does not keep drifting during native resizing");
                        SendMessage(handle, 0x0232, 0, 0);
                        list.RaiseEvent(new MouseWheelEventArgs(Mouse.PrimaryDevice, Environment.TickCount, -120) { RoutedEvent = UIElement.PreviewMouseWheelEvent });
                        await Task.Delay(400);
                        Check(viewer.VerticalOffset > offset && !SmoothScroll.IsAnimating(list), mode + ": scrolling resumes and settles after resizing");
                        var text = QualityChecks.Descendants(list).OfType<TextBlock>().First(item => item.Name == "EntryDescription");
                        Check(text.FontFamily.Source == ((FontFamily)app.Resources["HistoryFont"]).Source
                            && text.FontSize == (double)app.Resources["HistoryFontSize"], mode + ": history retains its themed font and size");
                    }
                    QualityChecks.Capture(window, Path.Combine(directory, page + "-" + mode + ".png"));
                    observations.Add(new { mode, page, trace.Sizes, trace.FullPaints, renderMode = renderMode.ToString(), target.BackgroundColor });
                    source.RemoveHook(trace.Observe); window.Close(); await Settle();
                }
            }
        }
        catch (Exception error) { failure = error.ToString(); }
        foreach (Window window in app.Windows.Cast<Window>().ToArray()) window.Close();
        await File.WriteAllTextAsync(Path.Combine(directory, "resize.json"), JsonSerializer.Serialize(new { passed = failure == null, checks, observations, failure,
            boundary = "Real native HWND resize messages, full-client update regions and WPF layout; isolated synthetic data. Monitor presentation and visible ghosting require the interactive review." }, new JsonSerializerOptions { WriteIndented = true }));
        if (review && failure == null)
        {
            await ThemeService.PersistAsync("light", "builtin:canvas@1", false);
            var window = new MainWindow("history") { Title = "缩放验证 · 合成历史", Width = 520, Height = 650 };
            window.Show(); await Task.Delay(200); window.Title = "缩放验证 · 合成历史";
            var handle = new WindowInteropHelper(window).Handle; var source = HwndSource.FromHwnd(handle)!;
            var trace = new NativeTrace(handle); source.AddHook(trace.Observe);
            nint Review(nint hwnd, int message, nint wParam, nint lParam, ref bool handled)
            {
                if (message == 0x0232) window.Dispatcher.BeginInvoke(DispatcherPriority.ContextIdle, new Action(async () =>
                {
                    await Task.Delay(80); if (!window.IsVisible) return;
                    QualityChecks.Capture(window, Path.Combine(directory, "interactive-after.png"));
                    await File.WriteAllTextAsync(Path.Combine(directory, "interactive.json"), JsonSerializer.Serialize(new { trace.Entries, trace.Exits, trace.Sizes, trace.FullPaints }, new JsonSerializerOptions { WriteIndented = true }));
                }));
                return 0;
            }
            source.AddHook(Review);
            window.Closed += async (_, _) => { source.RemoveHook(Review); source.RemoveHook(trace.Observe); await NativeBackend.StopAsync(); app.Shutdown(); };
            return;
        }
        await NativeBackend.StopAsync(); app.Shutdown(failure == null ? 0 : 1);
    }

    private sealed class NativeTrace(nint handle)
    {
        internal readonly List<SizeFrame> Sizes = [];
        internal int FullPaints, Entries, Exits;
        private int _width, _height;
        internal nint Observe(nint hwnd, int message, nint wParam, nint lParam, ref bool handled)
        {
            if (message == 0x0047 && lParam != 0)
            {
                var position = Marshal.PtrToStructure<WindowPosition>(lParam);
                if ((position.Flags & 0x0001) == 0 && (position.Width != _width || position.Height != _height))
                { Sizes.Add(new(position.Width, position.Height, position.Flags)); _width = position.Width; _height = position.Height; }
            }
            else if (message == 0x000F && GetUpdateRect(handle, out var update, false) && GetClientRect(handle, out var client)
                && update.Left <= client.Left && update.Top <= client.Top && update.Right >= client.Right && update.Bottom >= client.Bottom) ++FullPaints;
            else if (message == 0x0231) ++Entries;
            else if (message == 0x0232) ++Exits;
            return 0;
        }
    }
    private sealed record SizeFrame(int width, int height, uint flags);
    [StructLayout(LayoutKind.Sequential)] private struct WindowPosition { public nint Window, InsertAfter; public int X, Y, Width, Height; public uint Flags; }
    [StructLayout(LayoutKind.Sequential)] private struct NativeRect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] private static extern nint SendMessage(nint hwnd, int message, nint wParam, nint lParam);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)] private static extern bool SetWindowPos(nint hwnd, nint after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)] private static extern bool GetUpdateRect(nint hwnd, out NativeRect rectangle, [MarshalAs(UnmanagedType.Bool)] bool erase);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)] private static extern bool GetClientRect(nint hwnd, out NativeRect rectangle);
}
