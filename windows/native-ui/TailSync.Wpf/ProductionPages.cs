using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using Microsoft.Win32;

namespace TailSync.Wpf;

public partial class MainWindow
{
    private readonly CancellationTokenSource _pageLifetime = new();
    private JsonElement _storage, _oldStorage;
    private bool _productionHooks;
    private TextBlock? _updateMessage;
    private Button? _checkUpdate, _installUpdate;
    private readonly List<Action<JsonElement>> _settingsRefreshers = [];

    private void ProductionHooks()
    {
        if (_productionHooks) return;
        _productionHooks = true;
        Localizer.Changed += ProductionLocaleChanged;
        SettingsState.Changed += ProductionSettingsChanged;
        Closed += (_, _) => { _pageLifetime.Cancel(); _pageLifetime.Dispose(); Localizer.Changed -= ProductionLocaleChanged; SettingsState.Changed -= ProductionSettingsChanged; };
    }
    private void ProductionSettingsChanged(object? sender,EventArgs e)
    {
        foreach(var refresh in _settingsRefreshers.ToArray())refresh(SettingsState.Current);
    }
    private void ProductionLocaleChanged(object? sender, EventArgs e)
    {
        if (Page == "settings") BuildProductionSettings();
        if (Page == "connections") BuildProductionConnections();
    }
    private static string P(string key) => Localizer.Text(key);
    private static string Words(string chinese, string english) => Localizer.Choose(chinese, english);
    private static StackPanel ProductionSection(Panel parent, string title, string description)
    {
        var wrapper = new StackPanel { Margin = new(20, 0, 20, 20) };
        var heading = Ui.Text(title, 14, "Secondary"); heading.FontWeight = FontWeights.SemiBold; heading.Margin = new(4, 0, 4, 8);
        wrapper.Children.Add(heading);
        var rows = new StackPanel(); wrapper.Children.Add(Ui.SettingsCard(rows)); parent.Children.Add(wrapper); return rows;
    }
    private static void AddSetting(Panel panel, UIElement content)
    {
        if (panel.Children.Count > 0) Ui.Divider(panel);
        panel.Children.Add(content);
    }
    private Button AsyncButton(string icon, string text, Func<Task> action)
    {
        var button = AsyncButton(text, action); ButtonDesign.SetIcon(button, icon); return button;
    }
    private Button AsyncButton(string text, Func<Task> action, string? style = null)
    {
        var button = Ui.Button(text, style: style);
        button.Click += async (_, _) => await RunAction(button, action);
        return button;
    }
    private async Task RunAction(FrameworkElement element, Func<Task> action)
    {
        if (!element.IsEnabled || _pageLifetime.IsCancellationRequested) return;
        element.IsEnabled = false; ControlMotion.Busy(element, true);
        try { await action(); }
        catch (OperationCanceledException) when (_pageLifetime.IsCancellationRequested) { }
        catch (Exception error) { if (!_pageLifetime.IsCancellationRequested) Notify(error.Message); }
        finally { ControlMotion.Busy(element, false); if (!_pageLifetime.IsCancellationRequested) element.IsEnabled = true; }
    }
    private ToggleButton PersistentSwitch(string title, bool value, Func<bool, Task> save)
    {
        var toggle = Ui.Name(new ToggleButton { IsChecked = value }, title); toggle.SetResourceReference(StyleProperty, "MacSwitch");
        toggle.Click += async (_, _) =>
        {
            var next = toggle.IsChecked == true;
            await RunAction(toggle, async () => { try { await save(next); Notify(P("settings.saved")); } catch { toggle.IsChecked = !next; throw; } });
        };
        return toggle;
    }
    private void BuildProductionSettings()
    {
        ProductionHooks(); SettingsContent.Children.Clear(); _settingsRefreshers.Clear();
        var header = Ui.PageHeader(P("settings.title"), Words("管理同步、快捷键和应用偏好", "Manage sync, shortcuts, and app preferences.")); header.Margin = new(20, 2, 20, 22); SettingsContent.Children.Add(header);
        var settings = SettingsState.Current;
        if (settings.ValueKind != JsonValueKind.Object)
        {
            SettingsContent.Children.Add(AsyncButton("refresh", P("settings.loading"), async () => { await SettingsState.ReloadAsync(_pageLifetime.Token); BuildProductionSettings(); })); return;
        }
        if(!GuidedTour.Suppressed)
        {
            var start=AsyncButton("help",Words("新手引导","Getting started"),()=>GuidedTour.StartAsync());
            start.IsEnabled=!GuidedTour.Active;start.Margin=new(20,0,20,16);SettingsContent.Children.Add(start);
        }
        var general = ProductionSection(SettingsContent, P("settings.general"), P("settings.generalDescription"));
        _syncSwitch = PersistentSwitch(P("settings.syncEnabled"), AppSession.Sync, value => AppSession.SetSyncAsync(value));
        AddSetting(general, SettingRow(P("settings.syncEnabled"), P("settings.syncEnabledDescription"), _syncSwitch));
        var auto = PersistentSwitch(P("settings.launchAtLogin"), AutostartService.IsEnabled, value => { GuidedTour.CheckClipboard();AutostartService.SetEnabled(value); return Task.CompletedTask; });
        auto.IsEnabled = !NativeBackend.IsIsolated&&!GuidedTour.Active;
        AddSetting(general, SettingRow(P("settings.launchAtLogin"), P("settings.launchAtLoginDescription"), auto));
        AddSetting(general, SettingRow(P("settings.syncShortcut"), P("settings.syncShortcutDescription"), ProductionShortcut("sync_shortcut", "set_sync_shortcut", "CommandOrControl+Shift+S")));
        AddSetting(general, SettingRow(P("settings.historyShortcut"), P("settings.historyShortcutDescription"), ProductionShortcut("history_shortcut", "set_history_shortcut", "CommandOrControl+Shift+H")));
        foreach (var (field, key) in new[] { ("notifications_enabled", "notifications"), ("progress_bar_enabled", "progressBar") })
        {
            var toggle=PersistentSwitch(P("settings."+key),settings.Bool(field),value=>SettingsState.SaveAsync(field,value,_pageLifetime.Token));
            _settingsRefreshers.Add(value=>toggle.IsChecked=value.Bool(field));
            AddSetting(general,SettingRow(P("settings."+key),P("settings."+key+"Description"),toggle));
        }

        general.Tag="tour:general";
        var updates = ProductionSection(SettingsContent, P("settings.updates"), P("settings.updatesDescription"));
        _updateMessage = Ui.Detail(P("settings.loading"));
        _checkUpdate = AsyncButton("refresh", P("settings.updateCheck"), CheckForUpdate);
        ButtonDesign.SetIcon(_checkUpdate, "refresh");
        _installUpdate = AsyncButton(P("settings.updateInstall"), async () =>
        {
            if (DemoDialog.Confirm(this, P("settings.updateInstall"), Words("更新完成后会重启应用。", "The application will restart after updating."), P("settings.updateInstall")))
                await NativeBackend.InvokeAsync("install_update", token: _pageLifetime.Token);
        }, "PrimaryButton");
        _installUpdate.Visibility = Visibility.Collapsed;
        var updateActions = Ui.Stack(Orientation.Horizontal, 10, _checkUpdate, _installUpdate);
        updates.Children.Add(Ui.PreferenceRow(_updateMessage, updateActions, "download"));
        _ = LoadUpdateStatus();

        updates.Tag="tour:updates";
        var history = ProductionSection(SettingsContent, P("settings.history"), P("settings.historyDescription"));
        var limit = Ui.Name(new Slider { Minimum = 10, Maximum = 500, Value = settings.Long("history_limit", 100), TickFrequency = 1, Width = 180 }, P("settings.historyLimit"));
        var limitValue = Ui.Text(((int)limit.Value).ToString(), 13, "Accent"); limitValue.FontWeight = FontWeights.SemiBold;
        limitValue.HorizontalAlignment = HorizontalAlignment.Center;
        var limitBadge = new Border { Child = limitValue, MinWidth = 46, Padding = new(10, 6, 10, 6), CornerRadius = new(10) };
        limitBadge.SetResourceReference(Border.BackgroundProperty, "AccentSoftBrush");
        limit.ValueChanged += (_, _) => limitValue.Text = ((int)limit.Value).ToString();
        _settingsRefreshers.Add(value=>{if(!limit.IsMouseCaptureWithin&&!limit.IsKeyboardFocusWithin)limit.Value=value.Long("history_limit",100);});
        async Task SaveLimit()
        {
            var draft = (int)limit.Value;
            if (draft == SettingsState.Current.Long("history_limit")) return;
            await RunAction(limit, async () => { try { await SettingsState.SaveAsync("history_limit", draft, _pageLifetime.Token); Notify(P("settings.saved")); } finally { limit.Value = SettingsState.Current.Long("history_limit", 100); } });
        }
        limit.AddHandler(Thumb.DragCompletedEvent, new DragCompletedEventHandler(async (_, _) => await SaveLimit()));
        limit.PreviewMouseLeftButtonUp += async (_, _) => await SaveLimit();
        limit.LostKeyboardFocus += async (_, _) => await SaveLimit();
        limit.KeyUp += async (_, e) => { if (e.Key is Key.Left or Key.Right or Key.Up or Key.Down or Key.Home or Key.End) await SaveLimit(); };
        var limitDescription = Ui.Detail(P("settings.limitDescription").Replace("{value}", ((int)limit.Value).ToString()));
        limit.ValueChanged += (_, _) => limitDescription.Text = P("settings.limitDescription").Replace("{value}", ((int)limit.Value).ToString());
        var limitCopy = Ui.Label(P("settings.historyLimit")); limitCopy.Children.Add(limitDescription);
        AddSetting(history, Ui.PreferenceRow(limitCopy, Ui.Stack(Orientation.Horizontal, 16, limit, limitBadge)));

        history.Tag="tour:history-settings";
        var autoImages=PersistentSwitch(Words("自动保存网络图片","Automatically save network images"),settings.Bool("auto_save_network_images"),value=>SettingsState.SaveAsync("auto_save_network_images",value,_pageLifetime.Token));
        _settingsRefreshers.Add(value=>autoImages.IsChecked=value.Bool("auto_save_network_images"));
        AddSetting(history,SettingRow(Words("自动保存网络图片","Automatically save network images"),Words("复制网页图文时保存可用图片，保留已完成的下载。","Save available images with copied web content, preserving completed downloads."),autoImages));
        var storage = ProductionSection(SettingsContent, P("settings.storage"), P("settings.storageDescription"));
        BuildStorageRows(storage);
        storage.Tag="tour:storage";
        var connections = ProductionSection(SettingsContent, P("settings.connectionsTitle"), P("settings.connectionsDescription"));
        AddSetting(connections, SettingRow(P("settings.manageConnections"), P("settings.manageConnectionsDescription"), Ui.Button(P("settings.openConnections"), () => Open("connections"), icon: "devices")));
        var appearance = ProductionSection(SettingsContent, P("settings.appearance"), P("settings.appearanceDescription"));appearance.Tag="tour:appearance";
        appearance.Children.Add(new ThemeManagerView(!GuidedTour.Active));
        var language = Ui.Name(new ComboBox { Width = 150 }, P("settings.language")); language.Items.Add("简体中文"); language.Items.Add("English"); language.SelectedIndex = Localizer.Locale == "en" ? 1 : 0;
        language.SelectionChanged += async (_, _) => {var next=language.SelectedIndex==1?"en":"zh-CN";if(next!=SettingsState.Current.String("language"))await RunAction(language, async () => await SettingsState.SaveAsync("language",next,_pageLifetime.Token));};
        AddSetting(appearance, SettingRow(P("settings.language"), P("settings.languageDescription"), language));
    }

