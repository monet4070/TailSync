using System.Diagnostics;
using System.Text.Json;
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
    private readonly DispatcherTimer _searchDebounce = new() { Interval = TimeSpan.FromMilliseconds(250) };
    private readonly DispatcherTimer _longPress = new();
    private bool _pressCharging;
    private int _favoriteFeedbackCount;
    private bool _historyRefreshPending;
    private readonly CancellationTokenSource _lifetime = new();
    private readonly HistoryThumbnails _thumbnails = new();
    private readonly HashSet<string> _mutations = [];
    private HistoryEntry? _pressedEntry;
    private Point _pressPosition;
    private bool _suppressClick;
    private bool _composing;
    private long _compositionEnded;
    private TourOverlay? _tourOverlay;
    internal bool CanChangeDataMode => _mutations.Count == 0 && _favoriteFeedbackCount == 0;
    private long _historyVersion = -1;
    private string? _transferBatch;

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
        if (Page == "settings") { Width = 620; Height = Math.Min(900, SystemParameters.WorkArea.Height - 48); }
        if (Page == "connections") { Width = 620; Height = Math.Min(700, SystemParameters.WorkArea.Height - 48); }
        Title=Page switch { "favorites"=>"收藏", "settings"=>"设置", "connections"=>"连接与设备", _=>"历史记录" };TitleHost.Content=MakeTitleBar(Title);
        HistoryPage.Visibility=Page is "history" or "favorites"?Visibility.Visible:Visibility.Collapsed;
        SettingsPage.Visibility=Page=="settings"?Visibility.Visible:Visibility.Collapsed;
        ConnectionsPage.Visibility=Page=="connections"?Visibility.Visible:Visibility.Collapsed;
        ClearButton.Visibility=PinButton.Visibility=FavoritesButton.Visibility=Page=="favorites"?Visibility.Collapsed:Visibility.Visible;
        var menu=new ContextMenu();foreach(var (id,label) in new[]{("history","历史记录"),("favorites","收藏"),("settings","设置"),("connections","连接与设备")}) { var target=id;var item=new MenuItem { Header=label };ButtonDesign.SetIcon(item,id switch { "favorites" => "star", "connections" => "devices", _ => id });item.Click+=(_,_)=>Open(target);menu.Items.Add(item); } TitleHost.ContextMenu=menu;CaptionMenu=menu;
        if(Page=="settings")BuildSettings();if(Page=="connections")BuildConnections();
        _toast.Tick+=(_,_)=>{_toast.Stop();Feedback.Visibility=Visibility.Collapsed;};
        ApplyLanguage();Localizer.Changed+=LanguageChanged;
        ViewModel.GroupDates=!AppSession.IsDemo;
        StartDate.SelectedDate=DateTime.Today.AddDays(-6);EndDate.SelectedDate=DateTime.Today;
        _searchDebounce.Tick+=async(_,_)=>{_searchDebounce.Stop();ViewModel.Keyword=SearchBox.Text;ViewModel.ResetPage();await Refresh();};
        _longPress.Tick += async (_, _) =>
        {
            _longPress.Stop();
            if (_pressedEntry is not { } entry) return;
            if (!_pressCharging)
            {
                _pressCharging = true; entry.FavoriteProgress = 1;
                _longPress.Interval = TimeSpan.FromMilliseconds(420); _longPress.Start(); return;
            }
            _pressedEntry = null; _pressCharging = false; _suppressClick = true;
            ViewModel.SelectedEntry = entry; await ToggleFavorite(entry);
        };
        HistoryList.MouseLeave+=(_,_)=>CancelPress();HistoryList.PreviewMouseWheel+=(_,_)=>CancelPress();
        HistoryList.LostMouseCapture += (_, _) => CancelPress();
        TextCompositionManager.AddPreviewTextInputStartHandler(SearchBox,(_,_)=>{_composing=true;_searchDebounce.Stop();});
        TextCompositionManager.AddPreviewTextInputHandler(SearchBox,(_,_)=>{_composing=false;_compositionEnded=Environment.TickCount64;if(!AppSession.IsDemo){_searchDebounce.Stop();_searchDebounce.Start();}});
        Loaded+=async(_,_)=>{_ready=true;UpdateSync();try{await ViewModel.NavigateAsync(Page);UpdateEmpty();}catch(Exception error){ShowLoadError(error);}if(Page is "history" or "favorites"){if(Page=="history")FocusEntry(false);else SearchBox.Focus();if(!AppSession.IsDemo)await LoadCapabilities();}};
        GuidedTour.Changed+=TourChanged;
        ViewModel.PropertyChanged+=(_,args)=>
        {
            UpdateKeyboardHint();
            if(args.PropertyName==nameof(ShellViewModel.IsLoading)&&!ViewModel.IsLoading&&_historyRefreshPending&&_ready&&_pressedEntry==null&&_favoriteFeedbackCount==0)
            {_historyRefreshPending=false;Dispatcher.BeginInvoke(async()=>{if(_ready)await Refresh();});}
        };
        HistoryList.IsKeyboardFocusWithinChanged+=(_,_)=>UpdateKeyboardHint();
        SearchBox.IsKeyboardFocusWithinChanged+=(_,_)=>UpdateKeyboardHint();
        NativeBackend.RuntimeChanged+=RuntimeChanged;RestoreActivity.Changed+=RestoreChanged;RestoreChanged(null,EventArgs.Empty);
        StateChanged+=(_,_)=>{if(_preview!=null&&WindowState==WindowState.Minimized)_preview.WindowState=WindowState.Minimized;};
        Deactivated+=(_,_)=>CancelPress();

        PreviewKeyDown+=Window_KeyDown;
        AppSession.HistoryChanged+=HistoryChanged;AppSession.SyncChanged+=SyncChanged;
        Closed+=(_,_)=>{_ready=false;GuidedTour.Changed-=TourChanged;_tourOverlay?.Dispose();_tourOverlay=null;_toast.Stop();_searchDebounce.Stop();CancelPress();_lifetime.Cancel();_thumbnails.Clear();ViewModel.CancelPendingQuery();NativeBackend.RuntimeChanged-=RuntimeChanged;Localizer.Changed-=LanguageChanged;RestoreActivity.Changed-=RestoreChanged;_preview?.Close();AppSession.HistoryChanged-=HistoryChanged;AppSession.SyncChanged-=SyncChanged;if(OpenWindows.GetValueOrDefault(Page)==this)OpenWindows.Remove(Page);if(!AppSession.IsDemo&&!GuidedTour.Active&&Page is "history" or "favorites")ForegroundTracker.Restore();};
        OpenWindows[Page]=this;
        if(GuidedTour.Active)_tourOverlay=new(this,Root);
    }
    public static MainWindow Open(string page)
    {
        if(!AppSession.IsDemo&&!GuidedTour.Active&&page is "history" or "favorites")ForegroundTracker.Capture();
        if(OpenWindows.TryGetValue(page,out var existing)){WindowActivation.Raise(existing);existing.FocusCurrentEntry();return existing;}
        var window=new MainWindow(page);window.Show();WindowActivation.Raise(window);return window;
    }
    private async void HistoryChanged(object? sender,EventArgs e) { if(_ready && Page is "history" or "favorites")await Refresh(); }
    private void SyncChanged(object? sender,EventArgs e)=>UpdateSync();
    private void UpdateSync()
    {
        SyncDot.SetResourceReference(ShapeFill,AppSession.Sync?"PositiveBrush":"TertiaryBrush");
        if(_syncSwitch!=null)_syncSwitch.IsChecked=AppSession.Sync;
        SyncButton.ToolTip=AppSession.Sync?"同步已开启":"同步已暂停";
    }
    private static readonly DependencyProperty ShapeFill=System.Windows.Shapes.Shape.FillProperty;
    private async Task Refresh()
    {
        if (_favoriteFeedbackCount > 0 || _pressedEntry != null) { _historyRefreshPending = true; return; }
        _historyRefreshPending = false;
        CancelPress();
        try {await ViewModel.RefreshAsync();RetryHistoryButton.Visibility=Visibility.Collapsed;UpdateEmpty();}catch(OperationCanceledException){}catch(Exception error){ShowLoadError(error);}
    }
    private void UpdateEmpty()
    {
        EmptyState.Visibility=ViewModel.Entries.Count==0?Visibility.Visible:Visibility.Collapsed;
        var filtered=!string.IsNullOrWhiteSpace(ViewModel.Keyword)||ViewModel.Category!="all"||ViewModel.Date!="all";
        EmptyTitle.Text=T(filtered?"history.noMatches":Page=="favorites"?"favorites.emptyTitle":"history.emptyTitle");
        EmptyDescription.Text=T(filtered?"history.noMatchesDescription":Page=="favorites"?"favorites.emptyDescription":"history.emptyDescription");
        if(ViewModel.Date=="custom"&&((!ViewModel.Start.HasValue&&!ViewModel.End.HasValue)||ViewModel.Start>ViewModel.End)){EmptyTitle.Text=T("history.date.invalid");EmptyDescription.Text="";}
    }
    private void Notify(string message){FeedbackText.Text=message;Feedback.Visibility=Visibility.Visible;_toast.Stop();_toast.Start();}
    private async void Search_Changed(object sender,TextChangedEventArgs e){if(!_ready||_composing)return;_searchDebounce.Stop();ViewModel.CancelPendingQuery();if(!AppSession.IsDemo){_searchDebounce.Start();return;}ViewModel.Keyword=SearchBox.Text;ViewModel.ResetPage();await Refresh();}
    private async void Filter_Changed(object sender,SelectionChangedEventArgs e)
    {
        if(!_ready)return;
        ViewModel.Category=(CategoryFilter.SelectedItem as ComboBoxItem)?.Tag?.ToString()??"all";
        ViewModel.Date=(DateFilter.SelectedItem as ComboBoxItem)?.Tag?.ToString()??"all";
        CustomDates.Visibility=ViewModel.Date=="custom"?Visibility.Visible:Visibility.Collapsed;
        _searchDebounce.Stop();ViewModel.Keyword=SearchBox.Text;ViewModel.ResetPage();SetRange();await Refresh();
    }
    private void SetRange(){ViewModel.Start=StartDate.SelectedDate is { } start?DateOnly.FromDateTime(start):null;ViewModel.End=EndDate.SelectedDate is { } end?DateOnly.FromDateTime(end):null;}
    private async void Range_Changed(object sender,SelectionChangedEventArgs e){if(!_ready)return;ViewModel.ResetPage();SetRange();await Refresh();}
    private async void Sync_Click(object sender,RoutedEventArgs e){try{await AppSession.SetSyncAsync(!AppSession.Sync);}catch(Exception error){Notify(error.Message);}}
    private void Pin_Click(object sender,RoutedEventArgs e){Topmost=!Topmost;PinButton.ToolTip=T(Topmost?"history.unpinWindow":"history.pinWindow");((VectorIcon)PinButton.Content).SetResourceReference(VectorIcon.StrokeProperty,Topmost?"AccentBrush":"SecondaryBrush");}
    private void Favorites_Click(object sender,RoutedEventArgs e)=>Open("favorites");
    private bool Select(object sender)
    {
        if(ViewModel.IsQueryCurrent&&sender is FrameworkElement { DataContext:HistoryEntry entry }){ViewModel.SelectedEntry=entry;return true;}return false;
    }
    private async Task Restore(bool closeOnSuccess=false)
    {
        if(!ViewModel.IsQueryCurrent||RestoreActivity.IsBusy||_mutations.Count>0)return;
        try{var result=await ViewModel.RestoreSelectedAsync();if(closeOnSuccess&&result.Success&&result.Complete&&!AppSession.IsDemo&&!GuidedTour.Active)Close();else Notify(result.Message);}
        catch(Exception error){Notify(error.Message);}
    }
    private async void RestoreRow_Click(object sender,RoutedEventArgs e){if(Select(sender))await Restore();}
    private void PreviewRow_Click(object sender,RoutedEventArgs e){if(Select(sender))ShowPreview();}
    private async void FavoriteRow_Click(object sender,RoutedEventArgs e){if(Select(sender))await ToggleFavorite();}
    private async void DeleteRow_Click(object sender,RoutedEventArgs e)
    {
        if(!Select(sender)||ViewModel.SelectedEntry is not { } entry)return;
        if(entry.Pinned&&Page!="favorites"){Notify(T("history.favoriteProtected"));return;}
        if(AppSession.IsDemo&&!DemoDialog.Confirm(this,T("history.deleteEntry"),entry.Title,T("history.deleteEntry")))return;
        var key=entry.BatchId??entry.Id.ToString();if(!_mutations.Add(key))return;try{var result=await AppSession.Client.DeleteAsync(entry.Id,Page=="favorites");Notify(result.Message);if(result.Success)AppSession.RefreshHistory();}catch(Exception error){Notify(error.Message);}finally{_mutations.Remove(key);}
    }
    private async void Clear_Click(object sender,RoutedEventArgs e)
    {
        if(!ViewModel.IsQueryCurrent||RestoreActivity.IsBusy||Page!="history"||!DemoDialog.Confirm(this,T("history.clearConfirmTitle"),T("history.clearConfirmDescription"),T("history.clearAll")))return;
        ClearButton.IsEnabled=false;try{if(AppSession.Client is IHistoryPageClient paged)Notify((await paged.ClearHistoryAsync()).Message);else{foreach(var entry in await AppSession.Client.GetHistoryAsync(new()))if(!entry.Pinned)await AppSession.Client.DeleteAsync(entry.Id,false);Notify("已清空未收藏的示例内容");}_preview?.Close();ViewModel.ResetPage();_thumbnails.Clear();AppSession.RefreshHistory();}catch(Exception error){Notify(error.Message);}finally{ClearButton.IsEnabled=true;}
    }
    private void Info_Click(object sender,RoutedEventArgs e)
    {
        if(!AppSession.IsDemo||!Select(sender)||ViewModel.SelectedEntry is not { } entry||sender is not FrameworkElement anchor)return;
        var body=new StackPanel();body.Children.Add(Ui.Text(T("history.composite.details"),13));
        foreach(var (key,value) in new[]{("formats","text / html"),("representations","2"),("images",entry.Preview=="image"?"1":"0"),("nativeItems","1"),("nativeOnly",entry.LocalOnly?"是":"否"),("completeness","完整"),("restorable",entry.LocalOnly?"无":"text / html"),("withheld",entry.LocalOnly?"原生内容":"无")})body.Children.Add(SettingRow(T("history.composite.diag."+key),"",Ui.Text(value,11),3));
        var popup=new Popup { PlacementTarget=anchor,Placement=PlacementMode.Top,StaysOpen=false,AllowsTransparency=true,Child=Ui.Surface(body,new(14)),IsOpen=true };
        popup.Closed+=(_,_)=>anchor.Focus();
    }
    private void Format_Click(object sender,RoutedEventArgs e)
    {
        if(!AppSession.IsDemo||!Select(sender)||ViewModel.SelectedEntry is not { } entry||sender is not Button button)return;
        var menu=new ContextMenu();foreach(var key in new[]{"history.composite.copy.text","history.composite.copy.png"}){var item=new MenuItem { Header=T(key) };item.Click+=async(_,_)=>Notify((await AppSession.Client.RestoreAsync(entry.Id)).Message);menu.Items.Add(item);}menu.PlacementTarget=button;menu.IsOpen=true;
    }
    private async void History_DoubleClick(object sender,MouseButtonEventArgs e)
    {
        CancelPress();if(_suppressClick){_suppressClick=false;return;}if(e.OriginalSource is DependencyObject source && FindParent<ListBoxItem>(source)!=null && FindParent<Button>(source)==null)await Restore();
    }
    private static TElement? FindParent<TElement>(DependencyObject source) where TElement:DependencyObject
    {
        for(DependencyObject? current=source;current!=null;current=current is Visual?VisualTreeHelper.GetParent(current):LogicalTreeHelper.GetParent(current))if(current is TElement item)return item;
        return null;
    }
    private bool KeyboardReady(KeyEventArgs e)
    {
        if(_composing||Environment.TickCount64-_compositionEnded<150||e.Key is Key.ImeProcessed or Key.DeadCharProcessed||Keyboard.Modifiers!=ModifierKeys.None)return false;
        if(Keyboard.FocusedElement is not DependencyObject source||FindParent<ListBoxItem>(source)==null)return false;
        if(FindParent<ButtonBase>(source)!=null||FindParent<TextBoxBase>(source)!=null||FindParent<ComboBox>(source)!=null)return false;
        return IsActive && !OwnedWindows.Cast<Window>().Any(w=>w.IsVisible&&w is not PreviewWindow);
    }
    private async void History_KeyDown(object sender,KeyEventArgs e)
    {
        if(!KeyboardReady(e))return;
        if(e.Key==Key.F&&!e.IsRepeat){e.Handled=true;SearchBox.Focus();SearchBox.SelectAll();return;}
        if(!ViewModel.IsQueryCurrent||RestoreActivity.IsBusy||_mutations.Count>0)return;
        var number=e.Key>=Key.D1&&e.Key<=Key.D6?(int)e.Key-(int)Key.D1+1:e.Key>=Key.NumPad1&&e.Key<=Key.NumPad6?(int)e.Key-(int)Key.NumPad1+1:0;
        if(number>0&&Page=="history")
        {
            e.Handled=true;if(e.IsRepeat)return;
            if(ViewModel.VisibleEntries.FirstOrDefault(x=>x.ShortcutNumber==number) is {} target){ViewModel.SelectedEntry=target;await Restore(true);}return;
        }
        if(e.Key is Key.Left or Key.Right)
        {
            e.Handled=true;if(e.IsRepeat)return;
            if(e.Key==Key.Left&&ViewModel.HasPrevious){if(await ChangePage(ViewModel.PageIndex-1))FocusEntry(false);}
            else if(e.Key==Key.Right&&ViewModel.HasNext){if(await ChangePage(ViewModel.PageIndex+1))FocusEntry(false);}
            return;
        }
        if(e.Key is Key.Up or Key.Down)
        {
            var index=HistoryList.SelectedIndex;
            if(e.Key==Key.Up&&index<=0){e.Handled=true;if(ViewModel.HasPrevious){if(await ChangePage(ViewModel.PageIndex-1))FocusEntry(true);}else SearchBox.Focus();}
            else if(e.Key==Key.Down&&index==HistoryList.Items.Count-1&&ViewModel.HasNext){e.Handled=true;if(await ChangePage(ViewModel.PageIndex+1))FocusEntry(false);}
            return;
        }
        if(e.IsRepeat)return;
        if(e.Key==Key.Enter){e.Handled=true;await Restore();}else if(e.Key==Key.Space){e.Handled=true;ShowPreview();}else if(e.Key==Key.Escape){e.Handled=true;SearchBox.Focus();}else if(e.Key==Key.Delete&&ViewModel.SelectedEntry is {} entry){e.Handled=true;DeleteRow_Click(new FrameworkElement{DataContext=entry},e);}
    }
    private void FocusCurrentEntry()
    {
        if(Page!="history"||!ViewModel.IsQueryCurrent||ViewModel.SelectedEntry==null)return;
        HistoryList.ScrollIntoView(ViewModel.SelectedEntry);HistoryList.UpdateLayout();
        (HistoryList.ItemContainerGenerator.ContainerFromItem(ViewModel.SelectedEntry) as ListBoxItem)?.Focus();
    }
    private void FocusEntry(bool last)
    {
        if(HistoryList.Items.Count==0)return;
        HistoryList.SelectedIndex=last?HistoryList.Items.Count-1:0;
        HistoryList.ScrollIntoView(HistoryList.SelectedItem);HistoryList.UpdateLayout();
        (HistoryList.ItemContainerGenerator.ContainerFromIndex(HistoryList.SelectedIndex) as ListBoxItem)?.Focus();
    }
    private void Window_KeyDown(object sender,KeyEventArgs e)
    {
        if(_composing||Environment.TickCount64-_compositionEnded<150||e.Key is Key.ImeProcessed or Key.DeadCharProcessed)return;
        if(Keyboard.Modifiers==ModifierKeys.Control&&e.Key==Key.OemComma){e.Handled=true;Open("settings");return;}
        if(Page is not ("history" or "favorites"))return;
        if(Keyboard.Modifiers==ModifierKeys.Control&&e.Key==Key.F){e.Handled=true;SearchBox.Focus();SearchBox.SelectAll();}
        else if(e.Key==Key.Down&&!e.IsRepeat&&Keyboard.Modifiers==ModifierKeys.None&&SearchBox.IsKeyboardFocusWithin&&ViewModel.IsQueryCurrent&&HistoryList.Items.Count>0){e.Handled=true;HistoryList.SelectedIndex=0;HistoryList.ScrollIntoView(HistoryList.SelectedItem);HistoryList.UpdateLayout();(HistoryList.ItemContainerGenerator.ContainerFromIndex(0) as ListBoxItem)?.Focus();}
    }
    private void ShowPreview()
    {
        if(RestoreActivity.IsBusy||!ViewModel.IsQueryCurrent||ViewModel.SelectedEntry is not { } entry)return;
        var batch=entry.BatchId is {} batchId&&!ViewModel.IsBatchExpanded(batchId)?batchId:null;
        if(_preview is {} existing){existing.SetEntry(entry,batch);if(existing.WindowState==WindowState.Minimized)existing.WindowState=WindowState.Normal;existing.Activate();return;}
        var preview=new PreviewWindow(entry,async()=>{try{Notify((await AppSession.Client.RestoreAsync(entry.Id)).Message);}catch(Exception error){Notify(error.Message);}},batch){Owner=this};_preview=preview;preview.Closed+=(_,_)=>{if(_preview==preview)_preview=null;if(IsVisible)HistoryList.Focus();};preview.Show();
    }
    private void RestoreChanged(object? sender,EventArgs e){HistoryList.IsEnabled=!RestoreActivity.IsBusy;Resources["HistoryRestoreAvailable"]=!RestoreActivity.IsBusy;UpdateKeyboardHint();}
    private void ShowLoadError(Exception error){RetryHistoryButton.Visibility=Visibility.Visible;Notify(error.Message);}
    private async void RetryHistory_Click(object sender,RoutedEventArgs e)=>await Refresh();
    private async void PreviousPage_Click(object sender,RoutedEventArgs e){if(!ViewModel.HasPrevious)return;await ChangePage(ViewModel.PageIndex-1);}
    private async void NextPage_Click(object sender,RoutedEventArgs e){if(!ViewModel.HasNext)return;await ChangePage(ViewModel.PageIndex+1);}
    private async Task<bool> ChangePage(int page){if(!ViewModel.IsQueryCurrent||RestoreActivity.IsBusy||_mutations.Count>0)return false;var previous=ViewModel.PageIndex;try{await ViewModel.GoToPageAsync(page);UpdateEmpty();if(HistoryList.Items.Count>0)HistoryList.ScrollIntoView(HistoryList.Items[0]);return ViewModel.IsQueryCurrent&&ViewModel.PageIndex!=previous;}catch(Exception error){ShowLoadError(error);return false;}}
    private void ToggleBatch_Click(object sender,RoutedEventArgs e){if(sender is FrameworkElement{DataContext:HistoryEntry{BatchId:{} batch}})ViewModel.ToggleBatch(batch);}
    private async void RestoreBatch_Click(object sender,RoutedEventArgs e)
    {
        if(!ViewModel.IsQueryCurrent||sender is not FrameworkElement{DataContext:HistoryEntry{BatchId:{} batch,BatchStatus:"complete"}}||AppSession.Client is not IHistoryPageClient client)return;
        try{Notify((await client.RestoreBatchAsync(batch)).Message);}catch(Exception error){Notify(error.Message);}
    }
    private async Task ToggleFavorite(HistoryEntry? target = null)
    {
        var entry = target ?? ViewModel.SelectedEntry; if (entry == null||!ViewModel.IsQueryCurrent||RestoreActivity.IsBusy) return;
        var key = entry.BatchId ?? entry.Id.ToString(); if (!_mutations.Add(key)) { entry.FavoriteProgress = 0; return; }
        ++_favoriteFeedbackCount;
        try
        {
            var next = !entry.Pinned;
            var result = await AppSession.Client.SetFavoriteAsync(entry.Id, next, _lifetime.Token);
            if (result.Success)
            {
                foreach (var related in ViewModel.Entries.Where(x => x.Id == entry.Id || entry.BatchId != null && x.BatchId == entry.BatchId))
                {
                    related.Pinned = next; related.FavoriteCompletion = next ? 1 : -1; related.FavoriteProgress = 0;
                }
                await Task.Delay(550, _lifetime.Token);
            }
            else Notify(result.Message);
        }
        catch (OperationCanceledException) when (_lifetime.IsCancellationRequested) { }
        catch (Exception error) { Notify(error.Message); }
        finally
        {
            if (!ReferenceEquals(_pressedEntry, entry)) entry.FavoriteProgress = 0;
            entry.FavoriteCompletion = 0;
            _mutations.Remove(key); --_favoriteFeedbackCount;
            if (_ready) AppSession.RefreshHistory();
        }
    }
    private void History_PressStart(object sender,MouseButtonEventArgs e)
    {
        CancelPress(); _suppressClick = false;
        if (e.ClickCount != 1 || e.OriginalSource is not DependencyObject source || FindParent<Button>(source) != null || FindParent<ListBoxItem>(source)?.DataContext is not HistoryEntry entry) return;
        // Date and batch headers belong to the container but are not press targets.
        if (FindParent<Border>(source)?.Name != "EntryChrome" && !Ancestors(source).OfType<Border>().Any(x => x.Name == "EntryChrome")) return;
        _pressPosition = e.GetPosition(HistoryList); BeginFavoritePress(entry);
    }
    private static IEnumerable<DependencyObject> Ancestors(DependencyObject source)
    {
        for (DependencyObject? node = source; node != null; node = node is Visual ? VisualTreeHelper.GetParent(node) : LogicalTreeHelper.GetParent(node)) yield return node;
    }
    internal void BeginFavoritePress(HistoryEntry entry)
    {
        CancelPress(); _suppressClick = false;
        if (RestoreActivity.IsBusy) return;
        _pressedEntry = entry; entry.FavoriteCompletion = 0; _pressCharging = false;
        _longPress.Interval = TimeSpan.FromMilliseconds(220); _longPress.Start();
    }
    private void History_PressEnd(object sender, MouseButtonEventArgs e) { CancelPress(); if (_suppressClick) e.Handled = true; }
    private void History_PressMove(object sender,MouseEventArgs e){if(_pressedEntry!=null&&((e.GetPosition(HistoryList)-_pressPosition).Length>8||e.LeftButton!=MouseButtonState.Pressed))CancelPress();}
    internal void CancelPress()
    {
        var wasPressed = _pressedEntry != null;
        _longPress.Stop(); _pressCharging = false;
        if (_pressedEntry != null) _pressedEntry.FavoriteProgress = 0; _pressedEntry = null;
        if (wasPressed && _historyRefreshPending && _favoriteFeedbackCount == 0 && _ready)
        {
            _historyRefreshPending = false;
            Dispatcher.BeginInvoke(async () => { if (_ready) await Refresh(); });
        }
    }
    private void History_ContextMenuOpening(object sender,ContextMenuEventArgs e){if(!AppSession.IsDemo)e.Handled=true;}
    private void History_RightClick(object sender,MouseButtonEventArgs e)
    {
        var completedHold = _suppressClick && Mouse.LeftButton == MouseButtonState.Pressed; CancelPress(); if(completedHold){e.Handled=true;return;}if(AppSession.IsDemo)return;e.Handled=true;
        if(e.OriginalSource is DependencyObject source&&FindParent<Button>(source)==null&&FindParent<ListBoxItem>(source)?.DataContext is HistoryEntry entry)DeleteRow_Click(new FrameworkElement{DataContext=entry},e);
    }
    private async void Thumbnail_Loaded(object sender, RoutedEventArgs e)
    {
        if (sender is Image image) await LoadThumbnail(image);
    }
    private async void Thumbnail_ContextChanged(object sender, DependencyPropertyChangedEventArgs e)
    {
        if (sender is Image { IsLoaded: true } image) await LoadThumbnail(image);
    }
    private async Task LoadThumbnail(Image image)
    {
        if (AppSession.IsDemo || image.DataContext is not HistoryEntry { ThumbnailEligible: true } entry || entry.ThumbnailBytes != null) return;
        var bytes = await _thumbnails.GetAsync(entry, _lifetime.Token);
        // A virtualized Image may now represent another row. Never paint an
        // obsolete request into that recycled container.
        if (!_lifetime.IsCancellationRequested && ReferenceEquals(image.DataContext, entry) && bytes != null) entry.ThumbnailBytes = bytes;
    }
    private async Task LoadCapabilities()
    {
        try
        {
            var capabilities=await NativeBackend.InvokeAsync<JsonElement>("get_history_capabilities",null,_lifetime.Token);
            if(capabilities.TryGetProperty("date_range_filter",out var dates))DateFilter.IsEnabled=dates.GetBoolean();
            if(capabilities.TryGetProperty("categories",out var categories)&&categories.ValueKind==JsonValueKind.Array)
            {
                var available=categories.EnumerateArray().Select(x=>x.GetString()).ToHashSet();
                foreach(var item in CategoryFilter.Items.OfType<ComboBoxItem>().ToArray())
                    if(item.Tag as string!="all"&&!available.Contains(item.Tag as string))CategoryFilter.Items.Remove(item);
                if(CategoryFilter.SelectedItem==null)CategoryFilter.SelectedIndex=0;
            }
            var diagnostics=await NativeBackend.InvokeAsync<JsonElement>("get_migration_diagnostics",null,_lifetime.Token);
            var unresolved=diagnostics.Long("unresolved_count");
            if(unresolved>0)Notify(T("history.migrationWarningPrefix")+" "+unresolved+" "+T("history.migrationWarningSuffix"));
        }catch(OperationCanceledException){}catch(Exception error){Notify(error.Message);}
    }
    private async void RuntimeChanged(object? sender,EventArgs args)
    {
        if(!_ready||AppSession.IsDemo||GuidedTour.Active||Page is not ("history" or "favorites"))return;
        var snapshot=NativeBackend.RuntimeSnapshot;
        if(snapshot.ValueKind!=JsonValueKind.Object)return;
        if(snapshot.TryGetProperty("history_version",out var version)&&version.TryGetInt64(out var current)&&current!=_historyVersion){_historyVersion=current;if(ViewModel.IsLoading)_historyRefreshPending=true;else await Refresh();}
        if(snapshot.TryGetProperty("progress",out var progress)&&progress.ValueKind==JsonValueKind.Object&&progress.TryGetProperty("active",out var active)&&active.GetBoolean()&&SettingsState.Current.Bool("progress_bar_enabled",true))
        {
            TransferPanel.Visibility=Visibility.Visible;
            var sent=progress.GetProperty("sent").GetDouble();var total=Math.Max(1,progress.GetProperty("total").GetDouble());TransferProgress.Value=Math.Clamp(sent/total*100,0,100);
            TransferLabel.Text=$"{progress.GetProperty("completed_files").GetInt32()+1}/{progress.GetProperty("total_files").GetInt32()}  {progress.GetProperty("name").GetString()}  {sent/1048576:0.#}/{total/1048576:0.#} MB";
            _transferBatch=progress.GetProperty("batch_id").GetString();StopTransferButton.Visibility=progress.GetProperty("can_stop").GetBoolean()?Visibility.Visible:Visibility.Collapsed;
        }else TransferPanel.Visibility=Visibility.Collapsed;
        if(snapshot.TryGetProperty("sync_warning",out var warning)&&warning.ValueKind==JsonValueKind.Object)
            await WarningCoordinator.DeliverAsync(warning,IsVisible&&WindowState!=WindowState.Minimized,message=>{SyncButton.ToolTip=message;Notify(message);},_lifetime.Token);
    }
    private async void StopTransfer_Click(object sender,RoutedEventArgs e)
    {
        if(_transferBatch==null||AppSession.Client is not IHistoryPageClient client)return;StopTransferButton.IsEnabled=false;
        try{Notify((await client.CancelBatchAsync(_transferBatch)).Message);}catch(Exception error){Notify(error.Message);}finally{StopTransferButton.IsEnabled=true;}
    }

    private void LanguageChanged(object? sender,EventArgs e){_toast.Stop();Feedback.Visibility=Visibility.Collapsed;ApplyLanguage();ViewModel.RefreshPresentation();UpdateEmpty();UpdateKeyboardHint();}
    private void UpdateKeyboardHint()
    {
        if(KeyboardHint==null)return;
        KeyboardHint.Text=SearchBox.IsKeyboardFocusWithin?Localizer.Choose("↓ 返回条目","↓ Back to entries"):Page=="favorites"?Localizer.Choose("← → 翻页 · F 搜索","← → Pages · F Search"):Localizer.Choose("1–6 取用 · ← → 翻页 · F 搜索","1–6 Restore · ← → Pages · F Search");
        Resources["HistoryKeyboardAvailable"]=HistoryList.IsKeyboardFocusWithin&&ViewModel.IsQueryCurrent&&!RestoreActivity.IsBusy;
    }
    private async void TourChanged(object? sender,EventArgs e)
    {
        _searchDebounce.Stop();_toast.Stop();Feedback.Visibility=Visibility.Collapsed;CancelPress();_suppressClick=false;ViewModel.ClearSnapshot();_thumbnails.Clear();_preview?.Close();_preview=null;
        _tourOverlay?.Dispose();_tourOverlay=null;
        if(Page=="settings")BuildProductionSettings();
        if(Page=="connections")BuildProductionConnections();
        if(GuidedTour.Active)_tourOverlay=new(this,Root);
        if(_ready&&Page is "history" or "favorites"){try{await ViewModel.RefreshAsync();UpdateEmpty();}catch(Exception error){ShowLoadError(error);}}
    }
    internal FrameworkElement? TourTarget(string target) => target switch
    {
        "history-items"=>HistoryList,"history-search"=>SearchChrome,"favorites"=>FavoritesButton,
        _=>QualityChecks.Descendants(this).OfType<FrameworkElement>().FirstOrDefault(x=>x.Tag as string=="tour:"+target)
    };
    private void ApplyLanguage()
    {
        HistoryEntry.Localize=Localizer.Text;
        Title=T(Page switch{"settings"=>"settings.title","connections"=>"connections.title","favorites"=>"favorites.title",_=>"history.title"});
        if(TitleHost.Content is Grid bar&&bar.Children.OfType<TextBlock>().FirstOrDefault() is {} title)title.Text=Title;
        SearchBox.Tag=T("history.searchPlaceholder");ClearButton.Content=T("history.clearAll");
        PinButton.ToolTip=T(Topmost?"history.unpinWindow":"history.pinWindow");FavoritesButton.ToolTip=T("favorites.open");
        PreviousPageButton.ToolTip=T("history.prev");NextPageButton.ToolTip=T("history.next");
        foreach(var item in CategoryFilter.Items.OfType<ComboBoxItem>())item.Content=T("history.category."+item.Tag);
        foreach(var item in DateFilter.Items.OfType<ComboBoxItem>())item.Content=T("history.date."+item.Tag);
        RetryHistoryButton.Content=T("history.loadError")+" · "+T("history.retry");StopTransferButton.Content=T("history.stopTransfer");
        Resources["HistoryCopyAll"]=T("history.copyAll");Resources["HistoryRestore"]=T("history.restoreEntry");Resources["HistoryPreview"]=T("history.preview.title");
        Resources["HistoryFavorite"]=Localizer.Choose("收藏 / 取消收藏","Add / remove favorite");Resources["HistoryDelete"]=T("history.deleteEntry");
    }

}
