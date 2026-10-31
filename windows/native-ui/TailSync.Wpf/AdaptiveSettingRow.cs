using System.Windows;
using System.Windows.Controls;

namespace TailSync.Wpf;

// Keep descriptions readable when Windows fonts, translated labels, or a narrow
// window need more space. Measure the action at its natural size before deciding.
internal sealed class AdaptiveSettingRow : Grid
{
    private bool _stacked;
    internal bool IsStacked => _stacked;

    internal AdaptiveSettingRow()
    {
        ColumnDefinitions.Add(new());
        ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        RowDefinitions.Add(new() { Height = GridLength.Auto });
        RowDefinitions.Add(new() { Height = GridLength.Auto });
    }

    protected override Size MeasureOverride(Size constraint)
    {
        if (Children.Count == 2 && Children[1] is FrameworkElement action)
        {
            action.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
            var stacked = constraint.Width - action.DesiredSize.Width < 190;
            _stacked = stacked;
            SetColumn(action, stacked ? 0 : 1);
            SetRow(action, stacked ? 1 : 0);
            SetColumnSpan(action, stacked ? 2 : 1);
            SetColumnSpan(Children[0], stacked ? 2 : 1);
            action.HorizontalAlignment = stacked ? HorizontalAlignment.Left : HorizontalAlignment.Right;
            // Only the vertical margin changes, so natural width stays stable.
            action.Margin = new Thickness(0, stacked ? 10 : 0, 0, 0);
        }
        return base.MeasureOverride(constraint);
    }
}