    private FrameworkElement ProductionShortcut(string field, string command, string defaultShortcut)
    {
        var label = SettingsState.Current.String(field);
        var record = AsyncButton(ShortcutRecorderWindow.Display(label), async () =>
        {
            var dialog = new ShortcutRecorderWindow(this, field, command);
            await dialog.OpenAsync();
            await SettingsState.ReloadAsync(_pageLifetime.Token);
        });
        var reset = AsyncButton("↺", async () => { await NativeBackend.InvokeAsync(command, new { shortcut = defaultShortcut }, _pageLifetime.Token); await SettingsState.ReloadAsync(_pageLifetime.Token); UpdateShortcut(); }); reset.ToolTip = Words("重置快捷键", "Reset shortcut");
        var clear = AsyncButton("×", async () => { await NativeBackend.InvokeAsync(command, new { shortcut = "" }, _pageLifetime.Token); await SettingsState.ReloadAsync(_pageLifetime.Token); UpdateShortcut(); }); clear.ToolTip = P("settings.shortcutClear");
        record.Click += (_, _) => record.Dispatcher.BeginInvoke(() => { if (IsVisible) UpdateShortcut(); });
        // Re-read after a modal recording finishes, rather than displaying an optimistic value.
        void UpdateShortcut()
        {
            var keys = new StackPanel { Orientation = Orientation.Horizontal };
            keys.Children.Add(new VectorIcon { Kind = "keyboard", Margin = new(0, 0, 7, 0) });
            var shortcut = SettingsState.Current.String(field);
            keys.Children.Add(ShortcutDisplayFormatter.Keycaps(shortcut));
            keys.Children.Add(new VectorIcon { Kind = "edit", Width = 12, Height = 12, Margin = new(5, 0, 0, 0) });
            record.Content = keys;
            record.ToolTip = ShortcutDisplayFormatter.AccessibleName(shortcut);
            Ui.Name(record, Words("更改快捷键", "Change shortcut") + ": " + ShortcutDisplayFormatter.AccessibleName(shortcut));
        }
        record.IsEnabledChanged += (_, _) => { if (record.IsEnabled) UpdateShortcut(); };
        _settingsRefreshers.Add(_ => UpdateShortcut()); UpdateShortcut();
        reset.Content = null; ButtonDesign.SetIcon(reset, "refresh"); reset.SetResourceReference(StyleProperty, "IconButton"); Ui.Name(reset, Words("重置快捷键", "Reset shortcut"));
        clear.Content = null; ButtonDesign.SetIcon(clear, "close"); clear.SetResourceReference(StyleProperty, "IconButton"); Ui.Name(clear, P("settings.shortcutClear"));
        return Ui.Stack(Orientation.Horizontal, 6, record, reset, clear);
    }

