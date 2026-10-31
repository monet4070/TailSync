using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;

namespace TailSync.Wpf;

internal sealed class ConnectionsView : StackPanel
{
    private readonly Window _owner;
    private readonly Action<string> _notify;
    private readonly Func<string, CancellationToken, Task> _saveMode;
    private readonly StackPanel _devices = new(), _remote = new();
    private readonly HashSet<string> _expandedPeers = [];
    private readonly TextBlock _status = Ui.Text("", 12, "Secondary"), _pairState = Ui.Text("", 12, "Secondary"), _inviteState = Ui.Text("", 12, "Secondary"), _linkState = Ui.Text("", 12, "Secondary");
    private readonly TextBox _link = new() { MinHeight = 60, MaxLength = 16384, TextWrapping = TextWrapping.Wrap, AcceptsReturn = false };
    private readonly TextBox _inviteLink = new() { IsReadOnly = true, TextWrapping = TextWrapping.Wrap, MaxHeight = 110 };
    private readonly Button _pairToggle;
    private readonly Expander _remoteExpander;
    private CancellationTokenSource? _lifetime;
    private PairingWindow? _pairWindow;
    private JsonElement _peers, _pairing, _invite;
    private string _peerFingerprint = "", _mode = "", _inspectedLink = "";
    private int _linkGeneration;
    private int _refreshGeneration;
    private bool _busy;
    private bool _syncingModes;
    private bool _selfIdentityExpanded;
    private bool _modeSaving, _refreshingRoutes;
    private string? _requestedMode;
    private readonly TextBlock _modeState = Ui.Text("", 12, "Secondary");
    internal bool ModeSelectionSettled => !_modeSaving;
    private readonly UniformGrid _modeButtons = new() { Columns = 4 };

