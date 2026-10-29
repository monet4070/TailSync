using System.Diagnostics;
using System.IO;
using System.Security.Cryptography;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Microsoft.Win32;

namespace TailSync.PreviewDemo;

public sealed class PreviewWindow : Window
{
    private readonly Options _options;
    private readonly IPreviewEngine _engine = EngineFactory.Create();
    private readonly SemaphoreSlim _gate = new(1, 1);
    private readonly Telemetry _log;
    private readonly Image _image = new() { Stretch = Stretch.Fill, HorizontalAlignment = HorizontalAlignment.Center };
    private readonly TextBox _text = new() { IsReadOnly = true, TextWrapping = TextWrapping.Wrap, AcceptsReturn = true, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, Visibility = Visibility.Collapsed, Padding = new Thickness(18), FontSize = 14 };
    private readonly TextBox _search = new() { Width = 140, Height = 28, VerticalContentAlignment = VerticalAlignment.Center, ToolTip = "在当前页提取的文字中查找" };
    private readonly TextBlock _status = new() { TextWrapping = TextWrapping.Wrap, Margin = new Thickness(14, 7, 14, 7), FontSize = 12 };
    private readonly TextBlock _heading = new() { FontSize = 18, FontWeight = FontWeights.SemiBold, TextTrimming = TextTrimming.CharacterEllipsis, Margin = new Thickness(14, 8, 14, 0) };
    private readonly TextBlock _pageLabel = new() { VerticalAlignment = VerticalAlignment.Center, MinWidth = 70, TextAlignment = TextAlignment.Center };
    private readonly ScrollViewer _scroll = new() { HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
    private readonly StackPanel _toolbar = new() { Orientation = Orientation.Horizontal, Margin = new Thickness(8) };
    private int _page, _pages, _width, _revision;
    private bool _closing, _closed, _benchmarkFinished, _busy;
    private readonly CancellationTokenSource _lifetime = new();
    private int _exitCode;
    private double _openRequestMs;
    private string? _file;
    private readonly List<Button> _documentButtons = [];

    public PreviewWindow(Options options)
    {
        _options = options; _width = options.Width;
        _log = new(options.Output, _engine, options);
        Title = $"TailSync · {_engine.Name} · Preview Demo";
        Width = 800; Height = 720; MinWidth = 700; MinHeight = 460;
        Background = new SolidColorBrush(Color.FromRgb(248, 246, 240));
        FontFamily = new FontFamily("Segoe UI, Microsoft YaHei UI");
        var root = new Grid();
        foreach (var h in new[] { GridLength.Auto, GridLength.Auto, GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto }) root.RowDefinitions.Add(new() { Height = h });
        var header = new StackPanel();
        _heading.Text = _engine.Name;
        header.Children.Add(_heading);
        header.Children.Add(new TextBlock { Text = $"{_engine.Version}  ·  无 WebView  ·  {(_engine.Name == "MuPDF" ? "DOCX / Office 为重排预览" : "PDF 页面预览")}", Margin = new Thickness(14, 4, 14, 8), Foreground = Brushes.DimGray, FontSize = 11 });
        root.Children.Add(header);
        AddButton("打开文件", async () =>
        {
            var dialog = new OpenFileDialog { Filter = "Supported documents|" + string.Join(';', _engine.Extensions.Select(x => "*" + x)), CheckFileExists = true };
            if (dialog.ShowDialog(this) == true) await OpenAsync(dialog.FileName);
        });
        AddButton("关闭文档", CloseDocumentAsync, true);
        AddButton("上一页", () => ChangePageAsync(-1), true);
        _toolbar.Children.Add(_pageLabel);
        AddButton("下一页", () => ChangePageAsync(1), true);
        AddButton("−", () => ZoomAsync(-300), true);
        AddButton("+", () => ZoomAsync(300), true);
        AddButton("适应宽度", async () =>
        {
            _width = Math.Clamp((int)((_scroll.ViewportWidth - 36) * VisualTreeHelper.GetDpi(this).DpiScaleX), 128, 4096);
            await RenderAsync("fit");
        }, true);
        AddButton("页面", async () => { _text.Visibility = Visibility.Collapsed; _scroll.Visibility = Visibility.Visible; await Task.CompletedTask; }, true);
        var textButton = AddButton("文字", ShowTextAsync, true);
        textButton.ToolTip = _engine.SupportsText ? "提取当前页文字；并非页面上的选择层" : "此引擎不提供文字提取";
        Grid.SetRow(_toolbar, 1); root.Children.Add(_toolbar);
        var secondary = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(14, 0, 14, 8) };
        secondary.Children.Add(_search);
        var find = new Button { Content = "查找文字", Padding = new Thickness(10, 4, 10, 4), Margin = new Thickness(6, 0, 0, 0), IsEnabled = _engine.SupportsText };
        find.Click += async (_, _) => await GuardAsync(async () => { if (_text.Visibility != Visibility.Visible) await ShowTextAsync(); FindNext(); });
        secondary.Children.Add(find);
        secondary.Children.Add(new TextBlock { Text = "  Ctrl+O 打开 · Ctrl+滚轮缩放 · Esc 关闭文档", VerticalAlignment = VerticalAlignment.Center, Foreground = Brushes.DimGray, FontSize = 11 });
        Grid.SetRow(secondary, 2); root.Children.Add(secondary);
        _scroll.Content = _image; _scroll.Margin = new Thickness(12, 0, 12, 0);
        Grid.SetRow(_scroll, 3); Grid.SetRow(_text, 3); root.Children.Add(_scroll); root.Children.Add(_text);
        Grid.SetRow(_status, 4); root.Children.Add(_status);
        Content = root;
        SetDocumentControls(false);
        _status.Text = "打开 PDF" + (_engine.Name == "MuPDF" ? " 或 DOCX / PPTX / XLSX" : "") + $"。日志：{options.Output}";
        Loaded += async (_, _) => await GuardAsync(async () =>
        {
            _log.Event("window_loaded", new { dpi = VisualTreeHelper.GetDpi(this).DpiScaleX, width = ActualWidth, height = ActualHeight });
            if (options.Benchmark) await BenchmarkAsync();
            else if (options.File is not null) await OpenAsync(options.File);
        });
        Closing += (_, e) =>
        {
            if (_closed) return;
            e.Cancel = true;
            if (_closing) return;
            _closing = true; _revision++; _lifetime.Cancel();
            if (_options.Benchmark && !_benchmarkFinished && _exitCode == 0) _exitCode = 4;
            _ = ShutdownAsync();
        };
        PreviewKeyDown += async (_, e) =>
        {
            if (_options.Benchmark) return;
            if (e.Key == Key.Escape) { e.Handled = true; await GuardAsync(CloseDocumentAsync); }
            else if (e.Key == Key.O && Keyboard.Modifiers == ModifierKeys.Control) { e.Handled = true; ((Button)_toolbar.Children[0]).RaiseEvent(new RoutedEventArgs(Button.ClickEvent)); }
            else if (e.OriginalSource is not TextBox && e.Key is Key.Left or Key.Right) { e.Handled = true; await GuardAsync(() => ChangePageAsync(e.Key == Key.Left ? -1 : 1)); }
        };
        _scroll.PreviewMouseWheel += async (_, e) =>
        {
            if (Keyboard.Modifiers != ModifierKeys.Control || _options.Benchmark || _pages == 0) return;
            e.Handled = true; await GuardAsync(() => ZoomAsync(e.Delta > 0 ? 150 : -150));
        };
        if (_options.Benchmark) { _toolbar.IsEnabled = false; secondary.IsEnabled = false; }
    }
    private Button AddButton(string text, Func<Task> action, bool requiresDocument = false)
    {
        var button = new Button { Content = text, Padding = new Thickness(8, 5, 8, 5), Margin = new Thickness(3, 0, 3, 0), Background = Brushes.White, BorderBrush = new SolidColorBrush(Color.FromRgb(218, 213, 204)) };
        button.Click += async (_, _) => await GuardAsync(action);
        _toolbar.Children.Add(button);
        if (requiresDocument) _documentButtons.Add(button);
        return button;
    }
    private void SetDocumentControls(bool enabled)
    {
        foreach (var button in _documentButtons) button.IsEnabled = enabled && (!Equals(button.Content, "文字") || _engine.SupportsText);
        _pageLabel.Text = enabled ? $"{_page + 1} / {_pages}" : "—";
    }
    private async Task GuardAsync(Func<Task> action)
    {
        if (_closing || _busy) return;
        _busy = true;
        try { await action(); }
        catch (OperationCanceledException) when (_closing) { }
        catch (Exception error)
        {
            _status.Text = $"失败：{error.Message}";
            _log.Event("error", new { type = error.GetType().Name, error = error.ToString() });
            if (_options.Benchmark) { _exitCode = 3; Close(); }
        }
        finally { _busy = false; }
    }
    private async Task<T> EngineAsync<T>(Func<Task<T>> work)
    {
        await _gate.WaitAsync();
        try { return await Task.Run(work); }
        finally { _gate.Release(); }
    }
    private async Task EngineActionAsync(Action action) => await EngineAsync(() => { action(); return Task.FromResult(true); });

