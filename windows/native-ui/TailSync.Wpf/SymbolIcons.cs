using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;

namespace TailSync.Wpf;

// Keep the public element name used by existing templates. Icons are Unicode
// font glyphs, rendered at their final size (never scaled bitmaps or emoji).
public sealed class VectorIcon : FrameworkElement
{
    public static readonly DependencyProperty KindProperty = DependencyProperty.Register(nameof(Kind), typeof(string), typeof(VectorIcon), new FrameworkPropertyMetadata("file", FrameworkPropertyMetadataOptions.AffectsRender));
    public static readonly DependencyProperty StrokeProperty = DependencyProperty.Register(nameof(Stroke), typeof(Brush), typeof(VectorIcon), new FrameworkPropertyMetadata(null, FrameworkPropertyMetadataOptions.AffectsRender));
    public static readonly DependencyProperty FilledProperty = DependencyProperty.Register(nameof(Filled), typeof(bool), typeof(VectorIcon), new FrameworkPropertyMetadata(false, FrameworkPropertyMetadataOptions.AffectsRender));
    public string Kind { get => (string)GetValue(KindProperty); set => SetValue(KindProperty, value); }
    public Brush? Stroke { get => (Brush?)GetValue(StrokeProperty); set => SetValue(StrokeProperty, value); }
    public bool Filled { get => (bool)GetValue(FilledProperty); set => SetValue(FilledProperty, value); }

    internal static readonly IReadOnlyDictionary<string, int> Symbols = new Dictionary<string, int>
    {
        ["search"] = 0xE721, ["pin"] = 0xE718, ["star"] = 0xE734,
        ["file"] = 0xE8A5, ["text"] = 0xE8A5, ["info"] = 0xE946,
        ["more"] = 0xE712, ["folder"] = 0xE8B7, ["link"] = 0xE71B,
        ["code"] = 0xE943, ["image"] = 0xEB9F, ["laptop"] = 0xE7F8,
        ["globe"] = 0xE774, ["shield"] = 0xEA18, ["refresh"] = 0xE72C,
        ["keyboard"] = 0xE765, ["edit"] = 0xE70F, ["close"] = 0xE711,
        ["arrow"] = 0xE8A7, ["minimize"] = 0xE921, ["maximize"] = 0xE922,
        ["restore"] = 0xE923, ["check"] = 0xE73E, ["chevron"] = 0xE76C,
        ["back"] = 0xE76B, ["down"] = 0xE70D, ["disk"] = 0xEDA2,
        ["download"] = 0xE896, ["latency"] = 0xE9D9, ["copy"] = 0xE8C8,
        ["delete"] = 0xE74D, ["add"] = 0xE710, ["subtract"] = 0xE738,
        ["rotate"] = 0xE7AD, ["fit"] = 0xE740, ["wrap"] = 0xE8A4,
        ["settings"] = 0xE713, ["history"] = 0xE81C, ["sync"] = 0xE895,
        ["save"] = 0xE74E, ["mail"] = 0xE715, ["devices"] = 0xE772,
        ["light"] = 0xE706, ["dark"] = 0xE708, ["system"] = 0xE770,
        ["calendar"] = 0xE787, ["warning"] = 0xE7BA, ["stop"] = 0xE71A,
    };
    private static readonly GlyphTypeface[] Faces = LoadFaces();
    private static GlyphTypeface[] LoadFaces()
    {
        var result = new List<GlyphTypeface>();
        foreach (var family in new[] { "Segoe Fluent Icons", "Segoe MDL2 Assets" })
            if (new Typeface(new FontFamily(family), FontStyles.Normal, FontWeights.Normal, FontStretches.Normal).TryGetGlyphTypeface(out var face)) result.Add(face);
        return result.ToArray();
    }
    internal static (GlyphTypeface Face, ushort Index) Resolve(string kind, bool filled = false)
    {
        var code = kind == "star" && filled ? 0xE735 : Symbols.GetValueOrDefault(kind, Symbols["file"]);
        foreach (var face in Faces)
            if (face.CharacterToGlyphMap.TryGetValue(code, out var index) && index != 0) return (face, index);
        throw new InvalidOperationException($"Windows symbol font missing U+{code:X4} ({kind}).");
    }
    static VectorIcon()
    {
        WidthProperty.OverrideMetadata(typeof(VectorIcon), new FrameworkPropertyMetadata(16d));
        HeightProperty.OverrideMetadata(typeof(VectorIcon), new FrameworkPropertyMetadata(16d));
        HorizontalAlignmentProperty.OverrideMetadata(typeof(VectorIcon), new FrameworkPropertyMetadata(HorizontalAlignment.Center));
        VerticalAlignmentProperty.OverrideMetadata(typeof(VectorIcon), new FrameworkPropertyMetadata(VerticalAlignment.Center));
        TextElement.ForegroundProperty.OverrideMetadata(typeof(VectorIcon), new FrameworkPropertyMetadata(Brushes.Gray, FrameworkPropertyMetadataOptions.Inherits | FrameworkPropertyMetadataOptions.AffectsRender));
    }
    public VectorIcon() { IsHitTestVisible = false; UseLayoutRounding = true; SnapsToDevicePixels = true; }
    protected override void OnDpiChanged(DpiScale oldDpi, DpiScale newDpi) { base.OnDpiChanged(oldDpi, newDpi); InvalidateVisual(); }
    protected override void OnRender(DrawingContext dc)
    {
        base.OnRender(dc);
        if (ActualWidth <= 0 || ActualHeight <= 0 || string.IsNullOrEmpty(Kind)) return;
        var (face, index) = Resolve(Kind, Filled);
        var dpi = VisualTreeHelper.GetDpi(this);
        var em = Math.Min(ActualWidth, ActualHeight);
        GlyphRun Run(Point origin) => new(face, 0, false, em, (float)dpi.PixelsPerDip, new[] { index }, origin,
            new[] { face.AdvanceWidths[index] * em }, null, null, null, null, null, null);
        var bounds = Run(new Point()).ComputeInkBoundingBox();
        var x = Math.Round(((ActualWidth - bounds.Width) / 2 - bounds.X) * dpi.DpiScaleX) / dpi.DpiScaleX;
        var y = Math.Round(((ActualHeight - bounds.Height) / 2 - bounds.Y) * dpi.DpiScaleY) / dpi.DpiScaleY;
        dc.DrawGlyphRun(Stroke ?? (Brush)GetValue(TextElement.ForegroundProperty), Run(new Point(x, y)));
    }
}

public static class ButtonDesign
{
    public static readonly DependencyProperty IconProperty = DependencyProperty.RegisterAttached("Icon", typeof(string), typeof(ButtonDesign), new PropertyMetadata(""));
    public static string GetIcon(DependencyObject element) => (string)element.GetValue(IconProperty);
    public static void SetIcon(DependencyObject element, string value) => element.SetValue(IconProperty, value);
    public static readonly DependencyProperty IsBusyProperty = DependencyProperty.RegisterAttached("IsBusy", typeof(bool), typeof(ButtonDesign), new PropertyMetadata(false));
    public static bool GetIsBusy(DependencyObject element) => (bool)element.GetValue(IsBusyProperty);
    public static void SetIsBusy(DependencyObject element, bool value) => element.SetValue(IsBusyProperty, value);
}
