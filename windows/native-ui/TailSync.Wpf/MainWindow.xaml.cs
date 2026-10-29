using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;
using TailSync.Native.Core;
namespace TailSync.Wpf;

public partial class MainWindow : MacWindow
{
    private static readonly Dictionary<string,MainWindow> OpenWindows=[];
    private readonly DispatcherTimer _toast=new() { Interval=TimeSpan.FromSeconds(4) };
    private bool _ready;
    private PreviewWindow? _preview;
    public string Page { get; }
    public ShellViewModel ViewModel { get; }=new(AppSession.Client);
    public MainWindow() : this("history") { }
    public MainWindow(string page)
    {
        Page=page is "favorites" or "settings" or "connections" ? page : "history";
        InitializeComponent();DataContext=ViewModel;
        var dimensions=MacDesign.Root.GetProperty("windows").GetProperty(Page);
        Width=dimensions.GetProperty("width").GetInt32();Height=dimensions.GetProperty("height").GetInt32();
        MinWidth=dimensions.GetProperty("minWidth").GetInt32();MinHeight=dimensions.GetProperty("minHeight").GetInt32();
        Title=dimensions.GetProperty("title").GetString()!;TitleHost.Content=MakeTitleBar(Title);
        HistoryPage.Visibility=Page is "history" or "favorites"?Visibility.Visible:Visibility.Collapsed;
        SettingsPage.Visibility=Page=="settings"?Visibility.Visible:Visibility.Collapsed;
        ConnectionsPage.Visibility=Page=="connections"?Visibility.Visible:Visibility.Collapsed;
        ClearButton.Visibility=PinButton.Visibility=FavoritesButton.Visibility=Page=="favorites"?Visibility.Collapsed:Visibility.Visible;
        var menu=new ContextMenu();foreach(var (id,label) in new[]{("history","历史记录"),("favorites","收藏"),("settings","设置"),("connections","连接与设备")}) { var target=id;var item=new MenuItem { Header=label };item.Click+=(_,_)=>Open(target);menu.Items.Add(item); } TitleHost.ContextMenu=menu;CaptionMenu=menu;
        if(Page=="settings")BuildSettings();if(Page=="connections")BuildConnections();
        _toast.Tick+=(_,_)=>{_toast.Stop();Feedback.Visibility=Visibility.Collapsed;};
        Loaded+=async(_,_)=>{_ready=true;UpdateSync();await ViewModel.NavigateAsync(Page);UpdateEmpty();if(Page is "history" or "favorites")SearchBox.Focus();};
        PreviewKeyDown+=Window_KeyDown;
        AppSession.HistoryChanged+=HistoryChanged;AppSession.SyncChanged+=SyncChanged;
        Closed+=(_,_)=>{_ready=false;_toast.Stop();_preview?.Close();AppSession.HistoryChanged-=HistoryChanged;AppSession.SyncChanged-=SyncChanged;if(OpenWindows.GetValueOrDefault(Page)==this)OpenWindows.Remove(Page);};
        OpenWindows[Page]=this;
    }
    public static MainWindow Open(string page)
    {
        if(OpenWindows.TryGetValue(page,out var existing)){if(existing.WindowState==WindowState.Minimized)existing.WindowState=WindowState.Normal;existing.Activate();return existing;}
        var window=new MainWindow(page);window.Show();return window;
    }
    private async void HistoryChanged(object? sender,EventArgs e) { if(_ready && Page is "history" or "favorites")await Refresh(); }
    private void SyncChanged(object? sender,EventArgs e)=>UpdateSync();
    private void UpdateSync()
    {
        SyncDot.SetResourceReference(ShapeFill,AppSession.Sync?"PositiveBrush":"TertiaryBrush");
        if(_syncSwitch!=null)_syncSwitch.IsChecked=AppSession.Sync;
        SyncButton.ToolTip=AppSession.Sync?"同步已开启（演示）":"同步已暂停（演示）";
    }
    private static readonly DependencyProperty ShapeFill=System.Windows.Shapes.Shape.FillProperty;
    private async Task Refresh()
    {
        try {await ViewModel.RefreshAsync();UpdateEmpty();}catch(Exception error){Notify("读取示例内容失败："+error.Message);}
    }
    private void UpdateEmpty()
    {
        EmptyState.Visibility=ViewModel.Entries.Count==0?Visibility.Visible:Visibility.Collapsed;
        EmptyTitle.Text=T(Page=="favorites"?"favorites.emptyTitle":"history.empty");
        EmptyDescription.Text=Page=="favorites"?T("favorites.emptyDescription"):"";
    }
    private void Notify(string message){FeedbackText.Text=message;Feedback.Visibility=Visibility.Visible;_toast.Stop();_toast.Start();}
    private async void Search_Changed(object sender,TextChangedEventArgs e){if(!_ready)return;ViewModel.Keyword=SearchBox.Text;await Refresh();}
    private async void Filter_Changed(object sender,SelectionChangedEventArgs e)
    {
        if(!_ready)return;
        ViewModel.Category=(CategoryFilter.SelectedItem as ComboBoxItem)?.Tag?.ToString()??"all";
        ViewModel.Date=(DateFilter.SelectedItem as ComboBoxItem)?.Tag?.ToString()??"all";
        CustomDates.Visibility=ViewModel.Date=="custom"?Visibility.Visible:Visibility.Collapsed;
        SetRange();await Refresh();
    }
    private void SetRange(){ViewModel.Start=StartDate.SelectedDate is { } start?DateOnly.FromDateTime(start):null;ViewModel.End=EndDate.SelectedDate is { } end?DateOnly.FromDateTime(end):null;}
    private async void Range_Changed(object sender,SelectionChangedEventArgs e){if(!_ready)return;SetRange();await Refresh();}
    private void Sync_Click(object sender,RoutedEventArgs e)=>AppSession.SetSync(!AppSession.Sync);
    private void Pin_Click(object sender,RoutedEventArgs e){Topmost=!Topmost;PinButton.ToolTip=Topmost?"取消历史窗口置顶":"历史窗口置顶";((VectorIcon)PinButton.Content).SetResourceReference(VectorIcon.StrokeProperty,Topmost?"AccentBrush":"SecondaryBrush");}
    private void Favorites_Click(object sender,RoutedEventArgs e)=>Open("favorites");
    private bool Select(object sender)
    {
        if(sender is FrameworkElement { DataContext:HistoryEntry entry }){ViewModel.SelectedEntry=entry;return true;}return false;
    }
    private async Task Restore(){var result=await ViewModel.RestoreSelectedAsync();Notify(result.Message);}
    private async void RestoreRow_Click(object sender,RoutedEventArgs e){if(Select(sender))await Restore();}
    private void PreviewRow_Click(object sender,RoutedEventArgs e){if(Select(sender))ShowPreview();}
    private async void FavoriteRow_Click(object sender,RoutedEventArgs e){if(!Select(sender))return;var result=await ViewModel.ToggleFavoriteAsync();Notify(result.Message);AppSession.RefreshHistory();}
    private async void DeleteRow_Click(object sender,RoutedEventArgs e)
    {
        if(!Select(sender)||ViewModel.SelectedEntry is not { } entry)return;
        if(entry.Pinned&&Page!="favorites"){Notify(T("history.favoriteProtected"));return;}
        if(!DemoDialog.Confirm(this,T("history.delete"),entry.Title,"删除"))return;
        var result=await AppSession.Client.DeleteAsync(entry.Id,Page=="favorites");Notify(result.Message);AppSession.RefreshHistory();
    }
    private async void Clear_Click(object sender,RoutedEventArgs e)
    {
        if(Page!="history"||!DemoDialog.Confirm(this,"清空所有记录",T("history.confirmClear"),"清空"))return;
        foreach(var entry in await AppSession.Client.GetHistoryAsync(new()))if(!entry.Pinned)await AppSession.Client.DeleteAsync(entry.Id,false);
        AppSession.RefreshHistory();Notify("已清空未收藏的示例内容");
    }
    private void Info_Click(object sender,RoutedEventArgs e)
    {
        if(!Select(sender)||ViewModel.SelectedEntry is not { } entry||sender is not FrameworkElement anchor)return;
        var body=new StackPanel();body.Children.Add(Ui.Text(T("history.composite.details"),13));
        foreach(var (key,value) in new[]{("formats","text / html"),("representations","2"),("images",entry.Preview=="image"?"1":"0"),("nativeItems","1"),("nativeOnly",entry.LocalOnly?"是":"否"),("completeness","完整"),("restorable",entry.LocalOnly?"无":"text / html"),("withheld",entry.LocalOnly?"原生内容":"无")})body.Children.Add(SettingRow(T("history.composite.diag."+key),"",Ui.Text(value,11),3));
        var popup=new Popup { PlacementTarget=anchor,Placement=PlacementMode.Top,StaysOpen=false,AllowsTransparency=true,Child=Ui.Surface(body,new(14)),IsOpen=true };
        popup.Closed+=(_,_)=>anchor.Focus();
    }
    private void Format_Click(object sender,RoutedEventArgs e)
    {
        if(!Select(sender)||ViewModel.SelectedEntry is not { } entry||sender is not Button button)return;
        var menu=new ContextMenu();foreach(var key in new[]{"history.composite.copy.text","history.composite.copy.png"}){var item=new MenuItem { Header=T(key) };item.Click+=async(_,_)=>Notify((await AppSession.Client.RestoreAsync(entry.Id)).Message);menu.Items.Add(item);}menu.PlacementTarget=button;menu.IsOpen=true;
    }
    private async void History_DoubleClick(object sender,MouseButtonEventArgs e)
    {
        if(e.OriginalSource is DependencyObject source && FindParent<ListBoxItem>(source)!=null && FindParent<Button>(source)==null)await Restore();
    }
    private static TElement? FindParent<TElement>(DependencyObject source) where TElement:DependencyObject
    {
        for(DependencyObject? current=source;current!=null;current=current is Visual?VisualTreeHelper.GetParent(current):LogicalTreeHelper.GetParent(current))if(current is TElement item)return item;
        return null;
    }
    private async void History_KeyDown(object sender,KeyEventArgs e)
    {
        if(e.Key is Key.ImeProcessed or Key.DeadCharProcessed)return;
        if(e.OriginalSource is DependencyObject source && FindParent<Button>(source)!=null)return;
        if(e.Key==Key.Enter){e.Handled=true;await Restore();}else if(e.Key==Key.Space){e.Handled=true;ShowPreview();}else if(e.Key==Key.Escape){e.Handled=true;SearchBox.Focus();}
    }
    private void Window_KeyDown(object sender,KeyEventArgs e)
    {
        if(e.Key is Key.ImeProcessed or Key.DeadCharProcessed)return;
        if(Keyboard.Modifiers==ModifierKeys.Control&&e.Key==Key.OemComma){e.Handled=true;Open("settings");return;}
        if(Page is not ("history" or "favorites"))return;
        if(Keyboard.Modifiers==ModifierKeys.Control&&e.Key==Key.F){e.Handled=true;SearchBox.Focus();SearchBox.SelectAll();}
        else if(e.Key==Key.Down&&SearchBox.IsKeyboardFocusWithin&&HistoryList.Items.Count>0){e.Handled=true;HistoryList.SelectedIndex=0;HistoryList.ScrollIntoView(HistoryList.SelectedItem);HistoryList.UpdateLayout();(HistoryList.ItemContainerGenerator.ContainerFromIndex(0) as ListBoxItem)?.Focus();}
    }
    private void ShowPreview()
    {
        if(ViewModel.SelectedEntry is not { } entry)return;
        _preview?.Close();var preview=new PreviewWindow(entry,async()=>Notify((await AppSession.Client.RestoreAsync(entry.Id)).Message));_preview=preview;preview.Closed+=(_,_)=>{if(_preview==preview)_preview=null;HistoryList.Focus();};preview.Show();
    }
}