    private async Task OpenAsync(string path)
    {
        string extension = Path.GetExtension(path).ToLowerInvariant();
        if (!_engine.Extensions.Contains(extension)) throw new NotSupportedException($"{_engine.Name} does not support {extension}.");
        await CloseDocumentAsync();
        int revision = ++_revision;
        _openRequestMs = _log.ElapsedMs;
        _status.Text = "读取文件…";
        var read = Stopwatch.StartNew();
        var bytes = await Limits.ReadFileAsync(path);
        var readMs = read.Elapsed.TotalMilliseconds;
        var hash = Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
        if (_closing || revision != _revision) return;
        _log.Event("document_open_begin", new { name = Path.GetFileName(path), sha256 = hash, bytes = bytes.Length, extension, file_read_ms = readMs });
        var open = Stopwatch.StartNew();
        DocumentInfo info;
        try { info = await EngineAsync(() => _engine.OpenAsync(bytes, extension)); }
        catch { await EngineActionAsync(_engine.Close); throw; }
        if (_closing || revision != _revision) return;
        if (info.PageCount <= 0) throw new InvalidDataException("Document has no pages.");
        _pages = info.PageCount; _page = 0; _file = path;
        _heading.Text = Path.GetFileName(path) + (info.Reflowed ? " · 重排预览" : "");
        _log.Event("document_open", new { duration_ms = open.Elapsed.TotalMilliseconds, pages = _pages, reflowed = info.Reflowed, extension });
        SetDocumentControls(true);
        await RenderAsync("first_page");
    }
    private async Task RenderAsync(string reason)
    {
        if (_pages == 0 || _closing) return;
        int revision = ++_revision, page = _page, width = _width;
        _status.Text = $"渲染第 {page + 1} 页…";
        var requestMs = _log.ElapsedMs;
        var frame = await EngineAsync(async () =>
        {
            if (_closing || revision != _revision) return null;
            var render = Stopwatch.StartNew();
            var result = await _engine.RenderAsync(page, width);
            result.Validate();
            _log.Event("engine_render", new { page = page + 1, reason, requested_width = width, width = result.Width, height = result.Height, duration_ms = render.Elapsed.TotalMilliseconds, pixel_bytes = result.Bgra.Length });
            return result;
        });
        if (frame is null || _closing || revision != _revision) return;
        // WPF owns a copy. Do not retain the managed frame or all previously visited pages.
        var bitmap = BitmapSource.Create(frame.Width, frame.Height, 96, 96, PixelFormats.Bgra32, null, frame.Bgra, frame.Stride);
        bitmap.Freeze();
        _image.Source = bitmap;
        _image.Width = frame.Width / VisualTreeHelper.GetDpi(this).DpiScaleX;
        _image.Height = frame.Height / VisualTreeHelper.GetDpi(this).DpiScaleY;
        _scroll.Visibility = Visibility.Visible; _text.Visibility = Visibility.Collapsed; _text.Clear();
        _scroll.ScrollToTop();
        SetDocumentControls(true);
        var elapsed = _log.ElapsedMs - requestMs;
        _status.Text = $"第 {page + 1}/{_pages} 页 · {frame.Width}×{frame.Height} · {elapsed:F1} ms · 无页面缓存";
        _log.Event("frame_assigned", new { page = page + 1, reason, duration_ms = elapsed, first_from_open_request_ms = reason == "first_page" ? (double?)(_log.ElapsedMs - _openRequestMs) : null });
        await RenderTickAsync();
        _log.Event("render_tick", new { page = page + 1, reason, duration_ms = _log.ElapsedMs - requestMs, first_from_open_request_ms = reason == "first_page" ? (double?)(_log.ElapsedMs - _openRequestMs) : null });
    }
    private static async Task RenderTickAsync()
    {
        var completion = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        EventHandler? handler = null;
        handler = (_, _) => { CompositionTarget.Rendering -= handler; completion.TrySetResult(); };
        CompositionTarget.Rendering += handler;
        try
        {
            if (await Task.WhenAny(completion.Task, Task.Delay(3000)) != completion.Task)
                throw new TimeoutException("No WPF render tick. Keep the benchmark window visible and unminimized.");
        }
        finally { CompositionTarget.Rendering -= handler; }
    }
    private async Task ChangePageAsync(int delta)
    {
        if (_pages == 0) return;
        var next = Math.Clamp(_page + delta, 0, _pages - 1);
        if (next == _page) return;
        _page = next; await RenderAsync("page");
    }
    private async Task ZoomAsync(int delta)
    {
        if (_pages == 0) return;
        _width = Math.Clamp(_width + delta, 128, 4096); await RenderAsync("zoom");
    }
    private async Task ShowTextAsync()
    {
        if (_pages == 0 || !_engine.SupportsText) return;
        int revision = _revision, page = _page;
        var watch = Stopwatch.StartNew();
        string text = await EngineAsync(() => _engine.TextAsync(page));
        if (_closing || revision != _revision) return;
        _log.Event("text_extract", new { page = page + 1, duration_ms = watch.Elapsed.TotalMilliseconds, characters = text.Length });
        _text.Text = text;
        _scroll.Visibility = Visibility.Collapsed; _text.Visibility = Visibility.Visible;
        _status.Text = "当前页提取文字，可选择/复制/查找；不是原页面上的文字选择层。最多显示 250,000 字符。";
    }
    private void FindNext()
    {
        if (_search.Text.Length == 0) return;
        int start = Math.Min(_text.Text.Length, _text.SelectionStart + _text.SelectionLength);
        int at = _text.Text.IndexOf(_search.Text, start, StringComparison.OrdinalIgnoreCase);
        if (at < 0) at = _text.Text.IndexOf(_search.Text, StringComparison.OrdinalIgnoreCase);
        if (at < 0) { _status.Text = "当前页未找到。"; return; }
        _text.Focus(); _text.Select(at, _search.Text.Length); _text.ScrollToLine(_text.GetLineIndexFromCharacterIndex(at));
    }
    private async Task CloseDocumentAsync()
    {
        _revision++; _pages = 0; _file = null;
        _image.Source = null; _text.Clear(); _text.Visibility = Visibility.Collapsed; _scroll.Visibility = Visibility.Visible;
        SetDocumentControls(false);
        await EngineActionAsync(_engine.Close);
        _heading.Text = _engine.Name; _status.Text = "文档已释放；保留进程以观察空闲内存。";
        _log.Event("document_closed");
    }
    private async Task BenchmarkAsync()
    {
        _log.Event("baseline_begin"); await Task.Delay(_options.IdleMs, _lifetime.Token); _log.Event("baseline_end");
        for (int cycle = 0; cycle < _options.Cycles && !_closing; cycle++)
        {
            _log.Event("cycle_begin", new { cycle }); _width = _options.Width;
            await OpenAsync(_options.File!);
            await Task.Delay(_options.DwellMs, _lifetime.Token);
            foreach (int number in _options.Pages)
            {
                if (_closing) return;
                if (number > _pages) { _log.Event("page_skipped", new { number, pages = _pages }); continue; }
                _page = number - 1; await RenderAsync("benchmark_page"); await Task.Delay(_options.DwellMs, _lifetime.Token);
            }
            _width = Math.Min(4096, _options.Width * 2); await RenderAsync("benchmark_zoom"); await Task.Delay(_options.DwellMs, _lifetime.Token);
            _width = _options.Width; await RenderAsync("benchmark_restore_zoom"); await Task.Delay(_options.DwellMs, _lifetime.Token);
            await CloseDocumentAsync(); _log.Event("idle_after_close_begin", new { cycle });
            await Task.Delay(_options.IdleMs, _lifetime.Token); _log.Sample(); _log.Event("idle_after_close_end", new { cycle });
            _log.Event("cycle_end", new { cycle });
        }
        if (_closing) return;
        _benchmarkFinished = true; _log.Event("benchmark_complete"); Close();
    }
    private async Task ShutdownAsync()
    {
        try
        {
            _image.Source = null; _text.Clear();
            await EngineActionAsync(_engine.Dispose);
            _log.Event("shutdown", new { exit_code = _exitCode });
        }
        catch (Exception error) { _exitCode = 3; _log.Event("shutdown_error", new { error = error.ToString() }); }
        finally { _log.Dispose(); _closed = true; Application.Current.Shutdown(_exitCode); }
    }
}
