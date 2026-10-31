using System.Text.Json;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Microsoft.Win32;

namespace TailSync.Wpf;

// Theme settings belong to the same Core store as the former React front end.
// This view contains no alternate theme-file parser or private settings store.
public sealed class ThemeManagerView : StackPanel
{
    private readonly bool _connectOnLoad;
    private readonly Func<string, string, bool, CancellationToken, Task> _persist;
    private readonly CancellationToken? _testToken;
    private readonly StackPanel _body = new();
    private readonly TextBlock _status = Ui.Text("", 12, "Secondary");
    private JsonElement[] _descriptors = [];
    private CancellationTokenSource? _lifetime;
    private bool _busy;
    public ThemeManagerView(bool connectOnLoad = true) : this(connectOnLoad, ThemeService.PersistAsync) { }

    internal ThemeManagerView(bool connectOnLoad, Func<string, string, bool, CancellationToken, Task> persist, CancellationToken? testToken = null)
    {
        _connectOnLoad = connectOnLoad;
        _persist = persist;
        _testToken = testToken;
        Margin = new Thickness(16, 12, 16, 12);
        _status.Margin = new Thickness(0, 10, 0, 0);
        AutomationProperties.SetLiveSetting(_status, AutomationLiveSetting.Polite);
        Children.Add(_body); Children.Add(_status);
        Loaded += LoadedView;
        Unloaded += (_, _) => { _lifetime?.Cancel(); _lifetime?.Dispose(); _lifetime = null; ThemeService.Changed -= ThemeChanged; Localizer.Changed -= ThemeChanged; };
        Rebuild();
    }

    private static string T(string key) => Localizer.Text(key);
    private static string Text(string zh, string en) => Localizer.Locale == "en" ? en : zh;
    private CancellationToken Token => _lifetime?.Token ?? _testToken ?? new CancellationToken(true);

    private async void LoadedView(object sender, RoutedEventArgs e)
    {
        if (_lifetime != null) return;
        _lifetime = new CancellationTokenSource();
        ThemeService.Changed += ThemeChanged; Localizer.Changed += ThemeChanged;
        _body.IsEnabled = !_busy;
        if (_connectOnLoad) await Run(RefreshAsync);
        else { _descriptors = DemoDescriptors(); Rebuild(); }
    }

    private void ThemeChanged(object? sender, EventArgs args) => Rebuild();

    public async Task RefreshAsync()
    {
        var token = Token;
        var lifetime = _lifetime;
        var values = await NativeBackend.InvokeAsync<JsonElement>("list_themes_v2", token: token);
        if (token.IsCancellationRequested || !ReferenceEquals(_lifetime, lifetime)) return;
        if (values.ValueKind != JsonValueKind.Array) throw new InvalidOperationException(Text("主题列表格式无效", "Invalid theme list"));
        SetDescriptors(values.EnumerateArray());
    }

    internal void SetDescriptors(IEnumerable<JsonElement> descriptors)
    {
        _descriptors = descriptors.Select(x => x.Clone()).ToArray();
        Rebuild();
    }

