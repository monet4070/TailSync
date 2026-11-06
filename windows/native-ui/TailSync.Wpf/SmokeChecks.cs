using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Threading;
namespace TailSync.Wpf;

// Runs the actual WPF controls and templates on Windows, separate from the
// platform-neutral Core checks. Test results contain synthetic data only.
internal static class SmokeChecks
{
    internal static async Task Run(App app,string[] args)
    {
        var checks=new List<string>();string? failure=null;
        void Check(bool value,string label){if(!value)throw new InvalidOperationException(label);checks.Add(label);}
        try
        {
            Check(Math.Abs(((SolidColorBrush)app.FindResource("InputBrush")).Opacity-.045)<.000001,"input alpha retains source precision for nested fills");
            var history=new MainWindow("history");history.Show();await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
            Check(history.Width==400&&history.Height==600,"history uses Mac reference geometry");
            var list=(ListBox)history.FindName("HistoryList");Check(list.Items.Count==8,"fixture entries bind into actual ListBox");
            Check(VirtualizingPanel.GetIsVirtualizing(list)&&VirtualizingPanel.GetVirtualizationMode(list)==VirtualizationMode.Recycling,"history virtualization and recycling stay enabled");
            var search=(TextBox)history.FindName("SearchBox");search.Text="FLUENT";await Dispatcher.Yield();Check(list.Items.Count==1&&history.ViewModel.SelectedEntry?.Id==2,"WPF TextChanged updates the shared query");
            search.Text="does not exist";await Dispatcher.Yield();Check(list.Items.Count==0&&history.ViewModel.SelectedEntry==null,"empty search clears restore target");search.Clear();
            var favorites=new MainWindow("favorites");favorites.Show();await Dispatcher.Yield();Check(favorites.ViewModel.Entries.Count==3&&((Button)favorites.FindName("PinButton")).Visibility==Visibility.Collapsed,"favorites keep their own collection and toolbar");
            var settings=new MainWindow("settings");settings.Show();await Dispatcher.Yield();var content=(StackPanel)settings.FindName("SettingsContent");Check(content.Children.OfType<StackPanel>().Count(x=>x.Tag!=null)==6,"all six settings groups construct and template");
            var connections=new MainWindow("connections");connections.Show();await Dispatcher.Yield();Check(((StackPanel)connections.FindName("ConnectionsContent")).Children.Count==1,"connections retain one source grouping");
            ThemeService.Apply(appearance:"dark");await Dispatcher.Yield();Check(((SolidColorBrush)app.FindResource("CanvasBrush")).Color==Color.FromRgb(25,25,24),"dark palette reaches live resources");
            ThemeService.Apply(appearance:"light",id:"flux");Check(((SolidColorBrush)app.FindResource("AccentBrush")).Color==Color.FromRgb(20,121,112),"selected theme uses source palette");
            var preview=new PreviewWindow(history.ViewModel.Entries[0],()=>{});preview.Show();await Dispatcher.Yield();
            Check(preview.Width==640,"independent preview constructs");
            preview.UpdateLayout();
            var combo = QualityChecks.Descendants(preview).OfType<ComboBox>().First();
            var icon = QualityChecks.Descendants(combo).OfType<VectorIcon>().First(i => i.Kind == "down");
            var iconPos = icon.TransformToAncestor(combo).Transform(new Point(0, 0));
            Check(iconPos.X >= 80, $"dropdown arrow is right-aligned in toolbar mode combobox (x={iconPos.X})");
            combo.SelectedIndex = 2;
            await Dispatcher.Yield();
            preview.UpdateLayout();
            var viewer = QualityChecks.Descendants(preview).OfType<FlowDocumentScrollViewer>().First();
            Check(TextOptions.GetTextFormattingMode(viewer) == TextFormattingMode.Display, "viewer uses Display text formatting");
            Check(TextOptions.GetTextRenderingMode(viewer) == TextRenderingMode.ClearType, "viewer uses ClearType rendering");
            Check(viewer.Zoom == 100, "viewer maintains 100% zoom scale");
            var zoomLabel = QualityChecks.Descendants(preview).OfType<TextBlock>().First(t => t.Text.EndsWith("px"));
            var initialSize = viewer.Document.FontSize;
            Check(zoomLabel.Text == $"{initialSize:0}px", "toolbar px label matches markdown document font size");
            var zoomInBtn = QualityChecks.Descendants(preview).OfType<Button>().First(b => ButtonDesign.GetIcon(b) == "add");
            zoomInBtn.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Primitives.ButtonBase.ClickEvent));
            await Dispatcher.Yield();
            preview.UpdateLayout();
            Check(viewer.Document.FontSize == initialSize + 1, "markdown document font size increments");
            Check(zoomLabel.Text == $"{initialSize + 1:0}px", "toolbar px label increments with font size");
            preview.Close();
            foreach(var window in new Window[]{connections,settings,favorites,history})window.Close();
        }
        catch(Exception error){failure=error.ToString();}
        var report=args.FirstOrDefault(x=>x.StartsWith("--report="))?[9..]??"wpf-smoke-results.json";
        await File.WriteAllTextAsync(Path.GetFullPath(report),JsonSerializer.Serialize(new{passed=failure==null,checks,failure},new JsonSerializerOptions{WriteIndented=true}));
        app.Shutdown(failure==null?0:1);
    }
}