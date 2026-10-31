using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using System.Windows.Media.Animation;

namespace TailSync.Wpf;

// A single moving background; labels remain at their original pixel positions.
internal sealed class ConnectionModeSelector : Border
{
    private readonly UniformGrid _buttons;
    private readonly Border _selection;
    private readonly TranslateTransform _position = new();
    internal ConnectionModeSelector(UniformGrid buttons)
    {
        _buttons = buttons; Width = 292; Height = 30; CornerRadius = new(7); BorderThickness = new(1);
        SetResourceReference(BackgroundProperty, "SurfaceBrush"); SetResourceReference(BorderBrushProperty, "BorderBrush");
        var layers = new Grid(); Child = layers;
        var dividers = new UniformGrid { Columns = buttons.Children.Count, IsHitTestVisible = false };
        for (var i = 0; i < buttons.Children.Count; i++)
        {
            var divider = new Border { BorderThickness = new(i == 0 ? 0 : 1, 0, 0, 0), Margin = new(0, 7, 0, 7) };
            divider.SetResourceReference(BorderBrushProperty, "BorderBrush"); dividers.Children.Add(divider);
        }
        layers.Children.Add(dividers);
        _selection = new Border { CornerRadius = new(6), HorizontalAlignment = HorizontalAlignment.Left, IsHitTestVisible = false, RenderTransform = _position };
        _selection.SetResourceReference(BackgroundProperty, "AccentBrush");
        _selection.Child = new Border { CornerRadius = new(6), Background = new LinearGradientBrush(Color.FromArgb(30, 255, 255, 255), Color.FromArgb(28, 0, 0, 0), 90) };
        layers.Children.Add(_selection); layers.Children.Add(buttons);
        Loaded += (_, _) => Update(false); SizeChanged += (_, _) => Update(false);
        foreach (RadioButton button in buttons.Children) button.Checked += (_, _) => Update(true);
    }
    private void Update(bool animate)
    {
        var index = _buttons.Children.OfType<RadioButton>().ToList().FindIndex(button => button.IsChecked == true);
        if (index < 0 || ActualWidth <= 2) return;
        var width = (ActualWidth - 2) / _buttons.Children.Count;
        _selection.Width = width;
        var dpi = VisualTreeHelper.GetDpi(this).DpiScaleX;
        var target = Math.Round(index * width * dpi) / dpi;
        var from = _position.X; _position.BeginAnimation(TranslateTransform.XProperty, null); _position.X = target;
        if (animate && IsLoaded && SystemParameters.ClientAreaAnimation)
            _position.BeginAnimation(TranslateTransform.XProperty, new DoubleAnimation(from, target, TimeSpan.FromMilliseconds(140))
            { EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut }, FillBehavior = FillBehavior.Stop });
    }
}