    private void Rebuild()
    {
        _body.Children.Clear();
        var modeHeading = Ui.Text(T("settings.theme"), 14); _body.Children.Add(modeHeading);
        var modes = new UniformGrid { Columns = 3 };
        foreach (var (id, key) in new[] { ("system", "settings.themeSystem"), ("light", "settings.themeLight"), ("dark", "settings.themeDark") })
        {
            var radio = Ui.Name(new RadioButton { Content = T(key), IsChecked = ThemeService.Appearance == id, GroupName = "theme-mode-" + GetHashCode() }, T(key));
            radio.SetResourceReference(StyleProperty, "SegmentButton"); ButtonDesign.SetIcon(radio, id); ControlMotion.SegmentSurface(radio);
            radio.Checked += async (_, _) => await Run(async () =>
            {
                if (_connectOnLoad) await _persist(id, ThemeService.ActiveThemeId, ThemeService.HighContrast, Token);
                else ThemeService.Apply(appearance: id);
            });
            modes.Children.Add(radio);
        }
        var segment = new Border { Child = modes, Padding = new Thickness(2), CornerRadius = new CornerRadius(7), Margin = new Thickness(0, 0, 0, 12) };
        segment.SetResourceReference(BackgroundProperty, "InputBrush"); _body.Children.Add(segment);
        var contrast = Ui.Name(new CheckBox { Content = Text("提高对比度", "Increase contrast"), IsChecked = ThemeService.HighContrast, Margin = new Thickness(0, 0, 0, 18) }, Text("提高对比度", "Increase contrast"));
        contrast.Click += async (_, _) => await Run(async () =>
        {
            if (_connectOnLoad) await _persist(ThemeService.Appearance, ThemeService.ActiveThemeId, contrast.IsChecked == true, Token);
            else _status.Text = Text("预览模式不保存设置", "Preview mode does not save settings");
        });
        _body.Children.Add(contrast);
        _body.Children.Add(Ui.Text(T("settings.colorTheme"), 14));
        var caption = Ui.Text(T("settings.colorThemeDescription"), 12, "Secondary"); caption.Margin = new Thickness(0, 4, 0, 8); _body.Children.Add(caption);
        var cards = new WrapPanel { Name = "ThemeCards", Margin = new Thickness(-4, 0, -4, 0) };
        cards.SizeChanged += (_, e) =>
        {
            if (e.WidthChanged && e.NewSize.Width > 0)
                cards.ItemWidth = Math.Floor(e.NewSize.Width / Math.Max(1, (int)(e.NewSize.Width / 222)));
        };
        foreach (var descriptor in _descriptors) cards.Children.Add(Card(descriptor));
        _body.Children.Add(cards);
        var actions = new WrapPanel { Margin = new Thickness(0, 8, 0, 0) };
        actions.Children.Add(ActionButton("add", T("settings.customThemeImport"), () => ChoosePackage(null)));
        if (_connectOnLoad)
        {
            var refresh = ActionButton("refresh", Text("刷新主题", "Refresh themes"), RefreshAsync); refresh.Margin = new Thickness(8, 0, 0, 0); actions.Children.Add(refresh);
        }
        _body.Children.Add(actions); _body.IsEnabled = !_busy;
    }

