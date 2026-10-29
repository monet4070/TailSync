using System.Text.Json;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using TailSync.Native.Core;
using Windows.System;
using Windows.UI.Core;
using Microsoft.UI.Input;

namespace TailSync.Native;

// Presentation-only port of the Mac Settings/Connections structure. All actions
// remain local to the demo client until the Rust adapter is implemented.
public sealed partial class MainWindow
{
    private ToggleButton SyncSwitch = null!;
    private static string T(string key) => MacDesign.Text(key);
    private Brush Ink(string key)
    {
        var dictionaries = Application.Current.Resources.ThemeDictionaries;
        var theme = Root.ActualTheme == ElementTheme.Dark ? "Dark" : "Light";
        return (Brush)((ResourceDictionary)dictionaries[theme])[key];
    }
    private TextBlock Label(string text, double size = 13, string color = "PrimaryBrush") => new()
    {
        Text = text, FontSize = size, Foreground = Ink(color), TextWrapping = TextWrapping.Wrap,
        VerticalAlignment = VerticalAlignment.Center
    };
    private FontIcon Symbol(string glyph, double size = 14) => new() { Glyph = glyph, FontSize = size, Foreground = Ink("SecondaryBrush") };
    private Button SmallButton(string text, Action? click = null)
    {
        var b = new Button { Content = text, FontSize = 11, Padding = new(8, 3, 8, 3), MinHeight = 23,
            Background = Ink("SurfaceBrush"), BorderBrush = Ink("BorderBrush"), BorderThickness = new(1), CornerRadius = new(5), VerticalAlignment = VerticalAlignment.Center };
        if (click != null) b.Click += (_, _) => click();
        return b;
    }
    private ToggleButton Switch(string label, bool value)
    {
        var control = new ToggleButton { IsChecked = value, Style = (Style)Application.Current.Resources["MacSwitch"], VerticalAlignment = VerticalAlignment.Center };
        AutomationProperties.SetName(control, label);
        return control;
    }
    private Grid SettingRow(string title, string description, UIElement? control = null, double padding = 13)
    {
        var row = new Grid { Padding = new(16, padding, 16, padding), ColumnSpacing = 8, MinHeight = 36 };
        row.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        var copy = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        copy.Children.Add(Label(title));
        if (!string.IsNullOrEmpty(description)) copy.Children.Add(Label(description, 10, "TertiaryBrush"));
        row.Children.Add(copy);
        if (control != null) { Grid.SetColumn(control as FrameworkElement, 1); row.Children.Add(control); }
        return row;
    }
    private void Divider(StackPanel stack) => stack.Children.Add(new Border { Height = 1, Background = Ink("DividerBrush"), Margin = new(16, 0, 0, 0) });
    private StackPanel Section(StackPanel parent, string title)
    {
        var section = new StackPanel();
        section.Children.Add(new TextBlock { Text = title, Style = (Style)Application.Current.Resources["SectionHeading"] });
        var rows = new StackPanel();
        section.Children.Add(new Border { Child = rows, Style = (Style)Application.Current.Resources["SettingsCard"], Margin = new(12, 0, 12, 0) });
        parent.Children.Add(section);
        return rows;
    }
    private StackPanel Segments(IEnumerable<(string Value, string Text)> items, string value, double width, Action<string> change)
    {
        var panel = new StackPanel { Orientation = Orientation.Horizontal, Width = width, Spacing = 1, Padding = new(2), Background = Ink("InputBrush"), CornerRadius = new(6), VerticalAlignment = VerticalAlignment.Center };
        var options = items.ToArray();
        foreach (var (v, text) in options)
        {
            var b = SmallButton(text); b.Tag = v; b.Width = (width - 4 - options.Length + 1) / options.Length; b.BorderThickness = new(0); b.Padding = new(1); b.MinHeight = 21;
            b.Background = v == value ? Ink("SurfaceBrush") : new SolidColorBrush(Colors.Transparent);
            b.Click += (_, _) => { foreach (var other in panel.Children.OfType<Button>()) other.Background = other == b ? Ink("SurfaceBrush") : new SolidColorBrush(Colors.Transparent); change(v); };
            panel.Children.Add(b);
        }
        return panel;
    }
    private UIElement Shortcut(JsonElement spec)
    {
        var group = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
        var keys = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 3 };
        keys.Children.Add(Symbol("\uE765", 11));
        void SetKeys(IEnumerable<string> tokens)
        {
            while (keys.Children.Count > 1) keys.Children.RemoveAt(1);
            foreach (var token in tokens) keys.Children.Add(new Border { CornerRadius = new(4), BorderBrush = Ink("BorderBrush"), BorderThickness = new(1), Background = Ink("SurfaceBrush"), Padding = new(6, 3, 6, 3), Child = Label(token, 11) });
            keys.Children.Add(Symbol("\uE70F", 11));
        }
        SetKeys(spec.GetProperty("keys").EnumerateArray().Select(x => x.GetString()!));
        var recorder = SmallButton(""); recorder.Content = keys; recorder.CornerRadius = new(6); recorder.Padding = new(8, 4, 8, 4);
        var recording = false; var original = spec.GetProperty("keys").EnumerateArray().Select(x => x.GetString()!).ToArray();
        recorder.Click += (_, _) => { recording = true; SetKeys([T("settings.shortcutRecording")]); recorder.Focus(FocusState.Programmatic); };
        recorder.KeyDown += (_, e) =>
        {
            if (!recording || e.Key is VirtualKey.Menu or VirtualKey.Control or VirtualKey.Shift or VirtualKey.LeftWindows or VirtualKey.RightWindows) return;
            e.Handled = true; recording = false;
            if (e.Key == VirtualKey.Escape) { SetKeys(original); return; }
            var tokens = new List<string>();
            foreach (var (key, token) in new[] { (VirtualKey.Control, "⌃"), (VirtualKey.Menu, "⌥"), (VirtualKey.Shift, "⇧"), (VirtualKey.LeftWindows, "⌘") })
                if ((InputKeyboardSource.GetKeyStateForCurrentThread(key) & CoreVirtualKeyStates.Down) != 0) tokens.Add(token);
            tokens.Add(e.Key.ToString()); original = tokens.ToArray(); SetKeys(original);
        };
        recorder.LostFocus += (_, _) => { if (recording) { recording = false; SetKeys(original); } };
        group.Children.Add(recorder);
        var reset = SmallButton("↶", () => SetKeys(["⇧", "⌘", spec.GetProperty("id").GetString() == "syncShortcut" ? "S" : "H"]));
        var clear = SmallButton("×", () => SetKeys([T("settings.shortcutNone")]));
        foreach (var b in new[] { reset, clear }) { b.Width = b.Height = 24; b.MinWidth = b.MinHeight = 24; b.Padding = new(0); group.Children.Add(b); }
        return group;
    }
    private void BuildMacPages()
    {
        SettingsContent.Children.Clear(); ConnectionsContent.Children.Clear();
        var guide = SmallButton(""); guide.HorizontalAlignment = HorizontalAlignment.Stretch; guide.HorizontalContentAlignment = HorizontalAlignment.Stretch;
        guide.Padding = new(16); guide.Margin = new(16, 0, 16, 0); guide.CornerRadius = new(10);
        var guideCopy = new StackPanel { Spacing = 4 }; guideCopy.Children.Add(Label(T("onboarding.title"))); guideCopy.Children.Add(Label(T("onboarding.reopen"), 10, "SecondaryBrush"));
        var guideGrid = new Grid(); guideGrid.Children.Add(guideCopy); guideGrid.Children.Add(new TextBlock { Text = "↗", FontSize = 20, HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Center }); guide.Content = guideGrid; guide.Click += Guide_Click; SettingsContent.Children.Add(guide);
        foreach (var section in MacDesign.Root.GetProperty("sections").EnumerateArray())
        {
            var rows = Section(SettingsContent, T("settings." + section.GetProperty("id").GetString()));
            foreach (var spec in section.GetProperty("rows").EnumerateArray()) { if (rows.Children.Count > 0) Divider(rows); rows.Children.Add(BuildSetting(spec)); }
        }
        BuildConnections();
    }
    private UIElement BuildSetting(JsonElement spec)
    {
        var id = spec.GetProperty("id").GetString()!; var type = spec.GetProperty("type").GetString();
        var title = T(spec.GetProperty("title").GetString()!); var description = T(spec.GetProperty("description").GetString()!);
        UIElement? control = null;
        switch (type)
        {
            case "switch":
                var toggle = Switch(title, spec.TryGetProperty("value", out var v) && v.GetBoolean());
                if (id == "syncEnabled") { SyncSwitch = toggle; toggle.Click += Sync_Toggled; }
                control = toggle; break;
            case "shortcut": control = Shortcut(spec); break;
            case "update":
                var update = SettingRow(T("settings.currentVersion") + ": 0.1.0", T("settings.updateReady"), SmallButton(T("settings.updateCheck"), () => Notify(new(true, "更新界面演示；未访问更新服务"))));
                return update;
            case "slider":
                var limit = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
                var slider = new Slider { Minimum = 10, Maximum = 500, StepFrequency = 10, Value = 100, Width = 180, MinHeight = 28, VerticalAlignment = VerticalAlignment.Center };
                AutomationProperties.SetName(slider, title);
                var number = Label("100", 12, "AccentBrush");
                limit.Children.Add(slider); limit.Children.Add(new Border { Width = 46, Height = 26, Background = Ink("AccentSoftBrush"), CornerRadius = new(9), Child = number }); number.HorizontalAlignment = HorizontalAlignment.Center;
                slider.ValueChanged += (_, e) => number.Text = ((int)e.NewValue).ToString();
                description = description.Replace("{value}", "100"); control = limit; break;
            case "disclosure":
                var content = new StackPanel { Margin = new(16, 0, 16, 10), Visibility = Visibility.Collapsed };
                foreach (var child in spec.GetProperty("rows").EnumerateArray()) { if (content.Children.Count > 0) Divider(content); content.Children.Add(BuildSetting(child)); }
                var disclosure = SmallButton(""); disclosure.HorizontalAlignment = HorizontalAlignment.Stretch; disclosure.HorizontalContentAlignment = HorizontalAlignment.Stretch; disclosure.Background = new SolidColorBrush(Colors.Transparent); disclosure.BorderThickness = new(0); disclosure.Padding = new(0); disclosure.Content = SettingRow("›  " + title, description, null, 10);
                disclosure.Click += (_, _) => content.Visibility = content.Visibility == Visibility.Visible ? Visibility.Collapsed : Visibility.Visible;
                var disclosureRows = new StackPanel(); disclosureRows.Children.Add(disclosure); disclosureRows.Children.Add(content); return disclosureRows;
            case "automatic": control = Label(T("settings.imageAccessAutomatic"), 11, "SecondaryBrush"); break;
            case "storage": return SettingRow("~/Library/Application Support/TailSync", "125 MB / 10 GB", SmallButton(T("settings.storageChange"), () => Notify(new(true, "存储界面演示；未迁移真实文件"))));
            case "stepper":
                var quota = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 }; var value = 10; var quotaLabel = Label("10 GiB", 12);
                var arrows = new StackPanel(); var inc = SmallButton("⌃", () => { value = Math.Min(16384, value + 1); quotaLabel.Text = value + " GiB"; }); var dec = SmallButton("⌄", () => { value = Math.Max(1, value - 1); quotaLabel.Text = value + " GiB"; });
                foreach (var b in new[] { inc, dec }) { b.Width = 20; b.MinWidth = 20; b.Height = b.MinHeight = 12; b.Padding = new(0); arrows.Children.Add(b); } quota.Children.Add(quotaLabel); quota.Children.Add(arrows); control = quota; break;
            case "connections": control = SmallButton(T("settings.openConnections"), () => Open("connections")); break;
            case "appearance":
                control = Segments([("Default", T("settings.themeSystem")), ("Light", T("settings.themeLight")), ("Dark", T("settings.themeDark"))], Root.RequestedTheme.ToString(), 190, value => { if (Enum.TryParse<ElementTheme>(value, out var theme)) foreach (var window in Windows.Values) window.Root.RequestedTheme = theme; }); break;
            case "themes": return ThemeCards(title, description);
            case "language":
                var language = new ComboBox { Width = 130, MinHeight = 25, Padding = new(6, 2, 6, 2), FontSize = 11 };
                language.Items.Add("English"); language.Items.Add("简体中文"); language.SelectedIndex = 1; control = language; break;
        }
        return SettingRow(title, description, control);
    }
    private UIElement ThemeCards(string title, string description)
    {
        var wrapper = new StackPanel { Spacing = 10, Padding = new(16, 12, 16, 12) };
        wrapper.Children.Add(Label(title)); wrapper.Children.Add(Label(description, 10, "TertiaryBrush"));
        var grid = new Grid { ColumnSpacing = 8, RowSpacing = 8 }; grid.ColumnDefinitions.Add(new()); grid.ColumnDefinitions.Add(new());
        var themes = MacDesign.Root.GetProperty("themes").EnumerateArray().ToArray();
        for (var i = 0; i < themes.Length; i++)
        {
            if (i % 2 == 0) grid.RowDefinitions.Add(new() { Height = GridLength.Auto });
            var spec = themes[i]; var card = SmallButton(""); card.HorizontalAlignment = HorizontalAlignment.Stretch; card.HorizontalContentAlignment = HorizontalAlignment.Stretch; card.Padding = new(0); card.MinHeight = 108; card.CornerRadius = new(8);
            var body = new StackPanel(); var preview = new Grid { Height = 68, Padding = new(10), Background = ColorBrush(spec.GetProperty("canvas").GetString()!) };
            preview.Children.Add(new Border { Width = 24, Height = 24, CornerRadius = new(5), HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top, Background = ColorBrush(spec.GetProperty("accent").GetString()!) });
            var lines = new StackPanel { Spacing = 5, Margin = new(32, 4, 0, 0), VerticalAlignment = VerticalAlignment.Top };
            for (var n = 0; n < 3; n++) lines.Children.Add(new Border { Height = 5, Opacity = .2, Background = ColorBrush(spec.GetProperty("accent").GetString()!), CornerRadius = new(2), Margin = new(0, 0, n == 2 ? 30 : 0, 0) });
            preview.Children.Add(lines); body.Children.Add(preview); body.Children.Add(new TextBlock { Text = spec.GetProperty("name").GetString() + "    " + T("settings.themePackageBuiltIn"), FontSize = 11, Margin = new(9, 7, 9, 7), Foreground = Ink("PrimaryBrush") });
            card.Content = body; if (i == 0) card.BorderBrush = Ink("AccentBrush");
            card.Click += (_, _) => { foreach (var other in grid.Children.OfType<Button>()) other.BorderBrush = other == card ? Ink("AccentBrush") : Ink("BorderBrush"); Notify(new(true, "主题卡片选择演示；完整 Theme V2 渲染器尚未接入")); };
            Grid.SetRow(card, i / 2); Grid.SetColumn(card, i % 2); grid.Children.Add(card);
        }
        wrapper.Children.Add(grid); wrapper.Children.Add(SmallButton(T("settings.themePackageImport"), () => Notify(new(true, "主题导入界面演示；未安装主题包")))); return wrapper;
    }
    private static SolidColorBrush ColorBrush(string hex) => new(ColorHelper.FromArgb(255, Convert.ToByte(hex.Substring(1, 2), 16), Convert.ToByte(hex.Substring(3, 2), 16), Convert.ToByte(hex.Substring(5, 2), 16)));
    private void BuildConnections()
    {
        var rows = Section(ConnectionsContent, T("connections.title"));
        var remote = new StackPanel();
        var modes = Segments([("auto", T("settings.modeAuto")), ("lan", T("settings.modeLan")), ("iroh", T("settings.modeIroh")), ("tailscale", T("settings.modeTailscale"))], "auto", 280, mode => remote.Visibility = mode is "auto" or "iroh" ? Visibility.Visible : Visibility.Collapsed);
        rows.Children.Add(SettingRow(T("settings.connectionMode"), T("settings.connectionModeDescription"), modes)); Divider(rows);
        rows.Children.Add(SettingRow("MacBook Pro   " + T("settings.localDevice"), "D4:19:8E:2F:70:3A:6B:91\niroh: 9b41c6f8…e731a205")); Divider(rows);
        rows.Children.Add(SettingRow("2 " + T("settings.devices"), "", SmallButton("↻"), 6));
        foreach (var (name, status, enabled) in new[] { ("DESKTOP-V62KRAF", "192.168.1.106 · en0 · 局域网 · 7 ms", true), ("Studio PC", "离线 · Tailscale", false) })
        {
            var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 }; var latency = SmallButton("", () => Notify(new(true, "演示延迟：7 ms"))); latency.Content = Symbol("\uE8AB", 14); latency.MinWidth = 54; AutomationProperties.SetName(latency, T("settings.testAllConnections")); actions.Children.Add(latency); actions.Children.Add(SmallButton(T("settings.unpair"), () => Notify(new(true, "未移除真实设备")))); actions.Children.Add(Switch(name, enabled));
            rows.Children.Add(SettingRow("● " + name + "  ✓", status + "\n8F:62:41:BE:10:D2:59:AC", actions));
        }
        Divider(rows); var pair = SmallButton(T("settings.allowPairing")); pair.Click += Pair_Click; rows.Children.Add(SettingRow(T("settings.pairDevice"), T("settings.pairingClosed"), pair)); Divider(rows);
        var body = new StackPanel { Padding = new(16, 0, 16, 16), Spacing = 14, Visibility = Visibility.Collapsed };
        var create = SmallButton(T("settings.createRemoteInvite")); create.Click += Invite_Click;
        body.Children.Add(SettingRow(T("settings.createRemoteInvite"), T("settings.createRemoteInviteDescription"), create, 0));
        body.Children.Add(Label(T("settings.useRemoteInvite"), 12)); body.Children.Add(Label(T("settings.useRemoteInviteDescription"), 10, "SecondaryBrush"));
        body.Children.Add(new TextBox { PlaceholderText = T("settings.remoteInvitePlaceholder"), FontSize = 11, MinHeight = 26 });
        var inviteActions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Right };
        inviteActions.Children.Add(SmallButton("检查邀请", () => Notify(new(true, "远程邀请演示；未发送网络请求")))); var start = SmallButton("开始配对"); start.Click += Pair_Click; inviteActions.Children.Add(start); body.Children.Add(inviteActions);
        var expand = SmallButton(""); expand.Content = SettingRow("◎  " + T("settings.remotePairing"), T("settings.remotePairingDescription"), Label("›"), 10); expand.HorizontalContentAlignment = HorizontalAlignment.Stretch; expand.HorizontalAlignment = HorizontalAlignment.Stretch; expand.Background = new SolidColorBrush(Colors.Transparent); expand.BorderThickness = new(0); expand.Padding = new(0); expand.Click += (_, _) => body.Visibility = body.Visibility == Visibility.Visible ? Visibility.Collapsed : Visibility.Visible;
        remote.Children.Add(expand); remote.Children.Add(body); rows.Children.Add(remote);
    }
}
