using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Shapes;
using System.Windows.Threading;

namespace TailSync.Wpf;

internal static class GuidedTour
{
    private static readonly JsonElement Guide = ReadGuide();
    internal static bool Active { get; private set; }
    internal static bool Suppressed { get; private set; }
    internal static int Step { get; private set; }
    private static int _transition;
    internal static event EventHandler? Changed;
    internal static event EventHandler? StepChanged;
    internal static JsonElement Current => Guide.GetProperty("steps")[Step];
    internal static int Count => Guide.GetProperty("steps").GetArrayLength();
    internal static string Copy(string key) => Localized(Guide.GetProperty("ui").GetProperty(key));
    internal static string Localized(JsonElement value) => value.String(Localizer.Locale, value.String("en"));
    private static JsonElement ReadGuide()
    {
        using var stream = typeof(GuidedTour).Assembly.GetManifestResourceStream("TailSync.Wpf.OnboardingGuide.json")!;
        using var json = JsonDocument.Parse(stream); return json.RootElement.Clone();
    }
    internal static async Task InitializeAsync(bool autoStart)
    {
        Suppressed = await NativeBackend.InvokeAsync<bool>("onboarding_suppressed");
        if (autoStart && await NativeBackend.InvokeAsync<bool>("onboarding_should_show")) await StartAsync();
    }
    internal static async Task StartAsync()
    {
        if (Suppressed || Active || Interlocked.CompareExchange(ref _transition, 1, 0) != 0) return;
        try
        {
            if (RestoreActivity.IsBusy || Application.Current.Windows.OfType<MainWindow>().Any(w => !w.CanChangeDataMode))
                throw new InvalidOperationException(Localizer.Choose("请等待当前历史操作完成后再开始引导。", "Wait for the current history operation before starting the tour."));
            // The native marker remains the authority across WPF/Tauri upgrades.
            await NativeBackend.InvokeAsync("dismiss_onboarding");
            if (AppSession.Client is TourHistoryClient client) client.Reset();
            Active = true; Step = 0; Changed?.Invoke(null, EventArgs.Empty); ShowStep();
        }
        finally { Volatile.Write(ref _transition, 0); }
    }
    internal static async Task EndAsync(bool suppress = false)
    {
        if (!Active || Interlocked.CompareExchange(ref _transition, 1, 0) != 0) return;
        try
        {
            // Keep the tour usable if persistence fails.
            await NativeBackend.InvokeAsync(suppress ? "suppress_onboarding" : "dismiss_onboarding");
            try { await ThemeService.LoadAsync(); } catch (Exception error) { System.Diagnostics.Debug.WriteLine(error.Message); }
            Suppressed |= suppress; Active = false; Changed?.Invoke(null, EventArgs.Empty);
        }
        finally { Volatile.Write(ref _transition, 0); }
    }
    internal static void Move(int delta)
    {
        if (!Active) return;
        Step = Math.Clamp(Step + delta, 0, Count - 1); ShowStep();
    }
    private static void ShowStep()
    {
        MainWindow.Open(Current.String("window")); StepChanged?.Invoke(null, EventArgs.Empty);
    }
    internal static void CheckCommand(string command, object? args)
    {
        // Reject virtual IDs even after the tour ends and stale preview callbacks fire.
        if (args != null && ContainsVirtual(JsonSerializer.SerializeToElement(args)))
            throw new InvalidOperationException(Localizer.Choose("演示记录不能发送到真实后台。", "Demonstration records cannot be sent to the backend."));
        if (!Active) return;
        if (command.StartsWith("get_", StringComparison.Ordinal) || command.StartsWith("wait_", StringComparison.Ordinal)
            || command is "list_themes_v2" or "resolve_theme" or "preview_theme_asset_slot" or "validate_theme"
            or "onboarding_suppressed" or "onboarding_should_show" or "dismiss_onboarding" or "suppress_onboarding" or "handshake" or "shutdown_wpf_backend") return;
        throw new InvalidOperationException(Localizer.Choose("引导中不会修改真实设置或连接设备，请结束引导后操作。", "End the tour to change real settings or connect devices."));
    }
    internal static bool ContainsVirtual(JsonElement value)
    {
        if (value.ValueKind == JsonValueKind.Object) return value.EnumerateObject().Any(p => ContainsVirtual(p.Value));
        if (value.ValueKind == JsonValueKind.Array) return value.EnumerateArray().Any(ContainsVirtual);
        if (value.ValueKind == JsonValueKind.Number) return value.TryGetInt64(out var id) && TourHistoryClient.IsVirtual(id);
        return value.ValueKind == JsonValueKind.String && value.GetString() is { } text && TourHistoryClient.IsVirtualBatch(text);
    }
    internal static void CheckClipboard()
    {
        if (Active) throw new InvalidOperationException(Localizer.Choose("演示期间不会更改系统剪贴板。", "The tour does not change the system clipboard."));
    }
}