    private FrameworkElement Card(JsonElement descriptor)
    {
        var id = ThemeV2.Text(descriptor, "id");
        var label = ThemeV2.Text(descriptor, "name/" + Localizer.Locale, ThemeV2.Text(descriptor, "name/en", id));
        var valid = ThemeV2.Text(descriptor, "status") == "valid";
        var custom = ThemeV2.Text(descriptor, "source") == "custom";
        var root = new StackPanel { Margin = new Thickness(4, 4, 4, 8) };
        var button = Ui.Name(new Button { Padding = new Thickness(0), HorizontalContentAlignment = HorizontalAlignment.Stretch, IsEnabled = valid }, label);
        button.SetResourceReference(Control.BorderBrushProperty, id == ThemeService.ActiveThemeId ? "AccentBrush" : "BorderBrush");
        button.BorderThickness = new Thickness(1);
        if (ThemeV2.Value(descriptor, "diagnostics") is { ValueKind: JsonValueKind.Array } diagnostics)
        {
            var message = string.Join("\n", diagnostics.EnumerateArray().Select(ThemeV2Packages.FormatDiagnostic).Where(text => !string.IsNullOrWhiteSpace(text)));
            if (!string.IsNullOrWhiteSpace(message)) button.ToolTip = message;
        }
        var content = new StackPanel();
        var candidate = ThemeV2.Value(descriptor, ThemeService.EffectiveMode == "dark" ? "resolvedDark" : "resolvedLight");
        var sample = candidate is { ValueKind: JsonValueKind.Object } resolved
            ? resolved : JsonSerializer.SerializeToElement(new { id, mode = ThemeService.EffectiveMode });
        var preview = new Border { Padding = new Thickness(12), Height = 98, Resources = ThemeService.CreateResources(sample) };
        preview.SetResourceReference(Border.CornerRadiusProperty, "ControlRadius");
        preview.SetResourceReference(BackgroundProperty, "CanvasBrush");
        var heading = Ui.Text(valid ? "Aa · 文" : "!", 23); heading.SetResourceReference(TextBlock.FontFamilyProperty, "DisplayFont");
        var mini = Ui.Text(Text("一个安静的剪贴板", "A quiet clipboard"), 12, "Secondary"); mini.SetResourceReference(TextBlock.FontFamilyProperty, "HistoryFont");
        var swatches = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 8, 0, 0) };
        foreach (var color in new[] { "AccentBrush", "SecondaryBrush", "BorderBrush" })
        {
            var swatch = new Border { Width = 35, Height = 5, Margin = new Thickness(0, 0, 5, 0), CornerRadius = new CornerRadius(2) };
            swatch.SetResourceReference(BackgroundProperty, color); swatches.Children.Add(swatch);
        }
        preview.Child = Ui.Stack(Orientation.Vertical, 1, heading, mini, swatches); content.Children.Add(preview);
        var name = Ui.Text(label, 13); name.FontWeight = FontWeights.SemiBold;
        var detail = Ui.Text(valid ? custom ? ThemeV2.Text(descriptor, "version") : T("settings.themePackageBuiltIn") : T("settings.customThemeInvalid"), 12, "Secondary");
        var nameLine = new Grid(); nameLine.ColumnDefinitions.Add(new()); nameLine.ColumnDefinitions.Add(new() { Width = GridLength.Auto }); nameLine.Children.Add(name);
        if (id == ThemeService.ActiveThemeId) { var mark = new VectorIcon { Kind = "check", Width = 14, Height = 14 }; mark.SetResourceReference(VectorIcon.StrokeProperty, "AccentBrush"); Grid.SetColumn(mark, 1); nameLine.Children.Add(mark); }
        var footer = Ui.Stack(Orientation.Vertical, 2, nameLine, detail); footer.Margin = new Thickness(10, 8, 10, 8); content.Children.Add(footer);
        button.Content = content; button.Click += async (_, _) => await Run(async () =>
        {
            if (_connectOnLoad) await _persist(ThemeService.Appearance, id, ThemeService.HighContrast, Token);
            else ThemeService.Apply(id: id);
        });
        root.Children.Add(button);
        if (custom || !valid)
        {
            var tools = new WrapPanel { Margin = new Thickness(0, 6, 0, 0) };
            if (custom && valid)
            {
                tools.Children.Add(ActionButton("refresh", T("settings.customThemeUpdate"), () => ChoosePackage(descriptor)));
                var rollback = ActionButton("history", T("settings.customThemeRollback"), async () =>
                {
                    if (!Confirm(T("settings.customThemeRollbackConfirm") + " “" + label + "”?")) return;
                    await NativeBackend.InvokeAsync<JsonElement>("rollback_theme", new { themeId = id }, Token);
                    await RefreshAsync(); await ThemeService.LoadAsync(Token);
                });
                rollback.Margin = new Thickness(4, 0, 0, 0); tools.Children.Add(rollback);
            }
            var delete = ActionButton("delete", T("settings.customThemeDelete"), async () =>
            {
                if (!Confirm(T("settings.customThemeDelete") + " “" + label + "”?")) return;
                await NativeBackend.InvokeAsync<JsonElement>("delete_theme_v2", new { themeId = id, storageHandle = ThemeV2.Text(descriptor, "storageHandle") }, Token);
                await RefreshAsync(); await ThemeService.LoadAsync(Token);
            });
            delete.Margin = new Thickness(4, 0, 0, 0); tools.Children.Add(delete); root.Children.Add(tools);
        }
        return root;
    }

    private Button ActionButton(string icon, string label, Func<Task> action)
    {
        var button = ActionButton(label, action); ButtonDesign.SetIcon(button, icon); return button;
    }
    private Button ActionButton(string label, Func<Task> action)
    {
        var button = Ui.Button(label);
        button.Click += async (_, _) => await Run(action); return button;
    }

    private async Task ChoosePackage(JsonElement? update)
    {
        var token = Token;
        var lifetime = _lifetime;
        token.ThrowIfCancellationRequested();
        if (!_connectOnLoad) { _status.Text = Text("预览模式不安装主题包", "Preview mode does not install theme packages"); return; }
        var chooser = new OpenFileDialog { Title = T(update == null ? "settings.customThemeImportTitle" : "settings.customThemeUpdateTitle"), Filter = "TailSync V2 theme (*.tailsync-theme)|*.tailsync-theme", Multiselect = false, CheckFileExists = true };
        if (chooser.ShowDialog(Window.GetWindow(this)) != true) return;
        var pending = await ThemeV2Packages.ValidateAsync(chooser.FileName, update is { } descriptor ? ThemeV2.Text(descriptor, "id") : null,
            update is { } installed ? ThemeV2.Text(installed, "version") : null, token);
        if (token.IsCancellationRequested || !ReferenceEquals(_lifetime, lifetime)) return;
        var window = new ThemePackagePreviewWindow(pending) { Owner = Window.GetWindow(this) };
        if (window.ShowDialog() != true) return;
        if (pending.VersionRelation is "same" or "downgrade")
        {
            var key = pending.VersionRelation == "same" ? "settings.customThemeReplaceConfirm" : "settings.customThemeDowngradeConfirm";
            if (!Confirm(T(key) + "\n" + ThemeV2.Text(update!.Value, "version") + " → " + pending.Version)) return;
        }
        await ThemeV2Packages.ApplyAsync(pending, token);
        if (token.IsCancellationRequested || !ReferenceEquals(_lifetime, lifetime)) return;
        await RefreshAsync(); await ThemeService.LoadAsync(token);
    }

    private bool Confirm(string message) => MessageBox.Show(Window.GetWindow(this), message, T("settings.colorTheme"), MessageBoxButton.YesNo, MessageBoxImage.Question, MessageBoxResult.No) == MessageBoxResult.Yes;

    private async Task Run(Func<Task> action)
    {
        var token = Token;
        var lifetime = _lifetime;
        if (_busy || token.IsCancellationRequested) return;
        _busy = true; _body.IsEnabled = false; _status.Text = "";
        try { await action(); }
        catch (OperationCanceledException) { }
        catch (Exception error)
        {
            if (token.IsCancellationRequested || !ReferenceEquals(_lifetime, lifetime)) return;
            _status.Text = ThemeV2Packages.FormatError(error);
            // Native RadioButton/CheckBox input already changed IsChecked before
            // this handler ran. Failed writes must restore the canonical state.
            Rebuild();
        }
        finally { _busy = false; if (!token.IsCancellationRequested && ReferenceEquals(_lifetime, lifetime)) _body.IsEnabled = true; }
    }

    private static JsonElement[] DemoDescriptors() => new[] { "canvas", "flux", "ledger", "aura", "mono" }.Select(id => JsonSerializer.SerializeToElement(new
    {
        id = "builtin:" + id + "@1", source = "builtin", version = "1.0.0", status = "valid", name = new Dictionary<string, string> { ["en"] = char.ToUpperInvariant(id[0]) + id[1..], ["zh-CN"] = id switch { "canvas" => "画布 Canvas", "flux" => "流光 Flux", "ledger" => "书页 Ledger", "aura" => "柔光 Aura", _ => "单色 Mono" } }
    })).ToArray();
}

