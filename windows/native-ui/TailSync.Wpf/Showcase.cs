using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace TailSync.Wpf;

// Capture the live WPF visual tree, leaving the demo windows open for review.
internal static class Showcase
{
    internal static async Task Run(App app,string directory)
    {
        Directory.CreateDirectory(directory);
        try
        {
            ThemeService.Apply(appearance:"light",id:"canvas");
            var windows=new[]{MainWindow.Open("history"),MainWindow.Open("settings"),MainWindow.Open("connections")};
            app.MainWindow=windows[0];
            var names=new[]{"history","settings","connections"};
            double left=24;
            foreach(var window in windows)
            {
                window.Left=left;window.Top=64;left+=window.Width+24;
            }
            await Task.Delay(700);
            await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
            var captures=new List<RenderTargetBitmap>();
            for(var i=0;i<windows.Length;i++)
            {
                var window=windows[i];window.UpdateLayout();
                var image=new RenderTargetBitmap((int)Math.Ceiling(window.ActualWidth),(int)Math.Ceiling(window.ActualHeight),96,96,PixelFormats.Pbgra32);
                image.Render(window);captures.Add(image);
                Save(image,Path.Combine(directory,names[i]+".png"));
            }
            var width=captures.Sum(x=>x.PixelWidth)+24*(captures.Count+1);
            var height=captures.Max(x=>x.PixelHeight)+48;
            var visual=new DrawingVisual();
            using(var context=visual.RenderOpen())
            {
                context.DrawRectangle(new SolidColorBrush(Color.FromRgb(232,232,230)),null,new Rect(0,0,width,height));
                double x=24;
                foreach(var capture in captures){context.DrawImage(capture,new Rect(x,24,capture.PixelWidth,capture.PixelHeight));x+=capture.PixelWidth+24;}
            }
            var montage=new RenderTargetBitmap(width,height,96,96,PixelFormats.Pbgra32);montage.Render(visual);
            Save(montage,Path.Combine(directory,"wpf-overview.png"));
            var scrollbars=windows.SelectMany(window=>Descendants(window).OfType<ScrollBar>().Where(bar=>bar.IsVisible).Select(bar=>new{window=window.Title,width=bar.ActualWidth,height=bar.ActualHeight,orientation=bar.Orientation.ToString(),gripWidth=(bar.Template.FindName("PART_Track",bar) is Track track?track.Thumb?.Template.FindName("Grip",track.Thumb) as FrameworkElement:null)?.ActualWidth})).ToArray();
            File.WriteAllText(Path.Combine(directory,"capture.json"),JsonSerializer.Serialize(new{passed=true,kind="Live WPF visual tree at 96 DPI; excludes OS window shadow",windows=names,scrollbars}));
            windows[0].Activate();
        }
        catch(Exception error){File.WriteAllText(Path.Combine(directory,"error.txt"),error.ToString());app.Shutdown(1);}
    }
    private static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        for(var i=0;i<VisualTreeHelper.GetChildrenCount(root);i++){var child=VisualTreeHelper.GetChild(root,i);yield return child;foreach(var descendant in Descendants(child))yield return descendant;}
    }
    private static void Save(BitmapSource image,string path)
    {
        var encoder=new PngBitmapEncoder();encoder.Frames.Add(BitmapFrame.Create(image));using var output=File.Create(path);encoder.Save(output);
    }
}