// The card occupies its own grid row; the cutout follows actual WPF geometry,
// DPI, scrolling and layout. Only the illuminated control receives input.
internal sealed class TourOverlay : IDisposable
{
    private readonly MainWindow _window;
    private readonly Grid _root;
    private readonly Grid _shade = new() { ClipToBounds = true };
    private readonly Path _mask = new() { Fill = new SolidColorBrush(Color.FromArgb(175, 0, 0, 0)) };
    private readonly Border _ring = new() { BorderThickness = new(2), CornerRadius = new(6), IsHitTestVisible = false };
    private readonly Border _card;
    private ScrollViewer? _description;
    private Button? _never, _more;
    private FrameworkElement? _target;
    private Rect _last;
    private bool _updating;
    internal TourOverlay(MainWindow window, Grid root)
    {
        _window = window; _root = root;
        _ring.SetResourceReference(Border.BorderBrushProperty, "AccentBrush");
        _shade.Children.Add(_mask); _shade.Children.Add(_ring);
        Grid.SetRow(_shade, 1); Panel.SetZIndex(_shade, 80);
        _card = Ui.Surface(new StackPanel(), new(16)); _card.Margin = new(10); _card.MaxHeight = 230;
        Grid.SetRow(_card, 2); Panel.SetZIndex(_card, 81);
        root.RowDefinitions.Add(new() { Height = GridLength.Auto }); root.Children.Add(_shade); root.Children.Add(_card);
        _window.LayoutUpdated += Layout; GuidedTour.StepChanged += StepChanged; Localizer.Changed += StepChanged;
        Update();
    }
    private void StepChanged(object? sender, EventArgs e) => Update();
    private void Update()
    {
        var visible = GuidedTour.Active && GuidedTour.Current.String("window") == _window.Page;
        _shade.Visibility = _card.Visibility = visible ? Visibility.Visible : Visibility.Collapsed;
        if (!visible) return;
        var step = GuidedTour.Current;
        var content = new Grid(); content.RowDefinitions.Add(new() { Height = GridLength.Auto }); content.RowDefinitions.Add(new()); content.RowDefinitions.Add(new() { Height = GridLength.Auto });
        var title = Ui.Text($"{GuidedTour.Step + 1}/{GuidedTour.Count} · {GuidedTour.Localized(step.GetProperty("title"))}", 14); title.FontWeight = FontWeights.SemiBold;
        content.Children.Add(title);
        var description = Ui.Text(GuidedTour.Localized(step.GetProperty("body")), 12, "Secondary");
        var scroll = new ScrollViewer { Content = description, MaxHeight = 110, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, Margin = new(0, 8, 0, 8) }; _description = scroll; title.ToolTip = description.Text; Grid.SetRow(scroll, 1); content.Children.Add(scroll);
        var actions = new WrapPanel(); Grid.SetRow(actions, 2); content.Children.Add(actions);
        Button Add(string label, Func<Task> action, bool enabled = true)
        {
            var button = Ui.Button(label); button.FontSize = 12; button.Margin = new(0, 3, 5, 0); button.IsEnabled = enabled;
            button.Click += async (_, _) => { button.IsEnabled = false; try { await action(); } catch (Exception error) { description.Text = error.Message; } finally { button.IsEnabled = true; } };
            actions.Children.Add(button);
            return button;
        }
        Add(GuidedTour.Copy("back"), () => { GuidedTour.Move(-1); return Task.CompletedTask; }, GuidedTour.Step > 0);
        Add(GuidedTour.Copy(GuidedTour.Step == GuidedTour.Count - 1 ? "finish" : "next"), () => GuidedTour.Step == GuidedTour.Count - 1 ? GuidedTour.EndAsync() : Advance());
        Add(GuidedTour.Copy("skip"), () => GuidedTour.EndAsync());
        _never = Add(GuidedTour.Copy("never"), () => GuidedTour.EndAsync(true));
        _more = Ui.Button("⋯"); _more.ToolTip = GuidedTour.Copy("never"); _more.Margin = new(0, 3, 0, 0);
        var menu = new ContextMenu(); var neverItem = new MenuItem { Header = GuidedTour.Copy("never") };
        neverItem.Click += async (_, _) => { try { await GuidedTour.EndAsync(true); } catch (Exception error) { description.Text = error.Message; } };
        menu.Items.Add(neverItem); _more.Click += (_, _) => { menu.PlacementTarget = _more; menu.IsOpen = true; }; actions.Children.Add(_more);
        _card.Child = content;
        _target = _window.TourTarget(step.String("target")); _target?.BringIntoView(); _last = Rect.Empty;
        _window.Dispatcher.BeginInvoke(() => Layout(null, EventArgs.Empty), DispatcherPriority.Loaded);
    }
    private static Task Advance() { GuidedTour.Move(1); return Task.CompletedTask; }
    private void Layout(object? sender, EventArgs e)
    {
        if (_updating || !_shade.IsVisible || _shade.ActualWidth <= 0 || _shade.ActualHeight <= 0) return;
        _updating = true;
        try
        {
            var bounds = new Rect(0, 0, _shade.ActualWidth, _shade.ActualHeight);
            _card.Padding = new(_window.ActualHeight < 480 ? 8 : 16);
            _card.Margin = new(_window.ActualHeight < 480 ? 6 : 10);
            if (_description != null) _description.Visibility = _window.ActualHeight < 480 ? Visibility.Collapsed : Visibility.Visible;
            if (_never != null) _never.Visibility = _window.ActualWidth < 380 ? Visibility.Collapsed : Visibility.Visible;
            if (_more != null) _more.Visibility = _window.ActualWidth < 380 ? Visibility.Visible : Visibility.Collapsed;
            var hole = Rect.Empty;
            if (_target is { IsVisible: true } target)
            {
                try
                {
                    hole = target.TransformToVisual(_shade).TransformBounds(new Rect(target.RenderSize));
                    hole.Intersect(bounds);
                    for (DependencyObject? parent = target; parent != null; parent = VisualTreeHelper.GetParent(parent))
                        if (parent is ScrollViewer viewer) hole.Intersect(viewer.TransformToVisual(_shade).TransformBounds(new Rect(viewer.RenderSize)));
                }
                catch (InvalidOperationException) { hole = Rect.Empty; }
            }
            // Missing targets leave the page operable for retry/navigation.
            if (hole.IsEmpty) { _mask.Data = null; _ring.Visibility = Visibility.Collapsed; _last = Rect.Empty; return; }
            if (hole == _last && _mask.Data?.Bounds == bounds) return;
            _last = hole;
            var geometry = new GeometryGroup { FillRule = FillRule.EvenOdd };
            geometry.Children.Add(new RectangleGeometry(bounds)); geometry.Children.Add(new RectangleGeometry(hole, 6, 6)); _mask.Data = geometry;
            _ring.Visibility = Visibility.Visible; _ring.HorizontalAlignment = HorizontalAlignment.Left; _ring.VerticalAlignment = VerticalAlignment.Top;
            _ring.Margin = new(hole.X, hole.Y, 0, 0); _ring.Width = hole.Width; _ring.Height = hole.Height;
        }
        finally { _updating = false; }
    }
    public void Dispose()
    {
        _window.LayoutUpdated -= Layout; GuidedTour.StepChanged -= StepChanged; Localizer.Changed -= StepChanged;
        _root.Children.Remove(_shade); _root.Children.Remove(_card); _root.RowDefinitions.RemoveAt(2);
    }
}
