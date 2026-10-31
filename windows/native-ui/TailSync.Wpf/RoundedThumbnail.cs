using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace TailSync.Wpf;

// Clip the fitted bitmap itself. Clipping a square container leaves wide and
// tall images square where their letterboxed corners actually land.
public sealed class RoundedThumbnail : Image
{
    public static readonly DependencyProperty CornerRadiusProperty = DependencyProperty.Register(nameof(CornerRadius), typeof(CornerRadius), typeof(RoundedThumbnail),
        new FrameworkPropertyMetadata(new CornerRadius(8), FrameworkPropertyMetadataOptions.AffectsRender));
    public CornerRadius CornerRadius { get => (CornerRadius)GetValue(CornerRadiusProperty); set => SetValue(CornerRadiusProperty, value); }
    internal Rect FittedBounds
    {
        get
        {
            if (Source == null || Source.Width <= 0 || Source.Height <= 0 || !double.IsFinite(Source.Width) || !double.IsFinite(Source.Height)) return Rect.Empty;
            var scale = Math.Min(RenderSize.Width / Source.Width, RenderSize.Height / Source.Height);
            var width = Source.Width * scale; var height = Source.Height * scale;
            return new((RenderSize.Width - width) / 2, (RenderSize.Height - height) / 2, width, height);
        }
    }
    protected override void OnDpiChanged(DpiScale oldDpi, DpiScale newDpi) { base.OnDpiChanged(oldDpi, newDpi); InvalidateVisual(); }
    protected override void OnRender(DrawingContext drawing)
    {
        var bounds = FittedBounds;
        if (bounds.IsEmpty || bounds.Width <= 0 || bounds.Height <= 0) return;
        var radius = Math.Min(CornerRadius.TopLeft, Math.Min(bounds.Width, bounds.Height) / 2);
        drawing.PushClip(new RectangleGeometry(bounds, radius, radius));
        drawing.DrawImage(Source, bounds); drawing.Pop();
    }
}
