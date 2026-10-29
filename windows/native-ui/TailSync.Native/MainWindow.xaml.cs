using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using TailSync.Native.Core;
using Windows.Graphics;
using Windows.System;

namespace TailSync.Native;

public sealed partial class MainWindow : Window
{
    private static readonly DemoTailSyncClient DemoClient = new();
    private static readonly Dictionary<string, MainWindow> Windows = [];
    private static bool _syncEnabled = true;
    private readonly string _page;
    private bool _ready;
    private bool _dialogOpen;
    private Window? _preview;
    public ShellViewModel ViewModel { get; } = new(DemoClient);

    public MainWindow() : this("history") { }
    public MainWindow(string page)
    {
        _page = page;
        InitializeComponent();
        StartDate.Date = new DateTimeOffset(2026, 10, 24, 0, 0, 0, TimeSpan.FromHours(8));
        EndDate.Date = new DateTimeOffset(2026, 10, 25, 0, 0, 0, TimeSpan.FromHours(8));
        BuildMacPages();
        Windows[page] = this;
        ExtendsContentIntoTitleBar = true;
        if (AppWindow.Presenter is OverlappedPresenter chrome) chrome.SetBorderAndTitleBar(true, false);
        SetTitleBar(TitleBar);
        Title = WindowHeading.Text = page switch
        {
            "settings" => "Settings", "connections" => MacDesign.Text("connections.title"), "favorites" => "Favorites", _ => "History"
        };
        HistoryPage.Visibility = page is "history" or "favorites" ? Visibility.Visible : Visibility.Collapsed;
        PinButton.Visibility = FavoritesButton.Visibility = page == "favorites" ? Visibility.Collapsed : Visibility.Visible;
        ClearButton.Visibility = page == "favorites" ? Visibility.Collapsed : Visibility.Visible;
        SettingsPage.Visibility = page == "settings" ? Visibility.Visible : Visibility.Collapsed;
        ConnectionsPage.Visibility = page == "connections" ? Visibility.Visible : Visibility.Collapsed;
        Root.Loaded += async (_, _) =>
        {
            _ready = true;
            var size = MacDesign.WindowSize(page);
            ResizeInDips(size.Width, size.Height);
            UpdateCaption();
            SyncSwitch.IsChecked = _syncEnabled;
            UpdateSyncDot();
            await ViewModel.NavigateAsync(page);
            UpdateEmptyState();
        };
        Root.ActualThemeChanged += (_, _) => { UpdateCaption(); BuildMacPages(); SyncSwitch.IsChecked = _syncEnabled; };
        Activated += async (_, _) =>
        {
            if (_ready && (_page is "history" or "favorites")) await RefreshAsync();
        };
        Closed += (_, _) => { Windows.Remove(_page); _preview?.Close(); };
    }

    private void CloseWindow_Click(object sender, RoutedEventArgs e) => Close();
    private void MinimizeWindow_Click(object sender, RoutedEventArgs e) { if (AppWindow.Presenter is OverlappedPresenter p) p.Minimize(); }
    private void MaximizeWindow_Click(object sender, RoutedEventArgs e)
    {
        if (AppWindow.Presenter is not OverlappedPresenter p) return;
        if (p.State == OverlappedPresenterState.Maximized) p.Restore(); else p.Maximize();
    }

    private void ResizeInDips(int width, int height)
    {
        var scale = Root.XamlRoot?.RasterizationScale ?? 1;
        AppWindow.Resize(new SizeInt32((int)(width * scale), (int)(height * scale)));
    }

    private void UpdateCaption()
    {
        AppWindow.TitleBar.ButtonBackgroundColor = Microsoft.UI.Colors.Transparent;
        AppWindow.TitleBar.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
        AppWindow.TitleBar.ButtonForegroundColor = Root.ActualTheme == ElementTheme.Dark
            ? Microsoft.UI.Colors.White : Microsoft.UI.Colors.Black;
        if (_ready) UpdateSyncDot();
    }