internal sealed class ThemePackagePreviewWindow : MacWindow
{
    internal ThemePackagePreviewWindow(PendingThemePackage package)
    {
        Title = Localizer.Text("settings.themePreviewTitle"); Width = 820; Height = 650; MinWidth = 560; MinHeight = 440;
        var root = new Grid(); root.RowDefinitions.Add(new() { Height = new GridLength(32) }); root.RowDefinitions.Add(new()); root.RowDefinitions.Add(new() { Height = GridLength.Auto });
        root.Children.Add(MakeTitleBar(Title));
        var previews = new WrapPanel { Margin = new Thickness(12) };
        var labels = new[] { "settings.themePreviewLight", "settings.themePreviewDark", "settings.themePreviewHighContrastLight", "settings.themePreviewHighContrastDark" };
        for (var index = 0; index < package.Previews.Count; index++) previews.Children.Add(Preview(package.Previews[index], Localizer.Text(labels[index]), package.Path));
        var scroll = new ScrollViewer { Content = previews, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled }; SmoothScroll.SetIsEnabled(scroll, true); Grid.SetRow(scroll, 1); root.Children.Add(scroll);
        var footer = new StackPanel { Margin = new Thickness(20, 10, 20, 16) };
        footer.Children.Add(Ui.Text(Localizer.Text("settings.customThemeCandidateVersion") + ": " + package.Version, 13));
        foreach (var message in package.Diagnostics) footer.Children.Add(Ui.Text(message, 12, "Secondary"));
        var buttons = Ui.Stack(Orientation.Horizontal, 8, Ui.Button(Localizer.Text("settings.cancel"), () => DialogResult = false), Ui.Button(Localizer.Text(package.UpdateThemeId == null ? "settings.customThemeInstall" : "settings.customThemeUpdate"), () => DialogResult = true, "PrimaryButton"));
        buttons.HorizontalAlignment = HorizontalAlignment.Right; buttons.Margin = new Thickness(0, 10, 0, 0); footer.Children.Add(buttons); Grid.SetRow(footer, 2); root.Children.Add(footer);
        Content = root; PreviewKeyDown += (_, e) => { if (e.Key == Key.Escape) { e.Handled = true; DialogResult = false; } };
    }

