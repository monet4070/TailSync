using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Shell;
using System.Windows.Shapes;
using TailSync.Native.Core;
namespace TailSync.Wpf;

internal static class Ui
{
    public static Brush Brush(string key) => (Brush)Application.Current.FindResource(key + "Brush");
    public static T Name<T>(T element, string name) where T : DependencyObject { AutomationProperties.SetName(element, name); return element; }
    public static TextBlock Text(string value, double size = 13, string color = "Primary")
    {
        var label = new TextBlock { Text = value, FontSize = size, TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center };
        label.SetResourceReference(TextBlock.ForegroundProperty, color + "Brush"); return label;
    }
    public static StackPanel Stack(Orientation orientation = Orientation.Vertical, double gap = 0, params UIElement[] children)
    {
        var panel = new StackPanel { Orientation = orientation };
        foreach (var child in children) { if (child is FrameworkElement f && panel.Children.Count > 0 && gap > 0) { var m=f.Margin; f.Margin = orientation == Orientation.Horizontal ? new(m.Left+gap,m.Top,m.Right,m.Bottom) : new(m.Left,m.Top+gap,m.Right,m.Bottom); } panel.Children.Add(child); }
        return panel;
    }
    public static Button Button(string label, Action? action = null, string? style = null)
    {
        var b = Name(new Button { Content = label }, label);
        if (style != null) b.SetResourceReference(FrameworkElement.StyleProperty, style);
        if (action != null) b.Click += (_, _) => action(); return b;
    }
    public static Button IconButton(string icon, string name, Action action)
    {
        var button = Button(name, action, "IconButton"); button.Content = new VectorIcon { Kind = icon }; button.ToolTip = name; return button;
    }
    public static Border Surface(UIElement content, Thickness? padding = null)
    {
        var border = new Border { Child = content, CornerRadius = new(10), BorderThickness = new(1), Padding = padding ?? new(0) };
        border.SetResourceReference(Border.BackgroundProperty, "SurfaceBrush"); border.SetResourceReference(Border.BorderBrushProperty, "BorderBrush"); return border;
    }
    public static void Divider(Panel rows) { var line = new Border { Height = 1, Margin = new(16,0,0,0) }; line.SetResourceReference(Border.BackgroundProperty, "DividerBrush"); rows.Children.Add(line); }
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

public sealed class VectorIcon : FrameworkElement
{
    public static readonly DependencyProperty KindProperty = DependencyProperty.Register(nameof(Kind), typeof(string), typeof(VectorIcon), new FrameworkPropertyMetadata("file", FrameworkPropertyMetadataOptions.AffectsRender));
    public static readonly DependencyProperty StrokeProperty = DependencyProperty.Register(nameof(Stroke), typeof(Brush), typeof(VectorIcon), new FrameworkPropertyMetadata(Brushes.Gray, FrameworkPropertyMetadataOptions.AffectsRender));
    public static readonly DependencyProperty FilledProperty = DependencyProperty.Register(nameof(Filled), typeof(bool), typeof(VectorIcon), new FrameworkPropertyMetadata(false, FrameworkPropertyMetadataOptions.AffectsRender));
    public bool Filled { get => (bool)GetValue(FilledProperty); set => SetValue(FilledProperty,value); }
    public string Kind { get => (string)GetValue(KindProperty); set => SetValue(KindProperty,value); }
    public Brush Stroke { get => (Brush)GetValue(StrokeProperty); set => SetValue(StrokeProperty,value); }
    private static readonly Dictionary<string,Geometry> Shapes = new Dictionary<string,string> {
        ["search"]="M10 6A4 4 0 1 1 2 6A4 4 0 1 1 10 6M9 9L14 14", ["pin"]="M5 2L11 2M6 2L6 6L4 9L12 9L10 6L10 2M8 9L8 15",
        ["star"]="M8 1L10 5.5L15 6L11.2 9.3L12.3 14L8 11.5L3.7 14L4.8 9.3L1 6L6 5.5Z",
        ["file"]="M4 1L10 1L13 4L13 15L3 15L3 1ZM9 1L9 5L13 5M5 8L11 8M5 11L11 11",
        ["info"]="M15 8A7 7 0 1 1 1 8A7 7 0 1 1 15 8M8 7L8 12M8 4L8 4.5", ["more"]="M15 8A7 7 0 1 1 1 8A7 7 0 1 1 15 8M4 8L4.3 8M8 8L8.3 8M12 8L12.3 8",
        ["folder"]="M1 4L1 14L15 14L15 5L7 5L5 2L1 2Z", ["link"]="M6 5L8 3A3 3 0 0 1 13 8L11 10M5 6L3 8A3 3 0 0 0 8 13L10 11M5 11L11 5",
        ["code"]="M5 4L1 8L5 12M11 4L15 8L11 12M9 2L7 14", ["image"]="M1 2L15 2L15 14L1 14ZM1 12L6 7L9 10L12 7L15 10M12 4L12 5",
        ["laptop"]="M3 2L13 2L13 11L3 11ZM1 14L15 14L13 11L3 11Z", ["globe"]="M15 8A7 7 0 1 1 1 8A7 7 0 1 1 15 8M1 8L15 8M8 1C3 5 3 11 8 15C13 11 13 5 8 1",
        ["shield"]="M8 1L14 4L13 10L8 15L3 10L2 4ZM5 8L7 10L11 6", ["refresh"]="M13 5A6 6 0 1 0 14 10M10 5L14 5L14 1",
        ["keyboard"]="M1 3L15 3L15 13L1 13ZM3 6L4 6M6 6L7 6M9 6L10 6M12 6L13 6M3 9L4 9M6 9L7 9M9 9L10 9M12 9L13 9M4 11L12 11",
        ["edit"]="M3 11L2 14L5 13L14 4L12 2ZM10 4L12 6", ["close"]="M3 3L13 13M13 3L3 13", ["arrow"]="M3 13L13 3M5 3L13 3L13 11",
        ["check"]="M2 8L6 12L14 3", ["chevron"]="M5 2L11 8L5 14", ["down"]="M3 5L8 10L13 5", ["disk"]="M2 2L14 2L14 14L2 14ZM2 10L14 10M11 12L12 12",
        ["download"]="M8 1L8 10M4 6L8 10L12 6M2 12L2 15L14 15L14 12", ["latency"]="M1 5L14 5L11 2M15 11L2 11L5 14"
    }.ToDictionary(x=>x.Key,x=>Geometry.Parse(x.Value));
    public VectorIcon() { Width=Height=16; VerticalAlignment=VerticalAlignment.Center; SetResourceReference(StrokeProperty,"SecondaryBrush"); IsHitTestVisible=false; }
    protected override void OnRender(DrawingContext dc)
    {
        base.OnRender(dc); dc.PushTransform(new ScaleTransform(ActualWidth/16,ActualHeight/16));
        var pen=new Pen(Stroke,1.3) { StartLineCap=PenLineCap.Round, EndLineCap=PenLineCap.Round, LineJoin=PenLineJoin.Round };
        dc.DrawGeometry(Filled?Stroke:null,pen,Shapes.GetValueOrDefault(Kind,Shapes["file"])); dc.Pop();
    }
}

public class MacWindow : Window
{
    private HwndSource? _source;
    public ContextMenu? CaptionMenu { get; set; }
    public MacWindow()
    {
        SetResourceReference(StyleProperty,"MacWindow");
        WindowChrome.SetWindowChrome(this,new WindowChrome { CaptionHeight=32, ResizeBorderThickness=new(5), CornerRadius=new(12), GlassFrameThickness=new(0), UseAeroCaptionButtons=false });
        SourceInitialized += (_,_) => { var hwnd=new WindowInteropHelper(this).Handle;_source=HwndSource.FromHwnd(hwnd);_source?.AddHook(WindowMessage);if (OperatingSystem.IsWindowsVersionAtLeast(10,0,22000)) { var preference=2; DwmSetWindowAttribute(hwnd,33,ref preference,sizeof(int)); } };
        Closed += (_,_) => { _source?.RemoveHook(WindowMessage);_source=null; };
    }
    private IntPtr WindowMessage(IntPtr hwnd,int message,IntPtr wParam,IntPtr lParam,ref bool handled)
    {
        // WindowChrome consumes caption hit tests, so use the non-client message
        // to make the review window menu available without adding product chrome.
        if(message==0x00A5 && wParam.ToInt64()==2 && CaptionMenu is { } menu) { menu.PlacementTarget=this;menu.Placement=System.Windows.Controls.Primitives.PlacementMode.MousePoint;menu.IsOpen=true;handled=true; }
        return IntPtr.Zero;
    }
    protected Grid MakeTitleBar(string title)
    {
        var bar=new Grid { Height=32, Background=Brushes.Transparent };
        var heading=Ui.Text(title,13,"Secondary");heading.FontWeight=FontWeights.Bold;heading.Margin=new(84,0,0,0);bar.Children.Add(heading);
        var lights=new StackPanel { Orientation=Orientation.Horizontal, Margin=new(9,0,0,0), VerticalAlignment=VerticalAlignment.Center, HorizontalAlignment=HorizontalAlignment.Left };
        var colors=new[]{("#FF7067","#E44C45","关闭窗口"),("#FFCD41","#E7A322","最小化窗口"),("#78D54E","#4CAE24","最大化窗口")};
        for(var i=0;i<colors.Length;i++) {var index=i;var c=colors[i];var b=Ui.Button(c.Item3,()=>{if(index==0)Close();else if(index==1)WindowState=WindowState.Minimized;else WindowState=WindowState==WindowState.Maximized?WindowState.Normal:WindowState.Maximized;},"TrafficLight");b.Background=(Brush)new BrushConverter().ConvertFromString(c.Item1)!;b.BorderBrush=(Brush)new BrushConverter().ConvertFromString(c.Item2)!;b.Margin=new(i==0?0:10,0,0,0);WindowChrome.SetIsHitTestVisibleInChrome(b,true);lights.Children.Add(b);}
        bar.Children.Add(lights);return bar;
    }
    [DllImport("dwmapi.dll")] private static extern int DwmSetWindowAttribute(IntPtr window,int attribute,ref int value,int size);
}
