using System.Windows;
using System.Windows.Documents;
using System.Windows.Media;
using System.Windows.Media.Animation;

namespace TailSync.Wpf;

// The dimmed backing is a separate drawing, never a blur/effect on owner text.
internal sealed class ModalShade : Adorner, IDisposable
{
    private readonly AdornerLayer _layer;
    private ModalShade(UIElement content, AdornerLayer layer) : base(content)
    {
        _layer = layer; IsHitTestVisible = false; layer.Add(this);
        if (SystemParameters.ClientAreaAnimation) BeginAnimation(OpacityProperty,
            new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(160)) { FillBehavior = FillBehavior.Stop });
    }
    internal static ModalShade? Show(Window owner) => owner.Content is UIElement content && AdornerLayer.GetAdornerLayer(content) is { } layer ? new(content, layer) : null;
    protected override void OnRender(DrawingContext context)
    {
        var brush = (TryFindResource("CanvasBrush") as Brush ?? Brushes.Gray).CloneCurrentValue(); brush.Opacity *= .58;
        context.DrawRectangle(brush, null, new Rect(AdornedElement.RenderSize));
    }
    public void Dispose() { BeginAnimation(OpacityProperty, null); _layer.Remove(this); }
}
