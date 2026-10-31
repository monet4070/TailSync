using System.Diagnostics;
using System.Collections.Specialized;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace TailSync.Wpf;

// Scroll offsets remain in pixels while VirtualizingStackPanel keeps recycling.
// Only subscribe to the render clock during motion; unload/input cancels it.
public static class SmoothScroll
{
    public static readonly DependencyProperty IsEnabledProperty = DependencyProperty.RegisterAttached(
        "IsEnabled", typeof(bool), typeof(SmoothScroll), new PropertyMetadata(false, Changed));
    private static readonly DependencyProperty StateProperty = DependencyProperty.RegisterAttached(
        "State", typeof(State), typeof(SmoothScroll));
    public static bool GetIsEnabled(DependencyObject value) => (bool)value.GetValue(IsEnabledProperty);
    public static void SetIsEnabled(DependencyObject value, bool enabled) => value.SetValue(IsEnabledProperty, enabled);
    internal static bool IsAnimating(DependencyObject value) => (value.GetValue(StateProperty) as State)?.Running == true;
    internal static void Cancel(DependencyObject value) => (value.GetValue(StateProperty) as State)?.Stop();
    internal static double PixelOffset(double offset, double scale, double maximum)
    {
        // Clamp to the last complete device pixel instead of letting the final
        // animation frame leave the text at a fractional scroll transform.
        scale = double.IsFinite(scale) && scale > 0 ? scale : 1;
        var lastPixel = Math.Max(0, Math.Floor(Math.Max(0, maximum) * scale + .000001));
        return Math.Clamp(Math.Round(offset * scale), 0, lastPixel) / scale;
    }

    private static void Changed(DependencyObject value, DependencyPropertyChangedEventArgs args)
    {
        if (value is not FrameworkElement host) return;
        if (host.GetValue(StateProperty) is State old) old.Dispose();
        host.ClearValue(StateProperty);
        if ((bool)args.NewValue) host.SetValue(StateProperty, new State(host));
    }

    private sealed class State : IDisposable
    {
        private readonly FrameworkElement _host;
        private readonly INotifyCollectionChanged? _items;
        private ScrollViewer? _viewer;
        private double _target, _position;
        private long _lastTick;
        private TimeSpan _lastRenderingTime = TimeSpan.MinValue;
        private bool _snapping;
        public bool Running { get; private set; }

        public State(FrameworkElement host)
        {
            _host = host;
            _items = (host as ItemsControl)?.Items;
            if (_items != null) _items.CollectionChanged += ItemsChanged;
            host.PreviewMouseWheel += Wheel;
            host.PreviewMouseDown += CancelMouse;
            host.PreviewKeyDown += CancelKey;
            host.Unloaded += Unloaded;
            host.Loaded += Loaded;
            host.AddHandler(ScrollViewer.ScrollChangedEvent, new ScrollChangedEventHandler(ScrollChanged));
        }
        private void Loaded(object sender, RoutedEventArgs e)
        {
            _viewer = _host as ScrollViewer ?? FindViewer(_host);
            SnapOffset();
        }
        private void ScrollChanged(object sender, ScrollChangedEventArgs e)
        {
            if (e.OriginalSource != _viewer || _snapping) return;
            SnapOffset();
        }
        private void SnapOffset()
        {
            if (_viewer == null) return;
            var target = PixelOffset(_viewer.VerticalOffset, VisualTreeHelper.GetDpi(_viewer).DpiScaleY, _viewer.ScrollableHeight);
            if (Math.Abs(target - _viewer.VerticalOffset) < .00001) return;
            _snapping = true;
            try { _viewer.ScrollToVerticalOffset(target); }
            finally { _snapping = false; }
        }
        private void ScrollTo(double offset)
        {
            if (_viewer != null)
                _viewer.ScrollToVerticalOffset(PixelOffset(offset, VisualTreeHelper.GetDpi(_viewer).DpiScaleY, _viewer.ScrollableHeight));
        }

