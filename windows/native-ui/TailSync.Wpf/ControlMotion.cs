using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using System.Windows.Media.Animation;

namespace TailSync.Wpf;

public static partial class ControlMotion
{
    public static readonly DependencyProperty SwitchProperty = DependencyProperty.RegisterAttached("Switch", typeof(bool), typeof(ControlMotion), new PropertyMetadata(false, Attach));
    public static void SetSwitch(DependencyObject target, bool value) => target.SetValue(SwitchProperty, value);
    public static bool GetSwitch(DependencyObject target) => (bool)target.GetValue(SwitchProperty);
    private static void Attach(DependencyObject target, DependencyPropertyChangedEventArgs e)
    {
        if (target is not ToggleButton toggle) return;
        if ((bool)e.NewValue) { toggle.Loaded += Loaded; toggle.Checked += Changed; toggle.Unchecked += Changed; toggle.SizeChanged += SwitchSizeChanged; }
        else { toggle.Loaded -= Loaded; toggle.Checked -= Changed; toggle.Unchecked -= Changed; toggle.SizeChanged -= SwitchSizeChanged; }
    }
    private static void Loaded(object sender, RoutedEventArgs e) => Move((ToggleButton)sender, false);
    private static void Changed(object sender, RoutedEventArgs e) => Move((ToggleButton)sender, true);
    private static void SwitchSizeChanged(object sender, SizeChangedEventArgs e) => Move((ToggleButton)sender, false);
    private static void Move(ToggleButton toggle, bool animate)
    {
        if (toggle.Template?.FindName("Knob", toggle) is not Border knob) return;
        if (knob.RenderTransform is not TranslateTransform current || current.IsFrozen) knob.RenderTransform = new TranslateTransform();
        var transform = (TranslateTransform)knob.RenderTransform;
        var width = toggle.ActualWidth > 0 ? toggle.ActualWidth : double.IsFinite(toggle.Width) ? toggle.Width : 40;
        var target = toggle.IsChecked == true ? Math.Max(0, width - 4 - knob.Width) : 0d;
        var from = transform.X; transform.BeginAnimation(TranslateTransform.XProperty, null); transform.X = target;
        if (animate && toggle.IsLoaded && SystemParameters.ClientAreaAnimation)
            transform.BeginAnimation(TranslateTransform.XProperty, new DoubleAnimation(from, target, TimeSpan.FromMilliseconds(180)) { EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut }, FillBehavior = FillBehavior.Stop });
    }
    internal static void Reveal(UIElement element)
    {
        if (element is FrameworkElement content) AnimateDisclosure(content, true);
    }
}