    private async Task LoadUpdateStatus()
    {
        try
        {
            var status = await NativeBackend.InvokeAsync<JsonElement>("get_update_status", token: _pageLifetime.Token);
            if (_updateMessage == null || _pageLifetime.IsCancellationRequested) return;
            _updateMessage.Inlines.Clear();
            var version = new System.Windows.Documents.Run(Words("当前版本", "Current version") + ": " + status.String("current_version")) { FontSize = 14, FontWeight = FontWeights.SemiBold };
            version.SetResourceReference(System.Windows.Documents.TextElement.ForegroundProperty, "PrimaryBrush");
            _updateMessage.Inlines.Add(version);
            _updateMessage.Inlines.Add(new System.Windows.Documents.LineBreak());
            _updateMessage.Inlines.Add(Words("可检查更新 · 需手动安装", "Check for updates · manual installation"));
            if (_checkUpdate != null) _checkUpdate.IsEnabled = status.Bool("updates_enabled");
        }
        catch (Exception error) { if (_updateMessage != null) _updateMessage.Text = error.Message; }
    }
    private async Task CheckForUpdate()
    {
        var update = await NativeBackend.InvokeAsync<JsonElement>("check_for_update", token: _pageLifetime.Token);
        if (_updateMessage == null) return;
        _updateMessage.Text = update.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined ? P("settings.updateCurrent") : update.String("version") + "\n" + update.String("notes") + "\n" + Words("需使用包含 WPF 前端的发行包更新。", "Use a release package that includes the WPF frontend.");
    }
    private void BuildStorageRows(StackPanel rows)
    {
        rows.Children.Clear();
        var settings = SettingsState.Current;
        var location = SettingRow(_storage.String("root", settings.String("storage_root")), _storage.Bool("available", true) ? FormatBytes(_storage.Long("used_bytes")) + " / " + FormatBytes(settings.Long("storage_quota_bytes")) : _storage.String("error"), AsyncButton("folder", P("settings.storageChange"), async () =>
        {
            var folder = new OpenFolderDialog { Title = P("settings.storageChange") };
            if (folder.ShowDialog(this) != true) return;
            if (!DemoDialog.Confirm(this, P("settings.storageChange"), Words("将加密历史和文件迁移到所选目录。迁移期间请保持应用开启。", "Move encrypted history and files to the selected directory. Keep the application open during migration."), P("settings.storageChange"))) return;
            _oldStorage = await NativeBackend.InvokeAsync<JsonElement>("change_storage_location", new { parent = folder.FolderName }, _pageLifetime.Token);
            await SettingsState.ReloadAsync(_pageLifetime.Token);
            _storage = await NativeBackend.InvokeAsync<JsonElement>("get_storage_status", token: _pageLifetime.Token); BuildStorageRows(rows);
        }));
        AddSetting(rows, location);
        var quota = Ui.Name(new TextBox { Width = 84, Text = Math.Max(1, settings.Long("storage_quota_bytes") / (1024 * 1024 * 1024)).ToString(), MaxLength = 5 }, P("settings.storageQuota"));
        quota.PreviewTextInput += (_, e) => e.Handled = e.Text.Any(c => !char.IsAsciiDigit(c));
        async Task SaveQuota()
        {
            if (!int.TryParse(quota.Text, out var gib) || gib is < 1 or > 16384) { quota.Text = (SettingsState.Current.Long("storage_quota_bytes") / (1024 * 1024 * 1024)).ToString(); Notify(Words("存储配额必须为 1–16384 GiB。", "Storage quota must be 1–16384 GiB.")); return; }
            var bytes = gib * 1024L * 1024 * 1024;
            if (bytes == SettingsState.Current.Long("storage_quota_bytes")) return;
            await RunAction(quota, async () => {try{await SettingsState.SaveAsync("storage_quota_bytes", bytes, _pageLifetime.Token); Notify(P("settings.saved"));}finally{quota.Text=(SettingsState.Current.Long("storage_quota_bytes")/(1024L*1024*1024)).ToString();} });
        }
        quota.LostKeyboardFocus += async (_, _) => await SaveQuota(); quota.KeyDown += async (_, e) => { if (e.Key == Key.Enter) { e.Handled = true; await SaveQuota(); } };
        AddSetting(rows, SettingRow(P("settings.storageQuota"), P("settings.storageQuotaDescription"), Ui.Stack(Orientation.Horizontal, 8, quota, Ui.Text("GiB", 12, "Secondary"))));
        if (_oldStorage.ValueKind == JsonValueKind.Object && _oldStorage.String("old_root") != _oldStorage.String("new_root"))
        {
            AddSetting(rows, SettingRow(P("settings.storageOldData"), FormatBytes(_oldStorage.Long("old_size_bytes")), Ui.Stack(Orientation.Horizontal, 8,
                AsyncButton("delete", P("settings.storageDeleteOld"), async () =>
                {
                    var path = _oldStorage.String("old_root");
                    if (!DemoDialog.Confirm(this, P("settings.storageDeleteOld"), path, P("settings.storageDeleteOld"))) return;
                    await NativeBackend.InvokeAsync("delete_old_storage", new { path }, _pageLifetime.Token); _oldStorage = default; BuildStorageRows(rows);
                }), Ui.Button(P("settings.storageKeepOld"), () => { _oldStorage = default; BuildStorageRows(rows); }))));
        }
        if (_storage.ValueKind == JsonValueKind.Undefined) _ = LoadStorage(rows);
    }
    private async Task LoadStorage(StackPanel rows)
    {
        try { _storage = await NativeBackend.InvokeAsync<JsonElement>("get_storage_status", token: _pageLifetime.Token); if (!_pageLifetime.IsCancellationRequested) BuildStorageRows(rows); }
        catch (Exception error) { if (!_pageLifetime.IsCancellationRequested) Notify(error.Message); }
    }
    internal static string FormatBytes(long bytes) => bytes >= 1024L * 1024 * 1024 ? $"{bytes / (1024d * 1024 * 1024):0.#} GiB" : bytes >= 1024 * 1024 ? $"{bytes / (1024d * 1024):0.#} MiB" : bytes >= 1024 ? $"{bytes / 1024d:0.#} KiB" : bytes + " B";
    private void BuildProductionConnections()
    {
        ProductionHooks(); ConnectionsContent.Children.Clear(); ConnectionsContent.Children.Add(new ConnectionsView(this, message => Notify(message)));
    }
}