        private void Wheel(object sender, MouseWheelEventArgs e)
        {
            if (e.Handled || Keyboard.Modifiers != ModifierKeys.None || e.Delta == 0) return;
            _viewer ??= _host as ScrollViewer ?? FindViewer(_host);
            if (_viewer == null || _viewer.ScrollableHeight <= 0) return;
            var lines = SystemParameters.WheelScrollLines;
            if (lines == 0) return;
            var lastPixel = PixelOffset(_viewer.ScrollableHeight, VisualTreeHelper.GetDpi(_viewer).DpiScaleY, _viewer.ScrollableHeight);
            if (!Running && (e.Delta > 0 && _viewer.VerticalOffset <= .00001 || e.Delta < 0 && _viewer.VerticalOffset >= lastPixel - .00001)) return;
            // Preserve sub-detent input from precision wheels and touchpads.
            var distance = lines < 0 ? _viewer.ViewportHeight : lines * 18.0;
            var start = Running ? _target : _viewer.VerticalOffset;
            if (Running && Math.Sign(_target - _viewer.VerticalOffset) == Math.Sign(e.Delta))
                start = _viewer.VerticalOffset;
            var target = Math.Clamp(start - e.Delta / 120.0 * distance, 0, _viewer.ScrollableHeight);
            if (Math.Abs(target - start) < .01)
            {
                // Do not let the default handler jump to the edge while our
                // existing animation is still approaching that same edge.
                e.Handled = Running;
                return;
            }
            e.Handled = true;
            _target = target;
            if (!SystemParameters.ClientAreaAnimation)
            {
                Stop(); ScrollTo(target); return;
            }
            if (Running) return;
            _position = _viewer.VerticalOffset;
            _lastTick = Stopwatch.GetTimestamp();
            _lastRenderingTime = TimeSpan.MinValue;
            Running = true;
            CompositionTarget.Rendering += Frame;
        }

        private void Frame(object? sender, EventArgs e)
        {
            if (_viewer == null || !_host.IsVisible) { Stop(); return; }
            // WPF can raise Rendering again for the same composition frame after
            // layout changes. Never create a layout/render feedback loop.
            if (e is RenderingEventArgs frame)
            {
                if (frame.RenderingTime == _lastRenderingTime) return;
                _lastRenderingTime = frame.RenderingTime;
            }
            var now = Stopwatch.GetTimestamp();
            var seconds = (now - _lastTick) / (double)Stopwatch.Frequency;
            _lastTick = now;
            _target = Math.Clamp(_target, 0, _viewer.ScrollableHeight);
            _position += (_target - _position) * (1 - Math.Exp(-seconds / .045));
            if (Math.Abs(_target - _position) < .5)
            {
                ScrollTo(_target); Stop(); return;
            }
            // Pixel alignment protects glyph sharpness during motion.
            ScrollTo(_position);
        }

        private void CancelMouse(object sender, MouseButtonEventArgs e) => Stop();
        private void CancelKey(object sender, KeyEventArgs e) => Stop();
        private void ItemsChanged(object? sender, NotifyCollectionChangedEventArgs e) => Stop();
        private void Unloaded(object sender, RoutedEventArgs e) { Stop(); _viewer = null; }
        internal void Stop()
        {
            if (Running) { CompositionTarget.Rendering -= Frame; Running = false; }
            SnapOffset();
        }
        public void Dispose()
        {
            Stop();
            _host.PreviewMouseWheel -= Wheel;
            _host.PreviewMouseDown -= CancelMouse;
            _host.PreviewKeyDown -= CancelKey;
            _host.Unloaded -= Unloaded;
            _host.Loaded -= Loaded;
            _host.RemoveHandler(ScrollViewer.ScrollChangedEvent, new ScrollChangedEventHandler(ScrollChanged));
            if (_items != null) _items.CollectionChanged -= ItemsChanged;
        }
    }

    private static ScrollViewer? FindViewer(DependencyObject parent)
    {
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(parent); i++)
        {
            var child = VisualTreeHelper.GetChild(parent, i);
            if (child is ScrollViewer viewer) return viewer;
            if (FindViewer(child) is { } nested) return nested;
        }
        return null;
    }
}