// Only the decoration animates. Text stays at its original pixel coordinates.
public sealed class FavoriteFeedback : FrameworkElement
{
    private System.Windows.Threading.DispatcherOperation? _update;
    private int _pendingCompletion;
    public static readonly DependencyProperty PinnedProperty = DependencyProperty.Register(nameof(Pinned), typeof(bool), typeof(FavoriteFeedback), new PropertyMetadata(false, Changed));
    public static readonly DependencyProperty ChargeProperty = DependencyProperty.Register(nameof(Charge), typeof(double), typeof(FavoriteFeedback), new PropertyMetadata(0d, Changed));
    public static readonly DependencyProperty CompletionProperty = DependencyProperty.Register(nameof(Completion), typeof(int), typeof(FavoriteFeedback), new PropertyMetadata(0, Changed));
    public static readonly DependencyProperty StampProperty = DependencyProperty.Register(nameof(Stamp), typeof(bool), typeof(FavoriteFeedback), new FrameworkPropertyMetadata(false, FrameworkPropertyMetadataOptions.AffectsRender));
    public static readonly DependencyProperty AccentProperty = DependencyProperty.Register(nameof(Accent), typeof(Brush), typeof(FavoriteFeedback), new FrameworkPropertyMetadata(Brushes.OrangeRed, FrameworkPropertyMetadataOptions.AffectsRender));
    private static readonly DependencyProperty AmountProperty = DependencyProperty.Register("Amount", typeof(double), typeof(FavoriteFeedback), new FrameworkPropertyMetadata(0d, FrameworkPropertyMetadataOptions.AffectsRender));
    private static readonly DependencyProperty PopProperty = DependencyProperty.Register("Pop", typeof(double), typeof(FavoriteFeedback), new FrameworkPropertyMetadata(1d, FrameworkPropertyMetadataOptions.AffectsRender));
    private static readonly DependencyProperty RemovalProperty = DependencyProperty.Register("Removal", typeof(double), typeof(FavoriteFeedback), new FrameworkPropertyMetadata(0d, FrameworkPropertyMetadataOptions.AffectsRender));
    public bool Pinned { get => (bool)GetValue(PinnedProperty); set => SetValue(PinnedProperty, value); }
    public double Charge { get => (double)GetValue(ChargeProperty); set => SetValue(ChargeProperty, value); }
    public int Completion { get => (int)GetValue(CompletionProperty); set => SetValue(CompletionProperty, value); }
    public bool Stamp { get => (bool)GetValue(StampProperty); set => SetValue(StampProperty, value); }
    public Brush Accent { get => (Brush)GetValue(AccentProperty); set => SetValue(AccentProperty, value); }
    internal double VisibleAmount => (double)GetValue(AmountProperty);
    internal double VisibleRemoval => (double)GetValue(RemovalProperty);
    private static readonly Geometry Star = Geometry.Parse("M8 1L10 5.5L15 6L11.2 9.3L12.3 14L8 11.5L3.7 14L4.8 9.3L1 6L6 5.5Z");
    public FavoriteFeedback()
    {
        IsHitTestVisible = false; SetResourceReference(AccentProperty, "AccentBrush");
        Loaded += (_, _) => Update(false, 0);
        DataContextChanged += (_, _) => ResetMotion();
        Unloaded += (_, _) => ResetMotion();
    }
    private void ResetMotion() { _update?.Abort(); _update = null; _pendingCompletion = 0; BeginAnimation(AmountProperty, null); BeginAnimation(PopProperty, null); BeginAnimation(RemovalProperty, null); }
    private static void Changed(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        var feedback = (FavoriteFeedback)d;
        if (!feedback.IsLoaded) { feedback.Update(false, 0); return; }
        if (e.Property == CompletionProperty) feedback._pendingCompletion = (int)e.NewValue;
        if (feedback._update is { Status: System.Windows.Threading.DispatcherOperationStatus.Pending }) return;
        // Pinned, completion and charge are one semantic transition but arrive
        // through separate bindings. Sample the animation only after all three
        // settle, rather than restarting it from a not-yet-ticked base value.
        feedback._update = feedback.Dispatcher.BeginInvoke(System.Windows.Threading.DispatcherPriority.Render, new Action(() =>
        {
            feedback._update = null; var completion = feedback._pendingCompletion; feedback._pendingCompletion = 0;
            feedback.Update(true, completion);
        }));
    }
    private void Update(bool animate, int completion)
    {
        var target = Stamp ? (Completion < 0 ? 0d : Pinned || Completion > 0 ? 1d : 0d)
            : Completion < 0 ? 0d : Charge > 0 ? Pinned ? 0d : 1d : Pinned ? 1d : 0d;
        var from = VisibleAmount; BeginAnimation(AmountProperty, null); SetValue(AmountProperty, target);
        if (animate && IsLoaded && SystemParameters.ClientAreaAnimation)
        {
            var duration = Completion < 0 ? 420 : Stamp ? 180 : Charge > 0 ? 420 : 120;
            BeginAnimation(AmountProperty, new DoubleAnimation(from, target, TimeSpan.FromMilliseconds(duration)) { FillBehavior = FillBehavior.Stop });
            if (Stamp && completion > 0)
            {
                BeginAnimation(RemovalProperty, null); SetValue(RemovalProperty, 0d); SetValue(PopProperty, 1d);
                var bounce = new DoubleAnimationUsingKeyFrames { Duration = TimeSpan.FromMilliseconds(360), FillBehavior = FillBehavior.Stop };
                bounce.KeyFrames.Add(new LinearDoubleKeyFrame(.55, KeyTime.FromPercent(0)));
                bounce.KeyFrames.Add(new SplineDoubleKeyFrame(1.18, KeyTime.FromPercent(.65), new KeySpline(.16, 1, .3, 1)));
                bounce.KeyFrames.Add(new LinearDoubleKeyFrame(1, KeyTime.FromPercent(1)));
                BeginAnimation(PopProperty, bounce);
            }
            else if (Stamp && completion < 0)
            {
                var scale = (double)GetValue(PopProperty); BeginAnimation(PopProperty, null); SetValue(PopProperty, .65d);
                var release = new DoubleAnimationUsingKeyFrames { Duration = TimeSpan.FromMilliseconds(420), FillBehavior = FillBehavior.Stop };
                release.KeyFrames.Add(new LinearDoubleKeyFrame(scale, KeyTime.FromPercent(0)));
                release.KeyFrames.Add(new SplineDoubleKeyFrame(1.12, KeyTime.FromPercent(.25), new KeySpline(.16, 1, .3, 1)));
                release.KeyFrames.Add(new SplineDoubleKeyFrame(.65, KeyTime.FromPercent(1), new KeySpline(.4, 0, .2, 1)));
                BeginAnimation(PopProperty, release);
                BeginAnimation(RemovalProperty, null); SetValue(RemovalProperty, 1d);
                BeginAnimation(RemovalProperty, new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(420)) { FillBehavior = FillBehavior.Stop });
            }
            else if (Stamp && Completion == 0)
            {
                BeginAnimation(RemovalProperty, null); SetValue(RemovalProperty, 0d);
                var scale = (double)GetValue(PopProperty); var resting = Pinned && Charge > 0 ? .82d : 1d;
                BeginAnimation(PopProperty, null); SetValue(PopProperty, resting);
                BeginAnimation(PopProperty, new DoubleAnimation(scale, resting, TimeSpan.FromMilliseconds(180)) { FillBehavior = FillBehavior.Stop });
            }
        }
        else { BeginAnimation(PopProperty, null); SetValue(PopProperty, 1d); BeginAnimation(RemovalProperty, null); SetValue(RemovalProperty, Completion < 0 ? 1d : 0d); }
    }
    protected override void OnRender(DrawingContext dc)
    {
        var amount = Math.Clamp(VisibleAmount, 0, 1); if (amount <= 0 || ActualWidth <= 0 || ActualHeight <= 0) return;
        if (Stamp)
        {
            var scale = (double)GetValue(PopProperty);
            var removal = Math.Clamp(VisibleRemoval, 0, 1);
            dc.PushOpacity(amount * .9); dc.PushTransform(new ScaleTransform(scale, scale, ActualWidth / 2, ActualHeight / 2));
            dc.PushTransform(new RotateTransform((scale - 1) * 32 - removal * 12, ActualWidth / 2, ActualHeight / 2));
            dc.PushTransform(new TranslateTransform((ActualWidth - 16) / 2, (ActualHeight - 16) / 2 - removal * 6));
            dc.PushOpacity(1 - removal); dc.DrawGeometry(Accent, null, Star); dc.Pop();
            if (removal > 0) { dc.PushOpacity(removal); dc.DrawGeometry(null, new Pen(Accent, 1.25), Star); dc.Pop(); }
            dc.Pop(); dc.Pop(); dc.Pop(); dc.Pop();
        }
        else
        {
            var radius = TryFindResource("SurfaceRadius") is CornerRadius corners ? corners.TopLeft : 8;
            dc.PushClip(new RectangleGeometry(new Rect(RenderSize), radius, radius));
            dc.PushOpacity(.12); dc.DrawRectangle(Accent, null, new Rect(0, 0, ActualWidth * amount, ActualHeight)); dc.Pop(); dc.Pop();
        }
    }
}