    internal ConnectionsView(Window owner, Action<string> notify, Func<string, CancellationToken, Task>? saveMode = null)
    {
        _saveMode = saveMode ?? ((mode, token) => SettingsState.SaveAsync("connection_mode", mode, token));
        _owner = owner; _notify = notify; Margin = new(20, 0, 20, 20);
        var heading = Ui.PageHeader(T("settings.connectionsTitle"), Words("管理设备、连接方式与安全配对", "Manage devices, connection modes, and secure pairing.")); heading.Margin = new(0, 2, 0, 20); Children.Add(heading);
        var body = new StackPanel(); Children.Add(Ui.SettingsCard(body));
        var modeButtons = _modeButtons;modeButtons.Tag="tour:connection-mode";_devices.Tag="tour:peers";
        var group = Guid.NewGuid().ToString("N");
        foreach (var (mode, label) in new[] { ("auto", T("settings.modeAuto")), ("lan_only", T("settings.modeLan")), ("iroh_only", T("settings.modeIroh")), ("tailscale_only", "Tailscale") })
        {
            var button = Ui.Name(new RadioButton { Content = label, GroupName = group, Tag = mode, IsChecked = SettingsState.Current.String("connection_mode", "auto") == mode }, label); button.SetResourceReference(StyleProperty, "ConnectionModeButton");
            button.Checked += async (_, _) =>
            {
                if (_syncingModes) return;
                if (_busy || !IsLoaded) { SyncModes(); return; }
                await SelectMode(mode);
            };
            button.IsEnabled = GuidedTour.Active || NativeBackend.NetworkEnabled && NativeBackend.ConnectionModes.Contains(mode);
            modeButtons.Children.Add(button);
        }
        var segment = new ConnectionModeSelector(modeButtons);
        var modeCopy = ConnectionLabel(T("settings.connectionMode"), T("settings.connectionModeDescription"));
        modeCopy.MaxWidth = 210; modeCopy.HorizontalAlignment = HorizontalAlignment.Left;
        // Reserve the feedback line so committing a selection never shifts the page.
        _modeState.MinHeight = 17; _modeState.Margin = new(0, 4, 0, 0);
        var modeAction = Ui.Stack(Orientation.Vertical, 0, segment, _modeState);
        body.Children.Add(Ui.PreferenceRow(modeCopy, modeAction, padding: 18));
        Ui.Divider(body); body.Children.Add(_devices); Ui.Divider(body);
        _pairToggle = ActionButton(T("settings.allowPairing"), async () =>
        {
            if (_pairing.Bool("pairing_enabled"))
            {
                _pairing = await NativeBackend.InvokeAsync<JsonElement>("cancel_pairing", token: Token); _pairWindow?.Complete(); _pairWindow = null;
            }
            // Keep the device list available while waiting. Both devices may
            // open their pairing windows before either chooses a peer.
            else { _pairing = await NativeBackend.InvokeAsync<JsonElement>("enable_pairing", token: Token); }
            ApplyPairing();
        });
        var pairTitle = Ui.Text(T("settings.devicePairing"), 16); pairTitle.FontWeight = FontWeights.SemiBold; _pairState.FontSize = 13;
        var pairingRow=Ui.PreferenceRow(Ui.Stack(Orientation.Vertical, 4, pairTitle, _pairState), _pairToggle, "link", padding: 18);pairingRow.Tag="tour:pairing";body.Children.Add(pairingRow);
        Ui.Divider(body);
        var remoteHeading = ConnectionLabel(T("settings.remotePairing"), T("settings.remotePairingDescription"));
        _remote.Margin = new(46, 0, 16, 16);
        _remote.Children.Add(ActionButton("mail", T("settings.createRemoteInvite"), async () =>
        {
            _invite = await NativeBackend.InvokeAsync<JsonElement>("create_remote_pairing_invite", token: Token); _inviteLink.Text = _invite.String("link"); await UpdateInvite();
        }));
        _inviteLink.Margin = new(0, 8, 0, 6); _remote.Children.Add(_inviteLink); _remote.Children.Add(_inviteState);
        var copy = ActionButton("copy", Words("复制邀请", "Copy invite"), () => { if (_inviteLink.Text.Length > 0) { GuidedTour.CheckClipboard();Clipboard.SetText(_inviteLink.Text); _notify(Words("邀请已复制", "Invite copied")); } return Task.CompletedTask; });
        var cancel = ActionButton("close", T("settings.cancelRemoteInvite"), async () => { await NativeBackend.InvokeAsync("cancel_remote_pairing_invite", token: Token); _invite = default; _inviteLink.Clear(); await UpdateInvite(); });
        _remote.Children.Add(Ui.Stack(Orientation.Horizontal, 8, copy, cancel));
        var use = Ui.Text(T("settings.useRemoteInvite"), 14); use.Margin = new(0, 18, 0, 4); _remote.Children.Add(use);
        _remote.Children.Add(Ui.Text(T("settings.useRemoteInviteDescription"), 12, "Secondary"));
        _link.Tag = T("settings.remoteInvitePlaceholder"); _link.Margin = new(0, 8, 0, 8); _remote.Children.Add(_link);
        _link.TextChanged += (_, _) => { _inspectedLink = ""; _linkGeneration++; _linkState.Text = ""; };
        _remote.Children.Add(_linkState);
        _remote.Children.Add(Ui.Stack(Orientation.Horizontal, 8,
            ActionButton("search", T("settings.checkInvite"), Inspect),
            ActionButton("link", T("settings.startRemotePairing"), async () =>
            {
                if (_inspectedLink != _link.Text.Trim()) await Inspect();
                if (_inspectedLink.Length == 0) return;
                await StartPairing(() => NativeBackend.InvokeAsync<JsonElement>("start_remote_pairing", new { link = _inspectedLink }, Token));
            })));
        var remoteHeader = new Grid { Margin = new(16, 18, 16, 18) };
        remoteHeader.ColumnDefinitions.Add(new() { Width = new GridLength(30) }); remoteHeader.ColumnDefinitions.Add(new());
        var globe = new VectorIcon { Kind = "globe", Width = 18, Height = 18, HorizontalAlignment = HorizontalAlignment.Left }; globe.SetResourceReference(VectorIcon.StrokeProperty, "AccentBrush");
        remoteHeader.Children.Add(globe); Grid.SetColumn(remoteHeading, 1); remoteHeader.Children.Add(remoteHeading);
        _remoteExpander=new Expander{Header=remoteHeader,Content=_remote,IsExpanded=false};
        _remoteExpander.SetResourceReference(StyleProperty, "PreferenceDisclosure");
        _remoteExpander.Tag="tour:remote-pairing";body.Children.Add(_remoteExpander);
        _status.Margin = new(20, 8, 20, 0); Children.Add(_status);
        Loaded += async (_, _) =>
        {
            if(_lifetime!=null)return;
            var lifetime=new CancellationTokenSource();_lifetime=lifetime;var token=lifetime.Token;
            _mode = SettingsState.Current.String("connection_mode", "auto");
            Localizer.Changed += LocaleChanged;
            SettingsState.Changed += SettingsChanged;
            SyncModes();
            try { await Refresh(false); await TakeLink(); }
            catch (OperationCanceledException) when (token.IsCancellationRequested) { }
            catch (Exception error) { _status.Text = error.Message; }
            if(!token.IsCancellationRequested&&IsLoaded&&ReferenceEquals(_lifetime,lifetime))_ = Poll(token);
        };
        Unloaded += (_, _) => { _lifetime?.Cancel(); _lifetime?.Dispose(); _lifetime = null; Localizer.Changed -= LocaleChanged; SettingsState.Changed -= SettingsChanged; _pairWindow?.Complete(); _pairWindow = null; };
    }
    private CancellationToken Token => _lifetime?.Token ?? new CancellationToken(true);
    private static string T(string key) => Localizer.Text(key);
    private static string Words(string chinese, string english) => Localizer.Choose(chinese, english);
    private static StackPanel ConnectionLabel(string title, string description)
    {
        var name = Ui.Text(title, 16); name.FontWeight = FontWeights.SemiBold;
        return Ui.Stack(Orientation.Vertical, 4, name, Ui.Text(description, 13, "Secondary"));
    }
    private async Task SelectMode(string mode)
    {
        if(GuidedTour.Active){_mode=mode;_syncingModes=true;foreach(RadioButton b in _modeButtons.Children)b.IsChecked=(string)b.Tag==mode;_syncingModes=false;_remoteExpander.IsEnabled=mode is "auto" or "iroh_only";_modeState.Text=GuidedTour.Copy("demo");return;}
        _requestedMode = mode;
        if (_modeSaving) return;
        _modeSaving = true; _refreshingRoutes = true; ++_refreshGeneration;
        _modeState.Text = Words("正在切换…", "Switching…");
        _peerFingerprint = ""; RenderDevices(); SyncModes();
        // Give the radio feedback a render turn before any IPC continuation.
        await Dispatcher.Yield(DispatcherPriority.Render);
        try
        {
            while (_requestedMode is { } requested && requested != SettingsState.Current.String("connection_mode"))
                await _saveMode(requested, Token);
            _modeState.Text = "";
        }
        catch (OperationCanceledException) when (Token.IsCancellationRequested) { }
        catch (Exception error) { _modeState.Text = Words("切换失败，已恢复", "Change failed; restored"); _notify(error.Message); }
        finally
        {
            _modeSaving = false; _requestedMode = null; SyncModes();
            // Reading the next snapshot may wait on discovery; it must not
            // hold the selection or delay a subsequent user's choice.
            if (!Token.IsCancellationRequested) _ = RefreshAfterModeChange();
        }
    }
    private async Task RefreshAfterModeChange()
    {
        try { await Refresh(false); }
        catch (OperationCanceledException) when (Token.IsCancellationRequested) { }
        catch (Exception error) { _status.Text = error.Message; }
    }
    private Button ActionButton(string icon, string text, Func<Task> action)
    {
        var button = ActionButton(text, action); ButtonDesign.SetIcon(button, icon); return button;
    }
    private Button ActionButton(string text, Func<Task> action)
    {
        var button = Ui.Button(text, style: text == T("settings.startRemotePairing") || text == T("settings.pair") ? "PrimaryButton" : "ConnectionAction"); button.IsEnabled = GuidedTour.Active||NativeBackend.NetworkEnabled;
        if (!button.IsEnabled) button.ToolTip = Words("隔离预览未启用网络，请打开正式运行版本。", "Networking is disabled in this isolated preview. Open the production application.");
        button.Click += async (_, _) => await Run(button, action); return button;
    }
    private async Task Run(FrameworkElement element, Func<Task> action)
    {
        if(GuidedTour.Active){if(ReferenceEquals(element,_pairToggle))ShowDemoCode();else _notify(GuidedTour.Copy("demoPaired"));return;}
        if (_busy || _modeSaving) { _notify(Words("正在处理当前连接操作，请稍候。", "A connection operation is in progress. Please wait.")); return; }
        _busy = true; ++_refreshGeneration; element.IsEnabled = false; ControlMotion.Busy(element, true);
        try { await action(); _status.Text = ""; }
        catch (OperationCanceledException) when (Token.IsCancellationRequested) { }
        catch (Exception error) { _status.Text = error.Message; _pairWindow?.ReportError(error.Message); _notify(error.Message); }
        finally { _busy = false; ControlMotion.Busy(element, false); element.IsEnabled = true; }
    }
    private void SyncModes()
    {
        _syncingModes = true;
        try
        {
            _mode = _requestedMode ?? SettingsState.Current.String("connection_mode", "auto");
            foreach (RadioButton button in _modeButtons.Children) button.IsChecked = (string)button.Tag == _mode;
            _remoteExpander.IsEnabled = !_modeSaving && _mode is "auto" or "iroh_only";
            _remoteExpander.ToolTip = _mode is "auto" or "iroh_only" ? null : Words("切换到自动或 Iroh 模式后可用", "Available in Auto or Iroh mode");
        }
        finally { _syncingModes = false; }
    }
    private void SettingsChanged(object? sender, EventArgs e) { SyncModes(); }
    private void LocaleChanged(object? sender, EventArgs e) { _peerFingerprint = ""; RenderDevices(); }
    private async Task Poll(CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            try
            {
                await Task.Delay(1000, token);
                if (_busy || _modeSaving || !IsVisible || !NativeBackend.IsConnected) continue;
                await Refresh(false); await TakeLink();
            }
            catch (OperationCanceledException) when (token.IsCancellationRequested) { return; }
            catch (Exception error) { if (!token.IsCancellationRequested) _status.Text = error.Message; }
        }
    }
    private async Task Refresh(bool discover)
    {
        if(GuidedTour.Active){RenderDevices();_pairState.Text=GuidedTour.Copy("demo");return;}
        var generation=++_refreshGeneration;
        var peers = await NativeBackend.InvokeAsync<JsonElement>(discover ? "refresh_peers" : "get_peers", token: Token);
        var pairing = await NativeBackend.InvokeAsync<JsonElement>("get_pairing_status", token: Token);
        if (Token.IsCancellationRequested||generation!=_refreshGeneration) return;
        _peers = peers; _pairing = pairing; _refreshingRoutes = false;
        RenderDevices(); ApplyPairing();
        if (_remoteExpander.IsEnabled && _remoteExpander.IsExpanded) await UpdateInvite();
    }
    private void ShowDemoCode()=>MessageBox.Show(_owner,"123456\n\n"+GuidedTour.Copy("demoCode")+"\n\n"+GuidedTour.Copy("demoPaired"),GuidedTour.Copy("demoPairingTitle"),MessageBoxButton.OK,MessageBoxImage.Information);
    private void RenderDevices()
    {
        if(GuidedTour.Active)
        {
            _devices.Children.Clear();
            var pair=Ui.Button(GuidedTour.Copy("demoPair"),ShowDemoCode);
            _devices.Children.Add(Ui.PreferenceRow(Ui.Stack(Orientation.Vertical,4,Ui.Text("TailSync Demo · "+GuidedTour.Copy("demo"),15),Ui.Text(Words("虚拟设备 · 不会连接网络","Virtual device · No network connection"),12,"Secondary")),pair,padding:18));return;
        }
        if (_peers.ValueKind != JsonValueKind.Object) return;
        var fingerprint = _peers.GetRawText() + Localizer.Locale + _refreshingRoutes;
        if (_peerFingerprint == fingerprint) return;
        _peerFingerprint = fingerprint; _devices.Children.Clear();
        var self = _peers.Object("self");
        var localName = Ui.Text(self.String("hostname"), 16); localName.FontWeight = FontWeights.SemiBold;
        var badge = new Border { Child = Ui.Text(T("settings.thisDevice"), 12, "Accent"), Padding = new(5, 2, 5, 2), CornerRadius = new(4), Margin = new(8, 0, 0, 0) };
        badge.SetResourceReference(Border.BackgroundProperty, "AccentSoftBrush");
        var localTitle = new WrapPanel(); localTitle.Children.Add(localName); localTitle.Children.Add(badge);
        var identity = Ui.Stack(Orientation.Vertical, 4, localTitle);
        var localRoutes = self.Object("routes").Items().Select(route => route.String("address")).Where(address => address.Length > 0).ToArray();
        identity.Children.Add(Ui.Text(localRoutes.FirstOrDefault() ?? Words("已准备好连接其他设备", "Ready to connect to your devices"), 13, "Secondary"));
        var identityDetails = new StackPanel { Margin = new(0, 6, 0, 0) }; identityDetails.Children.Add(Address(self.String("fingerprint")));
        if (self.String("iroh_endpoint_id") is { Length: > 0 } endpoint)
        {
            var endpointLabel = Address("iroh:\n" + endpoint); endpointLabel.TextWrapping = TextWrapping.Wrap; endpointLabel.TextTrimming = TextTrimming.None; identityDetails.Children.Add(endpointLabel);
        }
        foreach (var route in localRoutes) identityDetails.Children.Add(Address(route));
        var identityDisclosure = new Expander { Header = Ui.Text(Words("设备标识", "Device identity"), 12, "Secondary"), Content = identityDetails, IsExpanded = _selfIdentityExpanded };
        identityDisclosure.SetResourceReference(StyleProperty, "InlineDisclosure"); identityDisclosure.Expanded += (_, _) => _selfIdentityExpanded = true; identityDisclosure.Collapsed += (_, _) => _selfIdentityExpanded = false; identity.Children.Add(identityDisclosure);
        _devices.Children.Add(Ui.PreferenceRow(identity, new Border(), "laptop", padding: 18));
        Ui.Divider(_devices);
        var peers = _peers.Object("peers").Items().ToArray();
        var refresh = ActionButton("refresh", T("settings.refreshDevices"), async () => await Refresh(true));
        refresh.Content = null; refresh.SetResourceReference(StyleProperty, "IconButton"); refresh.ToolTip = T("settings.refreshDevices");
        _devices.Children.Add(Ui.PreferenceRow(Ui.Text(Words($"{peers.Length} 台设备", $"{peers.Length} devices"), 13, "Secondary"), refresh, padding: 9));
        foreach (var peer in peers)
        {
            var hostname = peer.String("hostname"); var trusted = peer.Bool("trusted");
            var online = peer.Bool("online") || peer.String("current_interface").Length > 0;
            var name = Ui.Text(hostname, 16); name.FontWeight = FontWeights.SemiBold;
            var title = new WrapPanel(); title.Children.Add(name);
            var trust = Ui.Text(T(trusted ? "settings.paired" : "settings.notPaired"), 12, trusted ? "Positive" : "Secondary");
            var trustMark = new VectorIcon { Kind = "shield", Width = 12, Height = 12 }; trustMark.SetResourceReference(VectorIcon.StrokeProperty, trusted ? "PositiveBrush" : "SecondaryBrush");
            var trustBadge = Ui.Stack(Orientation.Horizontal, 5, trustMark, trust); trustBadge.Margin = new(10, 2, 0, 0); title.Children.Add(trustBadge);
            var copy = new StackPanel();
            var routes = peer.Object("routes").Items().ToArray();
            if (routes.Length == 0 && PairingAddress(peer).Length > 0)
                routes = [JsonSerializer.SerializeToElement(new { address = PairingAddress(peer), @interface = peer.String("current_interface", "lan"), connected = online, online })];
            var active = routes.FirstOrDefault(r => r.Bool("connected"));
            if (active.ValueKind != JsonValueKind.Object) active = routes.FirstOrDefault();
            var kind = active.String("interface");
            var routeLine = new WrapPanel();
            if (kind.Length > 0) { var routeName = Ui.Text(kind == "iroh" ? "Iroh ·" : kind == "lan" ? "LAN ·" : "Tailscale ·", 13, "Secondary"); routeName.Margin = new(0, 0, 6, 0); routeLine.Children.Add(routeName); }
            routeLine.Children.Add(Ui.Text(_refreshingRoutes ? Words("正在更新连接…", "Updating routes…") : T(!trusted ? "settings.syncNeedsPairing" : !peer.Bool("enabled") ? "settings.syncPeerPaused" : online ? "settings.connected" : "settings.pairedWaiting"), 13, !_refreshingRoutes && online && trusted && peer.Bool("enabled") ? "Positive" : "Secondary"));
            if (!_refreshingRoutes && active.Object("latency_ms").ValueKind == JsonValueKind.Number) { var latency = Ui.Text(active.Long("latency_ms") + " ms", 12, "Secondary"); latency.Margin = new(8, 0, 0, 0); routeLine.Children.Add(latency); }
            copy.Children.Add(routeLine);
            if (peer.Long("required_protocol_version") > 0) copy.Children.Add(Ui.Text(T("settings.protocolUpgradeRequired").Replace("{version}", peer.Long("required_protocol_version").ToString()), 12, "Accent"));
            var details = new StackPanel { Margin = new(16, 0, 16, 12), Visibility = _expandedPeers.Contains(hostname) ? Visibility.Visible : Visibility.Collapsed };
            var fingerprintLabel = Address(peer.String("fingerprint")); fingerprintLabel.Margin = new(30, 0, 0, 8); details.Children.Add(fingerprintLabel);
            foreach (var route in routes) details.Children.Add(RouteRow(route, peer));
            var inspect = Ui.IconButton("latency", Words("连接详情与延迟", "Connection details and latency"), () =>
            {
                var expanded = details.Visibility != Visibility.Visible;
                ControlMotion.AnimateDisclosure(details, expanded);
                if (expanded) _expandedPeers.Add(hostname); else _expandedPeers.Remove(hostname);
            });
            var actions = Ui.Stack(Orientation.Horizontal, 4, inspect);
            if (trusted)
            {
                var forget = ActionButton("delete", T("settings.forgetPairing"), async () =>
                {
                    if (!DemoDialog.Confirm(_owner, T("settings.forgetPairing"), hostname + "\n" + peer.String("fingerprint"), T("settings.forgetPairing"))) return;
                    await NativeBackend.InvokeAsync("forget_peer", new { hostname }, Token); await Refresh(false);
                }); forget.Content = null; forget.SetResourceReference(StyleProperty, "DestructiveIconButton"); forget.ToolTip = T("settings.forgetPairing"); actions.Children.Add(forget);
                var toggle = Ui.Name(new ToggleButton { IsChecked = peer.Bool("enabled") }, T("settings.enableSync") + " " + hostname); toggle.SetResourceReference(StyleProperty, "MacSwitch"); toggle.Margin = new(8, 0, 0, 0);
                toggle.Click += async (_, _) =>
                {
                    if (_busy || _modeSaving) { toggle.IsChecked = peer.Bool("enabled"); _notify(Words("正在处理当前连接操作，请稍候。", "A connection operation is in progress. Please wait.")); return; }
                    await Run(toggle, async () => { try { await NativeBackend.InvokeAsync("toggle_peer", new { hostname, enabled = toggle.IsChecked == true }, Token); await Refresh(false); } catch { toggle.IsChecked = peer.Bool("enabled"); throw; } });
                };
                actions.Children.Add(toggle);
            }
            else
            {
                var pair = ActionButton("link", T("settings.pair"), async () =>
                {
                    await StartPairing(() => NativeBackend.InvokeAsync<JsonElement>("start_pairing", new { address = PairingAddress(peer) }, Token));
                }); pair.IsEnabled = PairingAddress(peer).Length > 0; actions.Children.Add(pair);
            }
            var device = new Grid { Margin = new(16, 14, 16, 18) };
            device.ColumnDefinitions.Add(new() { Width = new GridLength(30) }); device.ColumnDefinitions.Add(new());
            var dot = new System.Windows.Shapes.Ellipse { Width = 8, Height = 8, HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Center };
            dot.SetResourceReference(System.Windows.Shapes.Shape.FillProperty, !_refreshingRoutes && online && trusted && peer.Bool("enabled") ? "PositiveBrush" : "TertiaryBrush"); device.Children.Add(dot);
            var metadata = new AdaptiveSettingRow(); metadata.Children.Add(copy); Grid.SetColumn(actions, 1); metadata.Children.Add(actions);
            var content = Ui.Stack(Orientation.Vertical, 5, title, metadata); Grid.SetColumn(content, 1); device.Children.Add(content);
            _devices.Children.Add(device); _devices.Children.Add(details);
        }
        if (peers.Length == 0) { var empty = Ui.Text(T("settings.noDevices"), 12, "Secondary"); empty.Margin = new(16, 8, 16, 18); _devices.Children.Add(empty); }
        if (_peers.String("discovery_error") is { Length: > 0 } error)
        {
            var warning = Ui.Text(NativeBackend.IsIsolated ? Words("预览模式 · 未启用设备发现", "Preview mode · device discovery disabled") : error, 12, NativeBackend.IsIsolated ? "Secondary" : "Accent");
            warning.ToolTip = error; warning.Margin = new(16, 0, 16, 12); _devices.Children.Add(warning);
        }
    }
    internal void RenderFixtureForChecks(JsonElement peers)
    {
        if (!NativeBackend.IsIsolated) throw new InvalidOperationException("Device fixtures require an isolated backend.");
        _peers = peers; _peerFingerprint = ""; RenderDevices();
    }
    private static TextBlock Address(string value)
    {
        var label = Ui.Text(value, 12, "Secondary"); label.TextWrapping = TextWrapping.NoWrap; label.TextTrimming = TextTrimming.CharacterEllipsis; label.ToolTip = value; label.FontFamily = new FontFamily("Cascadia Mono, Consolas"); return label;
    }
    private FrameworkElement RouteRow(JsonElement route, JsonElement? peer)
    {
        var address = route.String("address"); var kind = route.String("interface");
        var label = Address(address);
        var state = Ui.Text((kind == "iroh" ? "Iroh" : kind == "lan" ? "LAN" : "Tailscale") + " · " + T(route.Bool("connected") ? "settings.connected" : route.Bool("online") ? "settings.online" : "settings.offline"), 11, "Secondary");
        var test = ActionButton("latency", Words("测延迟", "Test latency"), async () =>
        {
            var result = await NativeBackend.InvokeAsync<JsonElement>("test_connection", new { address }, Token);
            state.Text = result.Object("latency_ms").ToString() + " ms" + (result.String("path") == "relay" ? " · " + T("settings.relayPath") : "");
        });
        test.FontSize = 12; test.IsEnabled = kind != "iroh" || route.Bool("rtt_capable"); test.ToolTip = test.IsEnabled ? address : T("settings.testRouteRediscover");
        return Ui.PreferenceRow(Ui.Stack(Orientation.Vertical, 3, label, state), test, padding: 6);
    }
    internal static string PairingAddress(JsonElement peer)
    {
        var mode = SettingsState.Current.String("connection_mode", "auto");
        var routes = peer.Object("routes").Items().Where(r => r.String("address").Trim().Length > 0
            && (mode == "auto" || mode == "lan_only" && r.String("interface") == "lan"
                || mode == "iroh_only" && r.String("interface") == "iroh" || mode == "tailscale_only" && r.String("interface") == "tailscale")).ToArray();
        var tcp = routes.Where(r => r.String("interface") != "iroh").ToArray();
        foreach (var route in new[] {
            tcp.FirstOrDefault(r=>r.Bool("connected")), tcp.FirstOrDefault(r=>r.Bool("online")), tcp.FirstOrDefault(r=>r.String("status")=="confirming"), routes.FirstOrDefault(r=>r.String("interface")=="iroh"), tcp.FirstOrDefault(r=>r.String("address")==peer.String("current_address")), tcp.FirstOrDefault(r=>r.String("address")==peer.String("address")), tcp.FirstOrDefault(), routes.FirstOrDefault() })
            if (route.String("address").Length > 0) return route.String("address");
        if (mode != "auto" && peer.Object("routes").Items().Any()) return "";
        var fallback = peer.String("address", peer.String("tailscale_ip")).Trim();
        return mode == "iroh_only" && (fallback.Length != 64 || !fallback.All(Uri.IsHexDigit)) ? "" : fallback;
    }
    private async Task StartPairing(Func<Task<JsonElement>> start)
    {
        if (!_pairing.Bool("pairing_enabled")) _pairing = await NativeBackend.InvokeAsync<JsonElement>("enable_pairing", token: Token);
        OpenPairWindow(); var window = _pairWindow!;
        try { _pairing = await start(); if (!window.Completed) ApplyPairing(); }
        catch (Exception) when (window.Completed) { /* A cancelled dialog supersedes the pending handshake. */ }
    }
    private void ApplyPairing()
    {
        _pairState.Text = _pairing.Bool("pairing_enabled") ? T("settings.waiting") + " · " + _pairing.Long("remaining_seconds") + "s · " + _pairing.Long("failed_attempts") + "/" + _pairing.Long("max_failures") : T("settings.pairingClosed");
        _pairToggle.Content = T(_pairing.Bool("pairing_enabled") ? "settings.closePairing" : "settings.allowPairing");
        _pairToggle.SetResourceReference(StyleProperty, _pairing.Bool("pairing_enabled") ? "ConnectionAction" : "PrimaryButton");
        var phase = _pairing.String("phase");
        if (_pairing.Object("peer").ValueKind == JsonValueKind.Object && phase is "verification" or "waiting_for_peer" or "finalizing") OpenPairWindow();
        if (phase == "paired") { _pairWindow?.Complete(); _pairWindow = null; _peerFingerprint = ""; }
        else _pairWindow?.Apply(_pairing);
    }
    private void OpenPairWindow()
    {
        if (_pairWindow != null) { _pairWindow.Apply(_pairing); return; }
        _pairWindow = new PairingWindow(_owner); var current = _pairWindow;
        current.Closed += (_, _) => { if (_pairWindow == current) _pairWindow = null; };
        current.Apply(_pairing); current.Show();
    }
    private async Task Inspect()
    {
        var link = _link.Text.Trim(); var generation = ++_linkGeneration;
        if (link.Length == 0) return;
        var preview = await NativeBackend.InvokeAsync<JsonElement>("inspect_remote_pairing_link", new { link }, Token);
        if (generation != _linkGeneration || link != _link.Text.Trim()) return;
        _inspectedLink = link;
        _linkState.Text = preview.String("endpoint_id") + "\n" + preview.Long("remaining_seconds") + "s";
    }
    private async Task TakeLink()
    {
        if(GuidedTour.Active)return;
        var pending = await NativeBackend.InvokeAsync<JsonElement>("take_pending_remote_pairing_link", token: Token);
        if (pending.ValueKind != JsonValueKind.String || pending.GetString() is not { Length: > 0 } link) return;
        _remoteExpander.IsExpanded = true; _link.Text = link; await Inspect();
    }
    private async Task UpdateInvite()
    {
        var state = await NativeBackend.InvokeAsync<JsonElement>("get_remote_pairing_invite_status", token: Token);
        _inviteState.Text = state.Bool("active") ? state.Long("remaining_seconds") + "s · " + state.String("state") : Words("邀请已关闭或过期", "Invite closed or expired");
        if (!state.Bool("active")) { _invite = default; _inviteLink.Clear(); }
    }
}