internal static class AutostartService
{
    private const string Key = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string Name = "TailSync";
    private const string PreviousWpfName = "TailSync.Wpf";
    internal static bool IsEnabled
    {
        get
        {
            if (NativeBackend.IsIsolated) return false;
            using var key = Registry.CurrentUser.OpenSubKey(Key);
            return IsTailSyncCommand(key?.GetValue(Name) as string) || IsTailSyncCommand(key?.GetValue(PreviousWpfName) as string);
        }
    }
    private static string Command => "\"" + Environment.ProcessPath + "\" --background";
    internal static void SetEnabled(bool value)
    {
        if (NativeBackend.IsIsolated) throw new InvalidOperationException("Autostart is disabled for isolated validation");
        using var key = Registry.CurrentUser.CreateSubKey(Key);
        var previous=key.GetValue(Name) as string;
        var previousWpf=key.GetValue(PreviousWpfName) as string;
        if(previous!=null&&!IsTailSyncCommand(previous))throw new IOException("The TailSync startup entry points to a custom command; it was not replaced.");
        try
        {
            if(value)key.SetValue(Name,Command,RegistryValueKind.String);else if(IsTailSyncCommand(previous))key.DeleteValue(Name,false);
            if(IsTailSyncCommand(previousWpf))key.DeleteValue(PreviousWpfName,false);
        }
        catch
        {
            if(previous!=null)key.SetValue(Name,previous,RegistryValueKind.String);else key.DeleteValue(Name,false);
            if(previousWpf!=null)key.SetValue(PreviousWpfName,previousWpf,RegistryValueKind.String);
            throw;
        }
    }
    internal static void MigrateEnabledRegistration(){if(IsEnabled)SetEnabled(true);}
    internal static bool IsTailSyncCommand(string? command)
    {
        if(string.IsNullOrWhiteSpace(command))return false;
        var match=System.Text.RegularExpressions.Regex.Match(command,@"^\s*(?:""(?<exe>[^""]+)""|(?<exe>.+?\.exe))(?:\s|$)",System.Text.RegularExpressions.RegexOptions.IgnoreCase);
        if(!match.Success)return false;
        var path=match.Groups["exe"].Value;
        if(!Path.IsPathFullyQualified(path))return false;
        return Path.GetFileName(path).Equals("tailsync.exe",StringComparison.OrdinalIgnoreCase)||Path.GetFileName(path).Equals("TailSync.Wpf.exe",StringComparison.OrdinalIgnoreCase);
    }
}