    private async Task RefreshAsync()
    {
        try { await ViewModel.RefreshAsync(); UpdateEmptyState(); }
        catch (Exception error) { Notify(new(false, $"读取示例内容失败：{error.Message}")); }
    }
    private void UpdateEmptyState() => EmptyLabel.Visibility = ViewModel.Entries.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    private void Notify(ActionResult result)
    {
        Feedback.Severity = result.Success ? InfoBarSeverity.Informational : InfoBarSeverity.Warning;
        Feedback.Message = result.Message;
        Feedback.IsOpen = true;
        UpdateEmptyState();
    }
    private async void Search_TextChanged(object sender, TextChangedEventArgs args)
    {
        if (!_ready) return;
        ViewModel.Keyword = SearchBox.Text;
        await RefreshAsync();
    }
    private async void Filter_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!_ready) return;
        ViewModel.Category = (CategoryFilter.SelectedItem as ComboBoxItem)?.Tag?.ToString() ?? "all";
        ViewModel.Date = (DateFilter.SelectedItem as ComboBoxItem)?.Tag?.ToString() ?? "all";
        CustomDates.Visibility = ViewModel.Date == "custom" ? Visibility.Visible : Visibility.Collapsed;
        ViewModel.Start = DateOnly.FromDateTime((StartDate.Date ?? DateTimeOffset.Now).DateTime);
        ViewModel.End = DateOnly.FromDateTime((EndDate.Date ?? DateTimeOffset.Now).DateTime);
        await RefreshAsync();
    }
    private async void CustomDate_Changed(object sender, CalendarDatePickerDateChangedEventArgs e)
    {
        if (!_ready) return;
        ViewModel.Start = DateOnly.FromDateTime((StartDate.Date ?? DateTimeOffset.Now).DateTime); ViewModel.End = DateOnly.FromDateTime((EndDate.Date ?? DateTimeOffset.Now).DateTime); await RefreshAsync();
    }
    private void Pin_Click(object sender, RoutedEventArgs e)
    {
        if (AppWindow.Presenter is OverlappedPresenter presenter) presenter.IsAlwaysOnTop = PinButton.IsChecked == true;
    }
    private void OpenFavorites_Click(object sender, RoutedEventArgs e) => Open("favorites");
    private void OpenSettings_Click(object sender, RoutedEventArgs e) => Open("settings");
    private void OpenConnections_Click(object sender, RoutedEventArgs e) => Open("connections");
    private static void Open(string page)
    {
        if (Windows.TryGetValue(page, out var existing)) { existing.Activate(); return; }
        new MainWindow(page).Activate();
    }
    private bool SelectRow(object sender)
    {
        if ((sender as FrameworkElement)?.Tag is not HistoryEntry entry) return false;
        ViewModel.SelectedEntry = entry;
        return true;
    }
    private async void RestoreRow_Click(object sender, RoutedEventArgs e)
    {
        if (SelectRow(sender)) Notify(await ViewModel.RestoreSelectedAsync());
    }
    private async void FavoriteRow_Click(object sender, RoutedEventArgs e)
    {
        if (!SelectRow(sender)) return;
        Notify(await ViewModel.ToggleFavoriteAsync());
    }
    private void PreviewRow_Click(object sender, RoutedEventArgs e) { if (SelectRow(sender)) ShowPreview(); }
    private async void DeleteRow_Click(object sender, RoutedEventArgs e)
    {
        if (!SelectRow(sender) || ViewModel.SelectedEntry is not { } entry || _dialogOpen) return;
        if (entry.Pinned && _page != "favorites") { Notify(new(false, "收藏受到保护，请到收藏窗口删除")); return; }
        _dialogOpen = true;
        try
        {
            var result = await new ContentDialog { XamlRoot = Root.XamlRoot, Title = "删除这条示例内容？", Content = entry.Title,
                PrimaryButtonText = "删除", CloseButtonText = "取消", DefaultButton = ContentDialogButton.Close }.ShowAsync();
            if (result == ContentDialogResult.Primary) Notify(await ViewModel.DeleteSelectedAsync());
        }
        finally { _dialogOpen = false; }
    }
    private void Info_Click(object sender, RoutedEventArgs e)
    {
        if (!SelectRow(sender) || ViewModel.SelectedEntry is not { } entry || sender is not FrameworkElement anchor) return;
        var details = new StackPanel { Spacing = 6, MinWidth = 230 };
        details.Children.Add(Label(MacDesign.Text("history.composite.details"), 13));
        foreach (var (key, value) in new[] { ("formats", "text / html"), ("representations", "2"), ("images", "0"),
            ("nativeItems", "1"), ("nativeOnly", entry.LocalOnly ? "是" : "否"), ("completeness", "完整"),
            ("restorable", entry.LocalOnly ? "无" : "text / html"), ("withheld", entry.LocalOnly ? "原生内容" : "无") })
            details.Children.Add(SettingRow(MacDesign.Text("history.composite.diag." + key), "", Label(value, 11), 3));
        var flyout = new Flyout { Content = details, Placement = FlyoutPlacementMode.Top };
        flyout.ShowAt(anchor);
    }
    private async void Clear_Click(object sender, RoutedEventArgs e)
    {
        if (_dialogOpen || _page != "history") return;
        _dialogOpen = true;
        try
        {
            var confirmed = await new ContentDialog { XamlRoot = Root.XamlRoot, Title = "清空示例历史？", Content = "收藏内容会保留。真实历史不会受到影响。",
                PrimaryButtonText = "清空", CloseButtonText = "取消", DefaultButton = ContentDialogButton.Close }.ShowAsync();
            if (confirmed != ContentDialogResult.Primary) return;
            foreach (var item in await DemoClient.GetHistoryAsync(new()))
                if (!item.Pinned) await DemoClient.DeleteAsync(item.Id, false);
            await RefreshAsync();
            Notify(new(true, "已清空未收藏的示例内容"));
        }
        finally { _dialogOpen = false; }
    }
    private async void History_KeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter) { e.Handled = true; Notify(await ViewModel.RestoreSelectedAsync()); }
        else if (e.Key == VirtualKey.Space) { e.Handled = true; ShowPreview(); }
        else if (e.Key == VirtualKey.Escape) { e.Handled = true; SearchBox.Focus(FocusState.Keyboard); }
    }
    private async void History_DoubleTapped(object sender, DoubleTappedRoutedEventArgs e) => Notify(await ViewModel.RestoreSelectedAsync());
    private void FocusSearch_Invoked(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        if (_page is not ("history" or "favorites")) return;
        SearchBox.Focus(FocusState.Keyboard);
        args.Handled = true;
    }
    private void ShowPreview()
    {
        if (ViewModel.SelectedEntry is not { } entry) return;
        _preview?.Close();
        var window = new Window { Title = "Preview", ExtendsContentIntoTitleBar = true };
        if (window.AppWindow.Presenter is OverlappedPresenter chrome) chrome.SetBorderAndTitleBar(true, false);
        var root = new Grid { RequestedTheme = Root.RequestedTheme, Background = Ink("SurfaceBrush") };
        foreach (var height in new[] { new GridLength(32), new GridLength(64), new GridLength(56), new GridLength(1, GridUnitType.Star) }) root.RowDefinitions.Add(new() { Height = height });
        var drag = new Grid { Margin = new(78, 0, 0, 0) }; drag.Children.Add(new TextBlock { Text = "Preview", Margin = new(6, 0, 0, 0), FontSize = 13, VerticalAlignment = VerticalAlignment.Center, Foreground = Ink("SecondaryBrush") }); root.Children.Add(drag);
        var lights = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10, Margin = new(9, 0, 0, 0), VerticalAlignment = VerticalAlignment.Center };
        foreach (var color in new[] { "#FF7067", "#FFCD41", "#78D54E" })
        {
            var light = new Button { Width = 13, Height = 13, Style = (Style)Application.Current.Resources["TrafficLight"], Background = ColorBrush(color), BorderBrush = ColorBrush(color) };
            light.Click += (_, _) => { if (color == "#FF7067") window.Close(); else if (window.AppWindow.Presenter is OverlappedPresenter p) { if (color == "#FFCD41") p.Minimize(); else if (p.State == OverlappedPresenterState.Maximized) p.Restore(); else p.Maximize(); } };
            lights.Children.Add(light);
        }
        root.Children.Add(lights);
        var restore = SmallButton(T("history.preview.restore")); restore.Click += async (_, _) => Notify(await DemoClient.RestoreAsync(entry.Id));
        var heading = SettingRow(entry.Title, entry.SizeLabel, restore, 12); Grid.SetRow(heading, 1); root.Children.Add(heading);
        var text = new TextBlock { Text = entry.Body, FontFamily = new(entry.Preview == "code" ? "Cascadia Code" : "Songti SC, SimSun"), FontSize = 13, TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true, Foreground = Ink("PrimaryBrush") };
        var toolbar = new Grid { Padding = new(16, 8, 16, 8), Background = Ink("CanvasBrush") }; Grid.SetRow(toolbar, 2);
        var search = new TextBox { PlaceholderText = T("history.preview.search"), Width = 130, MinHeight = 28, FontSize = 11, HorizontalAlignment = HorizontalAlignment.Left };
        AutomationProperties.SetName(search, T("history.preview.search"));
        // Text selection remains native; demo search highlights the first literal match.
        search.TextChanged += (_, _) => { text.TextHighlighters.Clear(); if (search.Text.Length == 0) return; var at = entry.Body.IndexOf(search.Text, StringComparison.OrdinalIgnoreCase); if (at >= 0) { var h = new Microsoft.UI.Xaml.Documents.TextHighlighter(); h.Ranges.Add(new Microsoft.UI.Xaml.Documents.TextRange { StartIndex = at, Length = search.Text.Length }); text.TextHighlighters.Add(h); } };
        toolbar.Children.Add(search);
        var tools = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Right };
        tools.Children.Add(SmallButton("−", () => text.FontSize = Math.Max(10, text.FontSize - 1))); tools.Children.Add(SmallButton("+", () => text.FontSize = Math.Min(28, text.FontSize + 1))); tools.Children.Add(SmallButton("↵", () => text.TextWrapping = text.TextWrapping == TextWrapping.Wrap ? TextWrapping.NoWrap : TextWrapping.Wrap)); toolbar.Children.Add(tools); root.Children.Add(toolbar);
        var body = new StackPanel { Padding = new(20), Spacing = 16 };
        if (entry.Preview == "image") body.Children.Add(new Image { Source = new SvgImageSource(new Uri("ms-appx:///Assets/Landscape.svg")), Stretch = Stretch.Uniform });
        body.Children.Add(text); var scroll = new ScrollViewer { Content = body }; Grid.SetRow(scroll, 3); root.Children.Add(scroll);
        var close = new KeyboardAccelerator { Key = VirtualKey.Escape }; close.Invoked += (_, args) => { args.Handled = true; window.Close(); }; root.KeyboardAccelerators.Add(close);
        window.Content = root; window.SetTitleBar(drag);
        var scale = Root.XamlRoot?.RasterizationScale ?? 1; window.AppWindow.Resize(new((int)(640 * scale), (int)(560 * scale)));
        _preview = window; window.Closed += (_, _) => { if (_preview == window) _preview = null; }; window.Activate();
    }
    private void UpdateSyncDot() => SyncDot.Fill = new SolidColorBrush(_syncEnabled
        ? (Root.ActualTheme == ElementTheme.Dark ? Microsoft.UI.ColorHelper.FromArgb(255, 117, 170, 134) : Microsoft.UI.ColorHelper.FromArgb(255, 68, 116, 90))
        : Microsoft.UI.ColorHelper.FromArgb(255, 152, 149, 139));
    private void Sync_Toggled(object sender, RoutedEventArgs e)
    {
        if (!_ready || SyncSwitch.IsChecked == _syncEnabled) return;
        _syncEnabled = SyncSwitch.IsChecked == true;
        foreach (var window in Windows.Values) { window.SyncSwitch.IsChecked = _syncEnabled; window.UpdateSyncDot(); }
        Notify(new(true, "已切换本次演示的同步状态"));
    }
    private void DemoSetting_Changed(object sender, RoutedEventArgs e) { if (_ready) Notify(new(true, "已更新演示设置；未修改系统设置")); }
    private void DemoSelection_Changed(object sender, SelectionChangedEventArgs e) { if (_ready) Notify(new(true, "已更新演示选项；未更改真实设置")); }
    private async void Guide_Click(object sender, RoutedEventArgs e) => await MessageAsync("新手引导", "1. 在历史列表选择内容\n2. 按空格打开独立预览\n3. 双击或按 Enter 演示恢复\n\n当前原型使用示例数据，不写入系统剪贴板。");
    private async void Shortcut_Click(object sender, RoutedEventArgs e) => await MessageAsync("快捷键", "此处使用与 Mac 版一致的键帽和录制控件。\n真实全局快捷键注册将在接入后台后提供。");
    private async void Pair_Click(object sender, RoutedEventArgs e)
    {
        if (_dialogOpen) return;
        _dialogOpen = true;
        try
        {
            var sheet = new StackPanel { Width = 292, Spacing = 14, Padding = new(8), MinHeight = 240 };
            var shield = Symbol("\uE83D", 28); shield.HorizontalAlignment = HorizontalAlignment.Center; sheet.Children.Add(shield);
            foreach (var (text, size) in new[] { ("DESKTOP-V62KRAF", 17d), ("829 416", 34d), (T("settings.compareCode"), 12d), ("8F:62:41:BE:10:D2:59:AC", 10d) })
            { var label = Label(text, size); label.TextAlignment = TextAlignment.Center; if (size == 34) label.FontFamily = new("Cascadia Mono"); sheet.Children.Add(label); }
            await new ContentDialog { XamlRoot = Root.XamlRoot, Content = sheet, PrimaryButtonText = T("settings.codesMatch"), CloseButtonText = T("settings.cancel"), Background = Ink("CanvasBrush") }.ShowAsync();
        }
        finally { _dialogOpen = false; }
    }
    private async void Invite_Click(object sender, RoutedEventArgs e) => await MessageAsync("远程邀请", "一次性邀请将在接入 Rust 后台后提供。当前原型不创建可用邀请。");
    private async Task MessageAsync(string title, string text)
    {
        if (_dialogOpen) return;
        _dialogOpen = true;
        try { await new ContentDialog { XamlRoot = Root.XamlRoot, Title = title, Content = new TextBlock { Text = text, TextWrapping = TextWrapping.Wrap }, CloseButtonText = "知道了" }.ShowAsync(); }
        finally { _dialogOpen = false; }
    }
}
