using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using TailSync.Native.Core;
namespace TailSync.Wpf;

public sealed class PreviewWindow : MacWindow
{
    private readonly TextBox _search;
    private readonly TextBox _text;
    public PreviewWindow(HistoryEntry entry,Action restore)
    {
        Title="Preview";Width=640;Height=560;MinWidth=420;MinHeight=320;
        var root=new Grid();foreach(var h in new[]{new GridLength(32),new GridLength(64),new GridLength(entry.Preview=="image"?44:56),new GridLength(1,GridUnitType.Star)})root.RowDefinitions.Add(new(){Height=h});
        root.Children.Add(MakeTitleBar(Title));
        var heading=new Grid { Margin=new(16,0,16,0) };heading.ColumnDefinitions.Add(new(){Width=GridLength.Auto});heading.ColumnDefinitions.Add(new());heading.ColumnDefinitions.Add(new(){Width=GridLength.Auto});
        var badge=new Border { Width=32,Height=32,CornerRadius=new(9),Child=new VectorIcon{Kind="file"},VerticalAlignment=VerticalAlignment.Center };badge.SetResourceReference(Border.BackgroundProperty,"AccentSoftBrush");heading.Children.Add(badge);
        var name=Ui.Text(entry.Title,15);name.SetResourceReference(TextBlock.FontFamilyProperty,"DisplayFont");name.TextTrimming=TextTrimming.CharacterEllipsis;name.TextWrapping=TextWrapping.NoWrap;
        var description=Ui.Stack(Orientation.Vertical,2,name,Ui.Text(entry.SizeLabel,10,"Tertiary"));description.VerticalAlignment=VerticalAlignment.Center;description.Margin=new(12,0,12,0);Grid.SetColumn(description,1);heading.Children.Add(description);
        var restoreButton=Ui.Button(MacDesign.Text("history.preview.restore"),restore);Grid.SetColumn(restoreButton,2);heading.Children.Add(restoreButton);Grid.SetRow(heading,1);root.Children.Add(heading);
        var tools=new Grid { Margin=new(16,0,16,0) };tools.ColumnDefinitions.Add(new(){Width=new GridLength(1,GridUnitType.Star)});tools.ColumnDefinitions.Add(new(){Width=GridLength.Auto});Grid.SetRow(tools,2);root.Children.Add(tools);
        _search=Ui.Name(new TextBox { Tag=MacDesign.Text("history.preview.search"),Width=130,Height=28,FontSize=11,HorizontalAlignment=HorizontalAlignment.Left },"搜索预览");tools.Children.Add(_search);
        _text=new TextBox { Text=entry.Body,IsReadOnly=true,IsReadOnlyCaretVisible=true,IsInactiveSelectionHighlightEnabled=true,TextWrapping=TextWrapping.Wrap,AcceptsReturn=true,VerticalScrollBarVisibility=ScrollBarVisibility.Auto,HorizontalScrollBarVisibility=ScrollBarVisibility.Auto,Background=Brushes.Transparent,BorderThickness=new(0),Padding=new(20),FontSize=entry.Preview=="code"?12:13 };
        if(entry.Preview=="code")_text.FontFamily=new("Cascadia Code, Consolas");else _text.SetResourceReference(Control.FontFamilyProperty,"DisplayFont");
        _search.TextChanged+=(_,_)=>{if(_search.Text.Length==0){_text.Select(0,0);return;}var at=_text.Text.IndexOf(_search.Text,StringComparison.OrdinalIgnoreCase);if(at>=0){_text.Select(at,_search.Text.Length);_text.ScrollToLine(_text.GetLineIndexFromCharacterIndex(at));}};
        var fontTools=Ui.Stack(Orientation.Horizontal,8,Ui.Button("−",()=>_text.FontSize=Math.Max(10,_text.FontSize-1)),Ui.Button("+",()=>_text.FontSize=Math.Min(28,_text.FontSize+1)),Ui.Button("↵",()=>_text.TextWrapping=_text.TextWrapping==TextWrapping.Wrap?TextWrapping.NoWrap:TextWrapping.Wrap));Grid.SetColumn(fontTools,1);tools.Children.Add(fontTools);
        if(entry.Preview=="image")
        {
            _search.Visibility=fontTools.Visibility=Visibility.Collapsed;
            var image=new Image { Source=Ui.Landscape(),Stretch=Stretch.Uniform,Margin=new(20) };var zoom=new ScaleTransform(1,1);image.LayoutTransform=zoom;
            var imageScroll=new ScrollViewer { Content=image,HorizontalScrollBarVisibility=ScrollBarVisibility.Auto,VerticalScrollBarVisibility=ScrollBarVisibility.Auto };Grid.SetRow(imageScroll,3);root.Children.Add(imageScroll);
            var imageTools=Ui.Stack(Orientation.Horizontal,8,Ui.Button("适应窗口",()=>zoom.ScaleX=zoom.ScaleY=1),Ui.Button("−",()=>zoom.ScaleX=zoom.ScaleY=Math.Max(.25,zoom.ScaleX-.25)),Ui.Button("+",()=>zoom.ScaleX=zoom.ScaleY=Math.Min(4,zoom.ScaleX+.25)));Grid.SetColumn(imageTools,1);tools.Children.Add(imageTools);
        }
        else {Grid.SetRow(_text,3);root.Children.Add(_text);}
        foreach(var row in new[]{1,2}){var line=new Border { Height=1,VerticalAlignment=VerticalAlignment.Bottom };line.SetResourceReference(Border.BackgroundProperty,"DividerBrush");Grid.SetRow(line,row);root.Children.Add(line);}
        Content=root;PreviewKeyDown+=(_,e)=>{if(e.Key==Key.Escape){e.Handled=true;Close();}else if(e.Key==Key.F&&Keyboard.Modifiers==ModifierKeys.Control){e.Handled=true;_search.Focus();_search.SelectAll();}};
    }
}
