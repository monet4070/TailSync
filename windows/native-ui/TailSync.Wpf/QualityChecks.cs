using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace TailSync.Wpf;

// Real WPF layout and routed-input regression checks, using demo data only.
// Render callbacks measure dispatcher cadence, not display presentation latency.
internal static class QualityChecks
{
    internal static async Task Run(App app, string directory)
    {
        Directory.CreateDirectory(directory);
        var checks = new List<string>();
        var layouts = new List<object>();
        var offsets = new List<double>();
        var intervals = new List<double>();
        string? failure = null;
        void Check(bool value, string label)
        {
            if (!value) throw new InvalidOperationException(label);
            checks.Add(label);
        }
        try
        {
            ThemeService.Apply(appearance: "light", id: "canvas");
            foreach (var page in new[] { "history", "settings", "connections" })
            {
                var window = new MainWindow(page); window.Show();
                await Settle();
                foreach (var disclosure in Descendants(window).OfType<Button>().Where(x => x.Content is AdaptiveSettingRow row && row.Children.OfType<VectorIcon>().Any(icon => icon.Kind == "chevron")).ToArray())
                    disclosure.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                await Settle();
                var originalWidth = window.Width;
                foreach (var narrow in new[] { false, true })
                {
                    window.Width = narrow ? window.MinWidth : originalWidth;
                    await Settle();
                    var actions = Descendants(window).OfType<AdaptiveSettingRow>().Where(x => x.IsVisible && x.Children.Count == 2).ToArray();
                    foreach (var row in actions)
                    {
                        var action = (FrameworkElement)row.Children[1];
                        var bounds = action.TransformToAncestor(row).TransformBounds(new Rect(action.RenderSize));
                        Check(bounds.Left >= -.5 && bounds.Right <= row.ActualWidth + .5 && bounds.Bottom <= row.ActualHeight + .5,
                            $"{page}/{window.Width}: setting action fits ({System.Windows.Automation.AutomationProperties.GetName(action)})");
                    }
                    foreach (var button in Descendants(window).OfType<Button>().Where(x => x.IsVisible && x.Content is FrameworkElement))
                    {
                        var content = (FrameworkElement)button.Content;
                        var bounds = content.TransformToAncestor(button).TransformBounds(new Rect(content.RenderSize));
                        Check(bounds.Left >= -.5 && bounds.Top >= -.5 && bounds.Right <= button.ActualWidth + .5 && bounds.Bottom <= button.ActualHeight + .5,
                            $"{page}/{window.Width}: button content fits ({System.Windows.Automation.AutomationProperties.GetName(button)}; bounds={bounds}, button={button.RenderSize})");
                    }
                    var list = (ListBox)window.FindName("HistoryList");
                    if (page == "history")
                    {
                        foreach (var item in Descendants(list).OfType<ListBoxItem>())
                        foreach (var button in Descendants(item).OfType<Button>().Where(x => x.IsVisible))
                        {
                            var bounds = button.TransformToAncestor(item).TransformBounds(new Rect(button.RenderSize));
                            Check(bounds.Left >= 0 && bounds.Right <= item.ActualWidth + .5 && bounds.Top >= 0 && bounds.Bottom <= item.ActualHeight + .5,
                                $"history/{window.Width}: row action remains inside its item");
                        }
                    }
                    var dpi = VisualTreeHelper.GetDpi(window);
                    layouts.Add(new { page, width = window.Width, dpi = dpi.PixelsPerInchX, stackedRows = actions.Count(x => x.IsStacked) });
                    Capture(window, Path.Combine(directory, page + (narrow ? "-narrow" : "") + ".png"));
                }
                if (page == "settings")
                {
                    window.Width = originalWidth;
                    var viewer = (ScrollViewer)window.FindName("SettingsPage");
                    viewer.ScrollToEnd(); await Settle();
                    Capture(window, Path.Combine(directory, "settings-bottom.png"));
                    var end = viewer.VerticalOffset;
                    Wheel(viewer, 120); await Task.Delay(350);
                    Check(viewer.VerticalOffset < end && !SmoothScroll.IsAnimating(viewer), "settings wheel moves and settles");
                }
                window.Close();
            }

            var history = new MainWindow("history"); history.Show(); await Settle();
            var historyList = (ListBox)history.FindName("HistoryList");
            var scroll = Descendants(historyList).OfType<ScrollViewer>().First();
            Check(VirtualizingPanel.GetScrollUnit(historyList) == ScrollUnit.Pixel && VirtualizingPanel.GetIsVirtualizing(historyList), "pixel scrolling retains virtualization");
            if (historyList.Items.Count >= 30)
                Check(Descendants(historyList).OfType<ListBoxItem>().Count() < historyList.Items.Count, "30-entry page only realizes the viewport and cache");
            Check(TextOptions.GetTextFormattingMode(history) == TextFormattingMode.Display && TextOptions.GetTextRenderingMode(history) == TextRenderingMode.ClearType,
                "window uses Display formatting and ClearType");
            var watch = Stopwatch.StartNew(); double previous = 0; var lastFrame = TimeSpan.MinValue;
            void Frame(object? sender, EventArgs e)
            {
                if (e is RenderingEventArgs frame)
                {
                    if (frame.RenderingTime == lastFrame) return;
                    lastFrame = frame.RenderingTime;
                }
                var now = watch.Elapsed.TotalMilliseconds;
                if (previous > 0) intervals.Add(now - previous);
                previous = now; offsets.Add(scroll.VerticalOffset);
            }
            CompositionTarget.Rendering += Frame;
            try
            {
                Wheel(historyList, -120); await Task.Delay(45);
                Wheel(historyList, -120); await Task.Delay(400);
            }
            finally { CompositionTarget.Rendering -= Frame; }
            Check(scroll.VerticalOffset > 0, "wheel input moves the actual list viewport");
            if (SystemParameters.ClientAreaAnimation)
                Check(offsets.Distinct().Count() >= 3, "wheel motion includes intermediate rendered offsets");
            Check(!SmoothScroll.IsAnimating(historyList), "render subscription stops after settling");
            var endingOffset = scroll.VerticalOffset;
            Wheel(historyList, 120); await Task.Delay(350);
            Check(scroll.VerticalOffset < endingOffset, "reverse wheel input moves upward");
            Wheel(historyList, -120); await Task.Delay(25);
            var beforeReverse = scroll.VerticalOffset;
            Wheel(historyList, 120); await Task.Delay(350);
            Check(scroll.VerticalOffset < beforeReverse, "direction reversal interrupts pending downward movement");
            Wheel(historyList, -120); await Task.Delay(25);
            ((TextBox)history.FindName("SearchBox")).Text = "does not exist";
            await Settle();
            Check(!SmoothScroll.IsAnimating(historyList), "changing search results cancels pending motion");
            ((TextBox)history.FindName("SearchBox")).Clear(); await Settle();
            scroll.ScrollToTop(); await Settle();
            Check(!Wheel(historyList, 120), "top boundary leaves wheel input unhandled");
            Wheel(historyList, -120); await Task.Delay(20);
            history.Close(); await Settle();
            Check(!SmoothScroll.IsAnimating(historyList), "closing a moving window removes its render subscription");
        }
        catch (Exception error) { failure = error.ToString(); }
        var sorted = intervals.Order().ToArray();
        await File.WriteAllTextAsync(Path.Combine(directory, "quality.json"), JsonSerializer.Serialize(new
        {
            passed = failure == null, checks, failure, layouts,
            wheel = new { animationEnabled = SystemParameters.ClientAreaAnimation, offsets, renderCallbackIntervalsMs = intervals,
                medianMs = sorted.Length > 0 ? sorted[sorted.Length / 2] : 0, p95Ms = sorted.Length > 0 ? sorted[(int)((sorted.Length - 1) * .95)] : 0 },
            limitation = "Live WPF layout at this monitor's DPI; render callbacks are not GPU presentation or end-to-end input latency. PNGs are visual-tree renders."
        }, new JsonSerializerOptions { WriteIndented = true }));
        app.Shutdown(failure == null ? 0 : 1);
    }

    private static bool Wheel(UIElement element, int delta)
    {
        var args = new MouseWheelEventArgs(Mouse.PrimaryDevice, Environment.TickCount, delta) { RoutedEvent = UIElement.PreviewMouseWheelEvent };
        element.RaiseEvent(args); return args.Handled;
    }
    private static async Task Settle() { await Dispatcher.Yield(DispatcherPriority.ApplicationIdle); await Task.Delay(60); }
    internal static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
        {
            var child = VisualTreeHelper.GetChild(root, i); yield return child;
            foreach (var descendant in Descendants(child)) yield return descendant;
        }
    }
    internal static void Capture(Window window, string path)
    {
        var dpi = VisualTreeHelper.GetDpi(window);
        var bitmap = new RenderTargetBitmap((int)Math.Ceiling(window.ActualWidth * dpi.DpiScaleX), (int)Math.Ceiling(window.ActualHeight * dpi.DpiScaleY), dpi.PixelsPerInchX, dpi.PixelsPerInchY, PixelFormats.Pbgra32);
        bitmap.Render(window);
        var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap));
        using var stream = File.Create(path); encoder.Save(stream);
    }
}