    private static Border Preview(JsonElement resolved, string label, string packagePath)
    {
        var border = new Border { Width = 350, Margin = new Thickness(8), Padding = new Thickness(16), BorderThickness = new Thickness(1), Resources = ThemeService.CreateResources(resolved) };
        border.SetResourceReference(Border.BackgroundProperty, "CanvasBrush"); border.SetResourceReference(Border.BorderBrushProperty, "BorderBrush"); border.SetResourceReference(Border.CornerRadiusProperty, "SurfaceRadius");
        var panel = new StackPanel(); panel.Children.Add(Ui.Text(label, 14));
        var search = Ui.Text("搜索历史 · Search history", 18); search.SetResourceReference(TextBlock.FontFamilyProperty, "SearchFont"); search.SetResourceReference(TextBlock.FontSizeProperty, "SearchFontSize"); search.Margin = new Thickness(0, 14, 0, 12); panel.Children.Add(search);
        var row = Ui.Text("一个安静的剪贴板 · A quiet clipboard", 13); row.SetResourceReference(TextBlock.FontFamilyProperty, "HistoryFont"); panel.Children.Add(row);
        var states = new WrapPanel { Margin = new Thickness(-3, 12, -3, 0) };
        foreach (var state in new[] { "default", "hover", "active", "selected", "disabled", "focus" })
        {
            var text = Ui.Text(Localizer.Text("settings.themePreviewState" + char.ToUpperInvariant(state[0]) + state[1..]), 12);
            var sample = new Border { Child = text, Padding = new Thickness(8, 5, 8, 5), Margin = new Thickness(3), BorderThickness = new Thickness(state == "focus" ? 2 : 1), CornerRadius = new CornerRadius(5) };
            var prefix = "ThemeButton" + char.ToUpperInvariant(state[0]) + state[1..];
            if (border.Resources.Contains(prefix + "BackgroundBrush")) sample.SetResourceReference(BackgroundProperty, prefix + "BackgroundBrush"); else sample.SetResourceReference(BackgroundProperty, "SurfaceBrush");
            if (border.Resources.Contains(prefix + "ForegroundBrush")) text.SetResourceReference(TextBlock.ForegroundProperty, prefix + "ForegroundBrush");
            sample.SetResourceReference(BorderBrushProperty, border.Resources.Contains(prefix + "BorderBrush") ? prefix + "BorderBrush" : "BorderBrush");
            if (state == "focus" && border.Resources.Contains(prefix + "FocusRingBrush")) sample.SetResourceReference(BorderBrushProperty, prefix + "FocusRingBrush");
            states.Children.Add(sample);
        }
        panel.Children.Add(states);
        var assets = new WrapPanel { Margin = new Thickness(0, 10, 0, 0) }; panel.Children.Add(assets);
        CancellationTokenSource? lifetime = null;
        border.Loaded += async (_, _) =>
        {
            if (lifetime != null) return;
            var current = new CancellationTokenSource(); lifetime = current;
            var token = current.Token;
            try
            {
                var images = await ThemeV2Assets.LoadAsync(resolved, packagePath, token);
                if (token.IsCancellationRequested || !ReferenceEquals(lifetime, current) || !border.IsLoaded) return;
                foreach (var (slot, image) in images)
                {
                    var visual = new Image { Source = image, Width = 72, Height = 46, Stretch = Stretch.Uniform, Margin = new Thickness(0, 0, 8, 0), ToolTip = slot };
                    assets.Children.Add(visual);
                }
            }
            catch (OperationCanceledException) { }
        };
        border.Unloaded += (_, _) => { lifetime?.Cancel(); lifetime?.Dispose(); lifetime = null; assets.Children.Clear(); };
        border.Child = panel; return border;
    }
}
