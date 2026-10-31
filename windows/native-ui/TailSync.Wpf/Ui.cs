using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Shell;
using System.Windows.Shapes;
using System.Windows.Threading;
using TailSync.Native.Core;
namespace TailSync.Wpf;

internal static class Ui
{
    public static Brush Brush(string key) => (Brush)Application.Current.FindResource(key + "Brush");
    public static T Name<T>(T element, string name) where T : DependencyObject { AutomationProperties.SetName(element, name); return element; }
    public static TextBlock Text(string value, double size = 14, string color = "Primary")
    {
        var label = new TextBlock { Text = value, FontSize = size, TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center };
        label.SetResourceReference(TextBlock.ForegroundProperty, color + "Brush"); return label;
    }
    public static TextBlock Detail(string value, string color = "Secondary")
    {
        var label = Text(value, 13, color); label.SetResourceReference(TextBlock.FontSizeProperty, "DetailFontSize"); return label;
    }
    public static StackPanel Stack(Orientation orientation = Orientation.Vertical, double gap = 0, params UIElement[] children)
    {
        var panel = new StackPanel { Orientation = orientation };
        foreach (var child in children) { if (child is FrameworkElement f && panel.Children.Count > 0 && gap > 0) { var m=f.Margin; f.Margin = orientation == Orientation.Horizontal ? new(m.Left+gap,m.Top,m.Right,m.Bottom) : new(m.Left,m.Top+gap,m.Right,m.Bottom); } panel.Children.Add(child); }
        return panel;
    }
    public static Button Button(string label, Action? action = null, string? style = null, string? icon = null)
    {
        var b = Name(new Button { Content = label }, label);
        ControlMotion.ButtonSurface(b);
        if (icon != null) ButtonDesign.SetIcon(b, icon);
        if (style != null) b.SetResourceReference(FrameworkElement.StyleProperty, style);
        if (action != null) b.Click += (_, _) => action(); return b;
    }
    public static Button IconButton(string icon, string name, Action action)
    {
        var button = Button(name, action, "IconButton"); button.Content = null; ButtonDesign.SetIcon(button, icon); button.ToolTip = name; return button;
    }
    public static Border Surface(UIElement content, Thickness? padding = null)
    {
        var border = new Border { Child = content, CornerRadius = new(10), BorderThickness = new(1), Padding = padding ?? new(0) };
        border.SetResourceReference(Border.BackgroundProperty, "SurfaceBrush"); border.SetResourceReference(Border.BorderBrushProperty, "BorderBrush"); border.SetResourceReference(Border.CornerRadiusProperty,"SurfaceRadius"); return border;
    }
    // Shadow only the empty backing shape: applying an Effect to the content
    // would rasterize the text and lose ClearType at fractional DPI scales.
    public static Grid SettingsCard(UIElement content)
    {
        var card = new Grid();
        var shadow = Surface(new Grid());
        shadow.IsHitTestVisible = false;
        shadow.SetResourceReference(FrameworkElement.StyleProperty, "CardShadow");
        var foreground = Surface(content);
        foreground.UseLayoutRounding = true; foreground.SnapsToDevicePixels = true;
        RenderOptions.SetClearTypeHint(foreground, ClearTypeHint.Enabled);
        card.Children.Add(shadow); card.Children.Add(foreground); return card;
    }
    public static StackPanel PageHeader(string title, string description)
    {
        var heading = Text(title); heading.ClearValue(TextBlock.FontSizeProperty); heading.SetResourceReference(FrameworkElement.StyleProperty, "SectionTitle"); heading.Margin = new(0);
        var caption = Text(description, 13, "Secondary");
        return Stack(Orientation.Vertical, 6, heading, caption);
    }
    public static Grid PreferenceRow(UIElement copy, UIElement action, string? icon = null, double padding = 15)
    {
        var row = new AdaptiveSettingRow { Margin = new(16, padding, 16, padding) };
        if (icon != null)
        {
            var label = new Grid(); label.ColumnDefinitions.Add(new() { Width = new GridLength(30) }); label.ColumnDefinitions.Add(new());
            var glyph = new VectorIcon { Kind = icon, Width = 18, Height = 18, HorizontalAlignment = HorizontalAlignment.Left };
            glyph.SetResourceReference(VectorIcon.StrokeProperty, "AccentBrush"); label.Children.Add(glyph);
            Grid.SetColumn(copy, 1); label.Children.Add(copy); copy = label;
        }
        if (copy is FrameworkElement text) { text.Margin = new(0, 0, 14, 0); text.VerticalAlignment = VerticalAlignment.Center; }
        row.Children.Add(copy); Grid.SetColumn(action, 1); row.Children.Add(action); ControlMotion.Preference(row, action); return row;
    }
    public static StackPanel Label(string title, string description = "", bool strong = false)
    {
        var label = Text(title); label.SetResourceReference(TextBlock.FontSizeProperty, "ThemePanelDefaultFontSize");
        if (strong) label.FontWeight = FontWeights.SemiBold;
        else label.SetResourceReference(TextBlock.FontWeightProperty, "ThemePanelDefaultFontWeight");
        var copy = Stack(Orientation.Vertical, 3, label);
        if (description.Length > 0) copy.Children.Add(Detail(description));
        return copy;
    }
    public static void Divider(Panel rows) { var line = new Border { Height = 1, Margin = new(16,0,16,0) }; line.SetResourceReference(Border.BackgroundProperty, "DividerBrush"); rows.Children.Add(line); }
    public static DrawingImage Landscape()
    {
        var group = new DrawingGroup();
        foreach (var (fill, path) in new[] {
            ("#DCE8E5", "M0 0h800v520H0z"), ("#ABBDB3", "M0 300L183 99l157 172L490 158l199 145 111-88v305H0z"),
            ("#768E83", "M0 321l240-148 165 188 170-112 225 92v179H0z"),
            ("#B6C7B9", "M0 344c195-19 325 32 470 11s195-7 330-16v181H0z"),
            ("#426254", "M0 373c93-33 194-33 291 22L126 520H0zm800 23c-57-32-109-23-172 35l-55 89h227z") })
            {
            Brush brush=fill switch { "#DCE8E5" => new LinearGradientBrush((Color)ColorConverter.ConvertFromString("#DCE8E5"),(Color)ColorConverter.ConvertFromString("#F2ECE0"),90),
                "#B6C7B9" => new LinearGradientBrush((Color)ColorConverter.ConvertFromString("#9BAFA7"),(Color)ColorConverter.ConvertFromString("#C7D3C5"),90),
                _ => (Brush)new BrushConverter().ConvertFromString(fill)! };
            group.Children.Add(new GeometryDrawing(brush,null,Geometry.Parse(path)));
        }
        group.Children.Insert(1, new GeometryDrawing((Brush)new BrushConverter().ConvertFromString("#F9F4DF")!, null, new EllipseGeometry(new Point(614,114),43,43)));
        group.Children.Add(new GeometryDrawing(null,new Pen(new SolidColorBrush(Color.FromArgb(153,227,231,212)),2),Geometry.Parse("M339 405h218m-265 22h239m-333 43h332")));
        group.Freeze(); return new DrawingImage(group);
    }
}

