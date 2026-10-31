using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;

namespace TailSync.Wpf;

public static partial class ControlMotion
{
    private static readonly DependencyProperty GenerationProperty = DependencyProperty.RegisterAttached("Generation", typeof(int), typeof(ControlMotion), new PropertyMetadata(0));
    public static readonly DependencyProperty DisclosureProperty = DependencyProperty.RegisterAttached("Disclosure", typeof(bool), typeof(ControlMotion), new PropertyMetadata(false, AttachDisclosure));
    public static bool GetDisclosure(DependencyObject element) => (bool)element.GetValue(DisclosureProperty);
    public static void SetDisclosure(DependencyObject element, bool enabled) => element.SetValue(DisclosureProperty, enabled);
    public static readonly DependencyProperty MenuMotionProperty = DependencyProperty.RegisterAttached("MenuMotion", typeof(bool), typeof(ControlMotion), new PropertyMetadata(false, AttachMenu));
    public static bool GetMenuMotion(DependencyObject element) => (bool)element.GetValue(MenuMotionProperty);
    public static void SetMenuMotion(DependencyObject element, bool enabled) => element.SetValue(MenuMotionProperty, enabled);

    private static Duration Duration(double milliseconds) => TimeSpan.FromMilliseconds(SystemParameters.ClientAreaAnimation ? milliseconds : 0);
    private static IEasingFunction Ease => new CubicEase { EasingMode = EasingMode.EaseOut };
    private static Color ColorOf(Brush? brush)
    {
        if (brush is not SolidColorBrush solid) return Colors.Transparent;
        var color = solid.Color; color.A = (byte)Math.Round(color.A * solid.Opacity); return color;
    }
    private static void FadeBrush(SolidColorBrush brush, Color target, double milliseconds = 160)
    {
        var from = brush.Color; brush.BeginAnimation(SolidColorBrush.ColorProperty, null); brush.Color = target;
        if (from != target && SystemParameters.ClientAreaAnimation)
            brush.BeginAnimation(SolidColorBrush.ColorProperty, new ColorAnimation(from, target, Duration(milliseconds)) { EasingFunction = Ease, FillBehavior = FillBehavior.Stop });
    }
    internal static IEnumerable<T> Visuals<T>(DependencyObject element) where T : DependencyObject
    {
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(element); i++)
        {
            var child = VisualTreeHelper.GetChild(element, i); if (child is T result) yield return result;
            foreach (var nested in Visuals<T>(child)) yield return nested;
        }
    }
    internal static void ButtonSurface(Button button)
    {
        var descriptor = DependencyPropertyDescriptor.FromProperty(Control.BackgroundProperty, typeof(Button));
        Border? chrome = null; SolidColorBrush? paint = null;
        void Update(object? sender, EventArgs e)
        {
            if (chrome == null || paint == null) return;
            FadeBrush(paint, ColorOf(button.Background));
        }
        void ThemeChanged(object? sender, EventArgs e)
        {
            if (paint == null) return;
            // Foreground resources change immediately. Commit the matching
            // surface in the same turn so theme changes cannot flash low contrast.
            paint.BeginAnimation(SolidColorBrush.ColorProperty, null);
            paint.Color = ColorOf(button.Background);
        }
        button.Loaded += (_, _) =>
        {
            button.ApplyTemplate(); chrome = button.Template?.FindName("Chrome", button) as Border ?? Visuals<Border>(button).FirstOrDefault();
            if (chrome == null) return;
            paint = new SolidColorBrush(ColorOf(button.Background)); chrome.Background = paint;
            descriptor.AddValueChanged(button, Update);
            ThemeService.Changed += ThemeChanged;
        };
        button.Unloaded += (_, _) => { descriptor.RemoveValueChanged(button, Update); ThemeService.Changed -= ThemeChanged; paint?.BeginAnimation(SolidColorBrush.ColorProperty, null); chrome = null; paint = null; };
    }
    internal static void Preference(Panel row, UIElement action)
    {
        // Rows are layout, not selection surfaces. Feedback belongs to the
        // focused control; keep the transparent hit target for row switches.
        row.Background = Brushes.Transparent;
        if (action is not ToggleButton toggle) return;
        row.Cursor = Cursors.Hand;
        row.MouseLeftButtonUp += (_, e) =>
        {
            if (e.Handled || !toggle.IsEnabled) return;
            for (var source = e.OriginalSource as DependencyObject; source != null && source != row;)
            {
                if (source is ButtonBase or TextBoxBase or Slider or ComboBox or Hyperlink) return;
                source = source is Visual ? VisualTreeHelper.GetParent(source) : (source as FrameworkContentElement)?.Parent;
            }
            toggle.SetCurrentValue(ToggleButton.IsCheckedProperty, toggle.IsChecked != true);
            toggle.RaiseEvent(new RoutedEventArgs(ButtonBase.ClickEvent, toggle)); e.Handled = true;
        };
    }
    public static void AnimateDisclosure(FrameworkElement element, bool expanded)
    {
        var generation = (int)element.GetValue(GenerationProperty) + 1; element.SetValue(GenerationProperty, generation);
        var wasVisible = element.Visibility == Visibility.Visible;
        var from = wasVisible ? element.ActualHeight : 0;
        element.BeginAnimation(FrameworkElement.HeightProperty, null);
        if (!expanded && !wasVisible) return;
        element.Visibility = Visibility.Visible; element.ClipToBounds = true; element.Height = double.NaN;
        var width = element.ActualWidth > 0 ? element.ActualWidth : element.Parent is FrameworkElement parent ? parent.ActualWidth : 400;
        element.Measure(new Size(Math.Max(1, width), double.PositiveInfinity));
        var target = expanded ? Math.Max(0, element.DesiredSize.Height - element.Margin.Top - element.Margin.Bottom) : 0;
        if (!SystemParameters.ClientAreaAnimation || target == from)
        {
            element.Visibility = expanded ? Visibility.Visible : Visibility.Collapsed; element.Height = double.NaN; return;
        }
        element.Height = target;
        var animation = new DoubleAnimation(from, target, Duration(expanded ? 320 : 180)) { EasingFunction = Ease, FillBehavior = FillBehavior.Stop };
        animation.Completed += (_, _) =>
        {
            if ((int)element.GetValue(GenerationProperty) != generation) return;
            element.BeginAnimation(FrameworkElement.HeightProperty, null); element.Height = double.NaN;
            element.Visibility = expanded ? Visibility.Visible : Visibility.Collapsed;
        };
        element.BeginAnimation(FrameworkElement.HeightProperty, animation);
    }
    private static void AttachDisclosure(DependencyObject target, DependencyPropertyChangedEventArgs e)
    {
        if (target is not Expander expander || !(bool)e.NewValue) return;
        void Change(bool animate)
        {
            expander.ApplyTemplate();
            if (expander.Template.FindName("Body", expander) is not FrameworkElement body) return;
            if (animate) AnimateDisclosure(body, expander.IsExpanded);
            else { body.BeginAnimation(FrameworkElement.HeightProperty, null); body.Height = double.NaN; body.Visibility = expander.IsExpanded ? Visibility.Visible : Visibility.Collapsed; }
            var chevron = Visuals<VectorIcon>(expander).FirstOrDefault(icon => icon.Kind == "chevron");
            if (chevron == null) return;
            var current = chevron.RenderTransform as RotateTransform;
            var from = current?.Angle ?? 0; var rotation = new RotateTransform(expander.IsExpanded ? 90 : 0); chevron.RenderTransform = rotation;
            if (animate && SystemParameters.ClientAreaAnimation) rotation.BeginAnimation(RotateTransform.AngleProperty, new DoubleAnimation(from, rotation.Angle, Duration(180)) { EasingFunction = Ease, FillBehavior = FillBehavior.Stop });
        }
        expander.Loaded += (_, _) => Change(false); expander.Expanded += (_, _) => Change(true); expander.Collapsed += (_, _) => Change(true);
    }
    internal static void OpenWindow(Window window, bool dialog)
    {
        if (!SystemParameters.ClientAreaAnimation || !double.IsFinite(window.Top)) return;
        var top = window.Top;
        // Move the native HWND; glyphs are never scaled or placed in a bitmap cache.
        window.BeginAnimation(Window.TopProperty, new DoubleAnimation(top + (dialog ? 18 : 8), top, Duration(dialog ? 380 : 260)) { EasingFunction = Ease, FillBehavior = FillBehavior.Stop });
    }
    internal static void Busy(FrameworkElement element, bool busy)
    {
        if (element is not Button button) return;
        button.ApplyTemplate();
        var icon = button.Template?.FindName("ActionIcon", button) as VectorIcon;
        if (icon?.RenderTransform is RotateTransform previous) previous.BeginAnimation(RotateTransform.AngleProperty, null);
        ButtonDesign.SetIsBusy(button, busy);
        if (icon == null) return;
        var rotation = new RotateTransform(); icon.RenderTransformOrigin = new(.5, .5); icon.RenderTransform = rotation;
        if (busy && SystemParameters.ClientAreaAnimation)
            rotation.BeginAnimation(RotateTransform.AngleProperty, new DoubleAnimation(0, 360, TimeSpan.FromMilliseconds(800)) { RepeatBehavior = RepeatBehavior.Forever });
    }

    internal static void SegmentSurface(RadioButton button)
    {
        SolidColorBrush? paint = null;
        void Update(object? sender, EventArgs e)
        {
            if (paint == null) return;
            var brush = button.IsChecked == true ? button.TryFindResource("SurfaceBrush") as Brush
                : button.IsMouseOver ? button.TryFindResource("HoverBrush") as Brush : Brushes.Transparent;
            FadeBrush(paint, ColorOf(brush));
        }
        button.Loaded += (_, _) =>
        {
            button.ApplyTemplate();
            if (button.Template.FindName("Segment", button) is Border border)
            { paint = new SolidColorBrush(ColorOf(border.Background)); border.Background = paint; }
            ThemeService.Changed += Update;
        };
        button.Checked += Update; button.Unchecked += Update; button.MouseEnter += (_, _) => Update(button, EventArgs.Empty); button.MouseLeave += (_, _) => Update(button, EventArgs.Empty);
        button.Unloaded += (_, _) => { ThemeService.Changed -= Update; paint = null; };
    }
    private static void AttachMenu(DependencyObject element, DependencyPropertyChangedEventArgs e)
    {
        if (!(bool)e.NewValue) return;
        if (element is ContextMenu menu)
            menu.Opened += (_, _) =>
            {
                var target = menu.VerticalOffset;
                if (SystemParameters.ClientAreaAnimation) menu.BeginAnimation(ContextMenu.VerticalOffsetProperty,
                    new DoubleAnimation(target - 6, target, Duration(240)) { EasingFunction = Ease, FillBehavior = FillBehavior.Stop });
            };
        if (element is ComboBox combo)
            combo.DropDownOpened += (_, _) =>
            {
                combo.ApplyTemplate();
                if (combo.Template.FindName("PART_Popup", combo) is Popup popup && SystemParameters.ClientAreaAnimation)
                    popup.BeginAnimation(Popup.VerticalOffsetProperty, new DoubleAnimation(-6, 0, Duration(240)) { EasingFunction = Ease, FillBehavior = FillBehavior.Stop });
            };
    }
}