public class MacWindow : Window
{
    private HwndSource? _source;
    private DispatcherOperation? _resizeRepaint;
    private bool _entered;
    public ContextMenu? CaptionMenu { get; set; }
    public MacWindow()
    {
        WindowStartupLocation=WindowStartupLocation.CenterScreen;
        SetResourceReference(StyleProperty,"MacWindow");
        WindowChrome.SetWindowChrome(this,new WindowChrome { CaptionHeight=32, ResizeBorderThickness=new(5), CornerRadius=new(12), GlassFrameThickness=new(0), UseAeroCaptionButtons=false });
        SourceInitialized += (_,_) => { var hwnd=new WindowInteropHelper(this).Handle;_source=HwndSource.FromHwnd(hwnd);_source?.AddHook(WindowMessage);UpdateRenderBackground();if (OperatingSystem.IsWindowsVersionAtLeast(10,0,22000)) { var preference=2; DwmSetWindowAttribute(hwnd,33,ref preference,sizeof(int)); } };
        SizeChanged += (_, e) => { if (e.WidthChanged || e.HeightChanged) QueueResizeRepaint(); };
        Closed += (_,_) => { _resizeRepaint?.Abort();_resizeRepaint=null;_source?.RemoveHook(WindowMessage);_source=null; };
        ContentRendered += (_, _) =>
        {
            if (_entered) return; _entered = true;
            if (this is MainWindow { Page: "settings" or "connections" } or PairingWindow or DemoDialog)
                ControlMotion.OpenWindow(this, this is PairingWindow or DemoDialog);
        };
    }
    private IntPtr WindowMessage(IntPtr hwnd,int message,IntPtr wParam,IntPtr lParam,ref bool handled)
    {
        if (message == 0x0046 && lParam != 0) // WM_WINDOWPOSCHANGING
        {
            var position = Marshal.PtrToStructure<WindowPosition>(lParam);
            if ((position.Flags & 0x0001) == 0 && GetWindowRect(hwnd, out var previous)
                && (position.Width != previous.Right - previous.Left || position.Height != previous.Bottom - previous.Top))
            {
                // A retained WPF scene reflows its right/center-aligned content.
                // USER32 must not copy the previous client pixels to new positions
                // while WPF is composing that scene at the new size.
                position.Flags |= 0x0100; // SWP_NOCOPYBITS
                Marshal.StructureToPtr(position, lParam, false);
            }
        }
        else if (message == 0x0231) // WM_ENTERSIZEMOVE
        {
            // The opening HWND animation and a pending wheel motion must not
            // compete with the user's native move/resize loop.
            if (DependencyPropertyHelper.GetValueSource(this, TopProperty).IsAnimated)
            {
                var top = Top; BeginAnimation(TopProperty, null);
                if (double.IsFinite(top)) SetCurrentValue(TopProperty, top);
            }
            foreach (var host in ControlMotion.Visuals<FrameworkElement>(this).Where(SmoothScroll.GetIsEnabled)) SmoothScroll.Cancel(host);
        }
        else if (message == 0x0232) QueueResizeRepaint(); // WM_EXITSIZEMOVE
        // WindowChrome consumes caption hit tests, so use the non-client message
        // to make the review window menu available without adding product chrome.
        if(message==0x00A5 && wParam.ToInt64()==2 && CaptionMenu is { } menu) { menu.PlacementTarget=this;menu.Placement=System.Windows.Controls.Primitives.PlacementMode.MousePoint;menu.IsOpen=true;handled=true; }
        return IntPtr.Zero;
    }
    protected override void OnPropertyChanged(DependencyPropertyChangedEventArgs e)
    {
        base.OnPropertyChanged(e);
        if (e.Property == BackgroundProperty) UpdateRenderBackground();
    }
    private void UpdateRenderBackground()
    {
        // Match the native render target's clear color to the opaque themed
        // canvas, including the newly exposed area before its first WPF frame.
        if (_source?.CompositionTarget is HwndTarget target && Background is SolidColorBrush { Color.A: 255, Opacity: 1 } brush)
            target.BackgroundColor = brush.Color;
    }
    private void QueueResizeRepaint()
    {
        if (_source == null || _resizeRepaint is { Status: DispatcherOperationStatus.Pending }) return;
        _resizeRepaint = Dispatcher.BeginInvoke(DispatcherPriority.Render, new Action(() =>
        {
            _resizeRepaint = null;
            if (_source is { IsDisposed: false } source && IsVisible && WindowState != WindowState.Minimized)
                // Invalidate the entire native client after layout, not only its
                // newly exposed strips. Leave WPF's synchronized resize/present
                // path intact; no background erase or nested UpdateWindow loop.
                InvalidateRect(source.Handle, 0, false);
        }));
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct WindowPosition { public nint Window, InsertAfter; public int X, Y, Width, Height; public uint Flags; }
    [StructLayout(LayoutKind.Sequential)]
    private struct NativeRect { public int Left, Top, Right, Bottom; }
    protected Grid MakeTitleBar(string title)
    {
        var bar=new Grid { Height=32, Background=Brushes.Transparent };
        bar.ColumnDefinitions.Add(new());bar.ColumnDefinitions.Add(new(){Width=GridLength.Auto});
        var branding=new DockPanel{Margin=new(14,0,8,0)};
        var logo=new Image{Width=18,Height=18,Stretch=Stretch.Uniform,Margin=new(0,0,8,0)};
        logo.SetResourceReference(Image.SourceProperty,"ThemeLogoImage");
        var logoStyle=new Style(typeof(Image));var missingLogo=new Trigger{Property=Image.SourceProperty,Value=null};missingLogo.Setters.Add(new Setter(VisibilityProperty,Visibility.Collapsed));logoStyle.Triggers.Add(missingLogo);logo.Style=logoStyle;
        DockPanel.SetDock(logo,Dock.Left);branding.Children.Add(logo);
        var heading=Ui.Text(title,12,"Secondary");heading.SetBinding(TextBlock.TextProperty,new System.Windows.Data.Binding(nameof(Title)){Source=this});heading.FontWeight=FontWeights.SemiBold;heading.TextWrapping=TextWrapping.NoWrap;heading.TextTrimming=TextTrimming.CharacterEllipsis;branding.Children.Add(heading);bar.Children.Add(branding);
        var controls=new StackPanel { Orientation=Orientation.Horizontal, HorizontalAlignment=HorizontalAlignment.Right };
        Button CaptionButton(string icon,string label,Action action,string style="WindowCaptionButton")
        {
            var button=Ui.Button(label,action,style);button.ToolTip=label;
            var glyph=new VectorIcon { Kind=icon,Width=14,Height=14 };
            glyph.SetBinding(VectorIcon.StrokeProperty,new System.Windows.Data.Binding(nameof(Control.Foreground)){Source=button});
            button.Content=glyph;WindowChrome.SetIsHitTestVisibleInChrome(button,true);controls.Children.Add(button);return button;
        }
        var minimize=CaptionButton("minimize","最小化窗口",()=>SystemCommands.MinimizeWindow(this));
        minimize.Visibility=ResizeMode==ResizeMode.NoResize&&!ShowInTaskbar?Visibility.Collapsed:Visibility.Visible;
        var maximize=CaptionButton("maximize","最大化窗口",()=>{if(WindowState==WindowState.Maximized)SystemCommands.RestoreWindow(this);else SystemCommands.MaximizeWindow(this);});
        maximize.Visibility = ResizeMode is ResizeMode.CanResize or ResizeMode.CanResizeWithGrip ? Visibility.Visible : Visibility.Collapsed;
        StateChanged+=(_,_)=>{
            var restored=WindowState==WindowState.Maximized;((VectorIcon)maximize.Content).Kind=restored?"restore":"maximize";
            maximize.ToolTip=restored?"还原窗口":"最大化窗口";AutomationProperties.SetName(maximize,(string)maximize.ToolTip);
        };
        CaptionButton("close","关闭窗口",()=>SystemCommands.CloseWindow(this),"WindowCloseButton");
        Grid.SetColumn(controls,1);bar.Children.Add(controls);return bar;
    }
    [DllImport("dwmapi.dll")] private static extern int DwmSetWindowAttribute(IntPtr window,int attribute,ref int value,int size);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)] private static extern bool GetWindowRect(nint window, out NativeRect rectangle);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)] private static extern bool InvalidateRect(nint window, nint rectangle, [MarshalAs(UnmanagedType.Bool)] bool erase);
}
