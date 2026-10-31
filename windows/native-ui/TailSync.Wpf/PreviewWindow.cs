using System.IO;
using System.Text;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using ICSharpCode.AvalonEdit;
using ICSharpCode.AvalonEdit.Highlighting;
using SkiaSharp;
using TailSync.Native.Core;

namespace TailSync.Wpf;

public sealed class PreviewWindow : MacWindow
{
    private readonly Grid _root = new();
    private readonly ContentControl _content = new();
    private readonly WrapPanel _tools = new() { Margin = new(14, 7, 14, 7) };
    private readonly TextBox _search;
    private readonly TextEditor _text = new() { IsReadOnly = true, WordWrap = true, Padding = new(20), FontSize = 15, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
    private readonly ComboBox _mode = new() { Width = 108, Height = 34, VerticalAlignment = VerticalAlignment.Center, Margin = new(6, 0, 0, 0) };
    private readonly TextBlock _name = Ui.Text("", 16), _metadata = Ui.Text("", 12, "Secondary"), _notice = Ui.Text("", 12, "Secondary");
    private readonly Button _previous, _next, _restore, _pdfPrevious, _pdfNext;
    private readonly TextBlock _pdfPageLabel=Ui.Text("",12,"Secondary");
    private readonly StackPanel _pdfNavigation;
    private readonly TextBlock _batchLabel = Ui.Text("", 12, "Secondary");
    private readonly TextBlock _zoomLabel = Ui.Text("100%", 12, "Secondary");
    private readonly Button _fit, _rotate, _copy, _svgSource, _svgTrust, _pdfText, _wrap;
    private readonly StackPanel _navigation;
    private readonly SemaphoreSlim _pdfRender = new(1);
    private CancellationTokenSource? _load;
    private HistoryEntry _entry;
    private string? _batchId;
    private PreviewPayload? _payload;
    private string _source = "";
    private string _limitedSource = "";
    private int _generation, _pdfPage, _pdfPages;
    private double _zoom = 1, _rotation;
    private bool _fitToWindow = true, _closed, _renderingText, _showSvgSource, _pdfTextMode;
    private byte[]? _trustedSvg;
    private SvgExternalSummary? _svgSummary;
    private Image? _image;
    private ScrollViewer? _imageScroll;
    private FlowDocumentScrollViewer? _document;
    private readonly double _defaultTextSize = PreviewPreferences.FontSize;

    public PreviewWindow(HistoryEntry entry, Action restore, string? batchId = null)
    {
        _entry = entry; _batchId = batchId;
        Title = Localizer.Text("history.preview.title"); Width = 640; Height = 560; MinWidth = 420; MinHeight = 360;
        foreach (var height in new[] { new GridLength(32), GridLength.Auto, GridLength.Auto, GridLength.Auto, new GridLength(1, GridUnitType.Star) })
            _root.RowDefinitions.Add(new() { Height = height });
        _root.Children.Add(MakeTitleBar(Title));
        var heading = new Grid { Margin = new(20, 16, 20, 14) };
        _name.FontWeight = FontWeights.SemiBold;
        heading.ColumnDefinitions.Add(new()); heading.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        _name.TextWrapping = TextWrapping.NoWrap; _name.TextTrimming = TextTrimming.CharacterEllipsis;
        _name.SetResourceReference(TextBlock.FontFamilyProperty, "ReadingFont");
        var title = Ui.Stack(Orientation.Vertical, 4, _name, _metadata); title.Margin = new(0, 0, 12, 0); heading.Children.Add(title);
        _restore = Ui.Button(Localizer.Text("history.preview.restore"), () => _ = RestoreAsync(), "PrimaryButton", icon: "copy"); Grid.SetColumn(_restore, 1); heading.Children.Add(_restore);
        Grid.SetRow(heading, 1); _root.Children.Add(heading);
        _search = Ui.Name(new TextBox { Tag = Localizer.Text("history.preview.search"), Width = 165, Height = 34, VerticalAlignment = VerticalAlignment.Center, Padding = new(10, 5, 10, 5), FontSize = 13 }, Localizer.Text("history.preview.search"));
        _search.TextChanged += (_, _) => Search(); _search.KeyDown += (_, e) => { if (e.Key == Key.Enter) { e.Handled = true; Search(true); } };
        _tools.Children.Add(_search);
        foreach (var (id, label) in new[] { ("text", "纯文本"), ("code", "源代码"), ("markdown", "Markdown") }) _mode.Items.Add(new ComboBoxItem { Tag = id, Content = label });
        _mode.SelectedIndex = 0; _mode.SelectionChanged += (_, _) => { if (!_renderingText && _payload != null && IsText(_payload)) ShowText(); }; _tools.Children.Add(_mode);
        var smaller = IconTool("subtract", Localizer.Choose("缩小", "Zoom out"), () => Zoom(-1)); var larger = IconTool("add", Localizer.Choose("放大", "Zoom in"), () => Zoom(1));
        _tools.Children.Remove(smaller); _tools.Children.Remove(larger); _zoomLabel.Margin = new(7, 0, 7, 0); _tools.Children.Add(Ui.Stack(Orientation.Horizontal, 0, smaller, larger, _zoomLabel));
        _fit = Tool(Localizer.Text("history.preview.fit"), Fit);
        _rotate = IconTool("rotate", Localizer.Text("history.preview.rotate"), () => { _rotation = (_rotation + 90) % 360; UpdateImageLayout(); }); _rotate.ToolTip = Localizer.Text("history.preview.rotate");
        _pdfText=Tool(Localizer.Choose("可选择文本","Selectable text"),()=>{_pdfTextMode=!_pdfTextMode;_ = ShowPdfPage();});
        _svgTrust=Tool(Localizer.Text("history.preview.svgTrustExternal"),()=>_ = GrantSvgAsync());
        _svgSource = Tool(Localizer.Text("history.preview.svgSource"), () => { _showSvgSource = !_showSvgSource; if (_showSvgSource) ShowText(true); else ShowSvg(); });
        _wrap = IconTool("wrap", Localizer.Text("history.preview.wrap"), () => _text.WordWrap = !_text.WordWrap); _wrap.ToolTip = Localizer.Text("history.preview.wrap");
        _copy = Tool("全部复制", Copy); ButtonDesign.SetIcon(_copy, "copy"); ButtonDesign.SetIcon(_fit, "fit"); ButtonDesign.SetIcon(_svgSource, "code");
        _previous = Ui.IconButton("back", Localizer.Text("history.preview.previous"), () => Navigate(-1)); _next = Ui.IconButton("chevron", Localizer.Text("history.preview.next"), () => Navigate(1));
        _previous.ToolTip = Localizer.Text("history.preview.previous"); _next.ToolTip = Localizer.Text("history.preview.next");
        _navigation = Ui.Stack(Orientation.Horizontal, 7, _previous, _batchLabel, _next); _navigation.Margin = new(9, 0, 0, 0); _tools.Children.Add(_navigation);
        _pdfPrevious=Ui.IconButton("back", Localizer.Choose("上一页", "Previous page"),()=>NavigatePdf(-1));_pdfNext=Ui.IconButton("chevron", Localizer.Choose("下一页", "Next page"),()=>NavigatePdf(1));
        _pdfNavigation=Ui.Stack(Orientation.Horizontal,7,_pdfPrevious,_pdfPageLabel,_pdfNext);_pdfNavigation.Margin=new(9,0,0,0);_tools.Children.Add(_pdfNavigation);
        var toolbar = new Border { Child = _tools, BorderThickness = new(0, 1, 0, 1) }; toolbar.SetResourceReference(Border.BackgroundProperty, "SurfaceBrush"); toolbar.SetResourceReference(Border.BorderBrushProperty, "DividerBrush"); Grid.SetRow(toolbar, 2); _root.Children.Add(toolbar);
        _notice.Margin = new(18, 0, 18, 8); _notice.Visibility = Visibility.Collapsed; Grid.SetRow(_notice, 3); _root.Children.Add(_notice);
        var contentFrame = Ui.Surface(_content); contentFrame.Margin = new(16, 12, 16, 16); contentFrame.ClipToBounds = true; Grid.SetRow(contentFrame, 4); _root.Children.Add(contentFrame);
        _text.SetResourceReference(Control.ForegroundProperty, "PrimaryBrush"); _text.Background = Brushes.Transparent;
        _text.Options.EnableHyperlinks = false; _text.Options.EnableEmailHyperlinks = false;
        _text.FontSize = _defaultTextSize;
        Content = _root;
        Loaded += async (_, _) => await LoadAsync();
        Closed += (_, _) => { _closed = true; _load?.Cancel(); _load?.Dispose(); _payload = null; _image = null; _imageScroll = null; _text.Text = ""; _source = ""; ThemeService.Changed -= ThemeChanged; Localizer.Changed -= LanguageChanged; RestoreActivity.Changed -= RestoreChanged; };
        PreviewKeyDown += KeyDownHandler;
        PreviewMouseWheel += (_, e) => { if (Keyboard.Modifiers == ModifierKeys.Control) { e.Handled = true; Zoom(e.Delta > 0 ? 1 : -1); } };
        ThemeService.Changed += ThemeChanged;RestoreActivity.Changed+=RestoreChanged;
        Localizer.Changed += LanguageChanged;
        LanguageChanged(null,EventArgs.Empty);
    }
    private Button IconTool(string icon, string title, Action action)
    {
        var button = Ui.IconButton(icon, title, action); button.Margin = new(4, 2, 0, 2); _tools.Children.Add(button); return button;
    }
    private Button Tool(string title, Action action)
    {
        var button = Ui.Button(title, action); button.Margin = new(6, 2, 0, 2); _tools.Children.Add(button); return button;
    }
    public void SetEntry(HistoryEntry entry, string? batchId = null)
    {
        if(RestoreActivity.IsBusy)return;_entry = entry; _batchId = batchId; if (IsLoaded) _ = LoadAsync();
    }
    private async Task LoadAsync()
    {
        var generation = ++_generation; _load?.Cancel(); _load?.Dispose(); _load = new(); var token = _load.Token;
        _payload = null; _source = _limitedSource = ""; _pdfPage = 0; _pdfPages = 0; _rotation = 0; _zoom = 1; _fitToWindow = true; _showSvgSource = false;_pdfTextMode=false;_trustedSvg=null;_svgSummary=null;
        _image = null; _imageScroll = null; _document = null; _text.Text = ""; _content.Content = Placeholder(Localizer.Text("history.preview.loading"));
        _name.Text = _entry.Title; _metadata.Text = _entry.SizeLabel; _notice.Visibility = Visibility.Collapsed; _restore.IsEnabled = false; _tools.IsEnabled = false; _tools.Visibility = Visibility.Collapsed;
        try
        {
            if (AppSession.IsDemo)
            {
                _payload = new(_entry.Id, _entry.Preview == "image" ? "image" : "text", _entry.Title, _entry.SizeBytes, null, null, null, Encoding.UTF8.GetBytes(_entry.Body));
                _source = _entry.Body; _limitedSource = _source;
                if (_entry.Preview == "image") ShowImage(Ui.Landscape()); else { SetMode(_entry.Preview == "code" ? "code" : "text"); ShowText(); }
            }
            else
            {
                if(TourHistoryClient.IsVirtual(_entry.Id)&&!GuidedTour.Active)throw new InvalidDataException("Demonstration preview expired");
                var payload = TourHistoryClient.IsVirtual(_entry.Id)?TourHistoryClient.Preview(_entry.Id,_batchId):PreviewEnvelope.Parse(await NativeBackend.InvokeBinaryAsync("get_preview", new { id = _entry.Id, batchId = _batchId }, token));
                if (generation != _generation || _closed) return;
                if (payload.EntryId.HasValue && payload.EntryId != _entry.Id) throw new InvalidDataException("预览响应与当前记录不一致");
                _payload = payload; _name.Text = payload.Name; _metadata.Text = FormatSize(payload.SizeBytes);
                if (payload.Renderer is "text" or "code" or "markdown" or "svg")
                {
                    _source = await Task.Run(() => Encoding.UTF8.GetString(payload.Data.Span), token); _limitedSource = Limit(_source);
                    if (generation != _generation || _closed) return;
                }
                var sampleDocument=TourHistoryClient.IsVirtual(_entry.Id)?TourHistoryClient.Document(_entry.Id):default;
                if(sampleDocument.ValueKind==JsonValueKind.Object)ShowDocument(PreviewRenderers.BundledDocument(sampleDocument));
                else switch (payload.Renderer)
                {
                    case "image":
                        var bitmap = await Task.Run(() => payload.Kind == "image" ? PreviewRenderers.Rgba(payload.Data.Span, payload.Width!.Value, payload.Height!.Value) : PreviewRenderers.Image(payload.Data), token);
                        if (generation != _generation || _closed) return; ShowImage(bitmap); break;
                    case "svg": _svgSummary=SvgExternalPolicy.Summarize(payload.Data);ShowSvg(); break;
                    case "pdf":
                        using (var stream = PreviewRenderers.Stream(payload.Data)) _pdfPages = await Task.Run(() => PDFtoImage.Conversion.GetPageCount(stream), token);
                        if (generation != _generation || _closed) return; await ShowPdfPage(); break;
                    case "docx":
                        await Dispatcher.Yield(DispatcherPriority.Background); if (generation != _generation || _closed) return; ShowDocument(PreviewRenderers.Docx(payload.Data)); break;
                    case "text": case "code": case "markdown": SetMode(payload.Renderer); ShowText(); break;
                    default: _content.Content = Placeholder(Localizer.Text("history.preview.unsupported")); break;
                }
            }
            _restore.IsEnabled = !RestoreActivity.IsBusy; _tools.IsEnabled = true; _tools.Visibility = Visibility.Visible; UpdateToolbar();
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { }
        catch (Exception error) { if (generation == _generation && !_closed) ShowFailure(error); }
    }
    private void ShowFailure(Exception error)
    {
        _tools.IsEnabled = false; _restore.IsEnabled = true;
        var message = Ui.Text(Localizer.Text("history.preview.error"), 16); message.FontWeight = FontWeights.SemiBold;
        var detail = Ui.Text(error.Message, 12, "Secondary"); detail.MaxWidth = 470;
        var retry = Ui.Button(Localizer.Text("history.retry"), () => _ = LoadAsync()); retry.HorizontalAlignment = HorizontalAlignment.Left;
        var body = Ui.Stack(Orientation.Vertical, 12, PlaceholderImage(), message, detail, retry); body.Margin = new(30); _content.Content = body;
    }
    private static bool IsText(PreviewPayload payload) => payload.Renderer is "text" or "code" or "markdown";
    private void SetMode(string id)
    {
        _renderingText = true; _mode.SelectedItem = _mode.Items.OfType<ComboBoxItem>().First(x => x.Tag as string == id); _renderingText = false;
    }
    private void ShowText(bool svgSource = false)
    {
        if (_payload == null) return;
        var mode = svgSource ? "code" : (_mode.SelectedItem as ComboBoxItem)?.Tag as string ?? "text";
        _image = null; _imageScroll = null; _document = null;
        _notice.Text = _source.Length > _limitedSource.Length ? Localizer.Text("history.preview.truncated") : "";
        _notice.Visibility = _notice.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        if (mode == "markdown") ShowDocument(PreviewRenderers.Markdown(_limitedSource));
        else
        {
            _text.Text = _limitedSource; _text.ShowLineNumbers = mode == "code" && _limitedSource.Count(x => x == '\n') < 5000;
            if (mode == "code") _text.FontFamily = new("Cascadia Code, Consolas"); else _text.SetResourceReference(Control.FontFamilyProperty, "ReadingFont");
            _text.SyntaxHighlighting = mode == "code" && _limitedSource.Length <= 64 * 1024 ? HighlightingManager.Instance.GetDefinitionByExtension(svgSource ? ".xml" : Path.GetExtension(_payload.Name)) ?? HighlightingManager.Instance.GetDefinition("C#") : null;
            _content.Content = _text;
        }
        UpdateToolbar(); Search();
    }
    private void ShowDocument(FlowDocument document)
    {
        _document = new FlowDocumentScrollViewer { Document = document, IsToolBarVisible = false, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled, Zoom = _zoom * 100 };
        _content.Content = _document;
    }
    private void ShowSvg()
    {
        if (_payload == null) return;
        try { ShowImage(PreviewRenderers.Svg(_trustedSvg??_payload.Data.ToArray(),_trustedSvg!=null)); }
        catch (Exception error) { _showSvgSource = true; ShowText(true); _notice.Text = Localizer.Text("history.preview.svgFallback") + " · " + error.Message; _notice.Visibility = Visibility.Visible; }
    }
    private void ShowImage(ImageSource source)
    {
        _document = null; _image = new Image { Source = source, Stretch = Stretch.Uniform, Margin = new(18), RenderTransformOrigin = new(.5, .5) };
        RenderOptions.SetBitmapScalingMode(_image, BitmapScalingMode.HighQuality);
        _imageScroll = new ScrollViewer { Content = _image, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
        _imageScroll.SizeChanged += (_, _) => UpdateImageLayout(); _content.Content = _imageScroll; UpdateImageLayout(); UpdateToolbar();
    }
    private async Task ShowPdfPage()
    {
        var payload = _payload; if (payload?.Renderer != "pdf") return;
        var generation = _generation; var token = _load!.Token; var page = _pdfPage;
        _pdfPrevious.IsEnabled = _pdfNext.IsEnabled = false;
        try
        {
            if(_pdfTextMode){await ShowPdfText(payload,generation,page,token);return;}
            _notice.Visibility=Visibility.Collapsed;
            await _pdfRender.WaitAsync(token);
            BitmapSource bitmap;
            try
            {
                bitmap = await Task.Run(() =>
                {
                    token.ThrowIfCancellationRequested(); using var stream = PreviewRenderers.Stream(payload.Data);
                    // Bound raster memory even for PDFs with oversized media boxes.
                    using var rendered = PDFtoImage.Conversion.ToImage(stream, page, options: new PDFtoImage.RenderOptions(Width: 1500, Height: 2100, WithAspectRatio: true));
                    using var encoded = rendered.Encode(SKEncodedImageFormat.Png, 100);
                    return PreviewRenderers.Image(encoded.ToArray());
                }, token);
            }
            finally { _pdfRender.Release(); }
            if (generation != _generation || _closed || page != _pdfPage) return; ShowImage(bitmap); UpdateToolbar();
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { }
        catch (Exception error) { if (generation == _generation && !_closed) ShowFailure(error); }
    }

    private async Task ShowPdfText(PreviewPayload payload,int generation,int page,CancellationToken token)
    {
        _content.Content=Placeholder(Localizer.Text("history.preview.loading"));
        var source=await Task.Run(()=>
        {
            token.ThrowIfCancellationRequested();using var stream=PreviewRenderers.Stream(payload.Data);
            using var document=UglyToad.PdfPig.PdfDocument.Open(stream);
            return document.GetPage(page+1).Text;
        },token);
        if(generation!=_generation||_closed||page!=_pdfPage||!_pdfTextMode)return;
        _source=source;_limitedSource=Limit(source);_text.Text=_limitedSource;_text.ShowLineNumbers=false;_text.SyntaxHighlighting=null;_text.SetResourceReference(Control.FontFamilyProperty,"ReadingFont");
        _image=null;_imageScroll=null;_document=null;_content.Content=_text;
        _notice.Text=Localizer.Choose("PDF 可选择文本视图；此处的排版顺序可能与页面图像不同。","Selectable PDF text; reading order can differ from the page image.");_notice.Visibility=Visibility.Visible;UpdateToolbar();Search();
    }
    private async Task GrantSvgAsync()
    {
        var payload=_payload;var summary=_svgSummary;if(payload?.Renderer!="svg"||summary==null)return;
        if(_trustedSvg!=null){_trustedSvg=null;ShowSvg();return;}
        if(!summary.CanGrant)
        {
            DemoDialog.Inform(this,Localizer.Text("history.preview.svgTrustRejectedTitle"),Localizer.Text("history.preview.svgTrustRejectedMessage")+"\n\n"+string.Join("\n",summary.RejectedTargets));return;
        }
        if(!DemoDialog.Confirm(this,Localizer.Text("history.preview.svgTrustAlertTitle"),Localizer.Text("history.preview.svgTrustAlertMessage")+"\n\n"+string.Join("\n",summary.AllowedOrigins),Localizer.Text("history.preview.svgTrustAlertAllow")))return;
        var generation=_generation;var token=_load!.Token;_svgTrust.IsEnabled=false;
        try
        {
            var materialized=await SvgExternalPolicy.MaterializeAsync(payload.Data,summary,token);
            if(generation!=_generation||_closed)return;
            var image=PreviewRenderers.Svg(materialized,true);
            // The grant is committed only after every image and the native
            // drawing have loaded successfully for this exact preview target.
            _trustedSvg=materialized;_showSvgSource=false;ShowImage(image);UpdateToolbar();
        }
        catch(OperationCanceledException)when(token.IsCancellationRequested){}
        catch(Exception error){if(generation==_generation&&!_closed){_trustedSvg=null;_notice.Text=error.Message;_notice.Visibility=Visibility.Visible;UpdateToolbar();}}
        finally{if(!_closed)_svgTrust.IsEnabled=true;}
    }
    private void UpdateToolbar()
    {
        var payload = _payload; if (payload == null) return;
        var text = IsText(payload) || payload.Renderer == "svg" && _showSvgSource || payload.Renderer=="pdf"&&_pdfTextMode;
        _search.Visibility = text || payload.Renderer == "docx" ? Visibility.Visible : Visibility.Collapsed;
        _mode.Visibility = IsText(payload) ? Visibility.Visible : Visibility.Collapsed;
        _wrap.Visibility = _content.Content == _text ? Visibility.Visible : Visibility.Collapsed;
        _fit.Visibility = _rotate.Visibility = _image != null ? Visibility.Visible : Visibility.Collapsed;
        _svgSource.Visibility = payload.Renderer == "svg" ? Visibility.Visible : Visibility.Collapsed;
        _pdfText.Visibility=payload.Renderer=="pdf"?Visibility.Visible:Visibility.Collapsed;
        _pdfText.Content=Localizer.Choose(_pdfTextMode?"页面图像":"可选择文本",_pdfTextMode?"Page image":"Selectable text");
        _svgTrust.Visibility=payload.Renderer=="svg"&&_svgSummary is {} summary&&(summary.Images.Count>0||summary.RejectedTargets.Count>0)?Visibility.Visible:Visibility.Collapsed;
        _svgTrust.Content=Localizer.Text(_trustedSvg!=null?"history.preview.svgTrustedExternal":_svgSummary?.RejectedTargets.Count>0?"history.preview.svgExternalBlocked":"history.preview.svgTrustExternal");
        _svgSource.Content = Localizer.Text(_showSvgSource ? "history.preview.svgVisual" : "history.preview.svgSource");
        _copy.Visibility = text || payload.Renderer is "image" or "svg" ? Visibility.Visible : Visibility.Collapsed;
        var batch = payload.Batch;
        _navigation.Visibility = batch != null ? Visibility.Visible : Visibility.Collapsed;
        _previous.IsEnabled = batch?.PreviousId != null;_next.IsEnabled = batch?.NextId != null;
        _batchLabel.Text = batch != null ? $"{batch.ItemIndex + 1} / {batch.ItemCount}" : "";
        _pdfNavigation.Visibility=payload.Renderer=="pdf"?Visibility.Visible:Visibility.Collapsed;
        _pdfPrevious.IsEnabled=_pdfPage>0;_pdfNext.IsEnabled=_pdfPage+1<_pdfPages;_pdfPageLabel.Text=$"{_pdfPage+1} / {_pdfPages}";
        _navigation.IsEnabled=_pdfNavigation.IsEnabled=!RestoreActivity.IsBusy;
        _zoomLabel.Text = text ? $"{_text.FontSize:0}px" : $"{_zoom * 100:0}%";
    }
    private void Navigate(int direction)
    {
        if(RestoreActivity.IsBusy)return;
        var id = direction < 0 ? _payload?.Batch?.PreviousId : _payload?.Batch?.NextId;
        if (id.HasValue) { _entry = new() { Id = id.Value, Title = _entry.Title }; _batchId = _payload?.Batch?.BatchId; _ = LoadAsync(); }
    }
    private void NavigatePdf(int direction)
    {
        if(RestoreActivity.IsBusy||_payload?.Renderer!="pdf")return;var page=Math.Clamp(_pdfPage+direction,0,Math.Max(0,_pdfPages-1));if(page!=_pdfPage){_pdfPage=page;_ = ShowPdfPage();}
    }
    private void UpdateImageLayout()
    {
        if (_image?.Source == null || _imageScroll == null) return;
        var sourceWidth = _image.Source.Width; var sourceHeight = _image.Source.Height;
        if (!double.IsFinite(sourceWidth) || !double.IsFinite(sourceHeight) || sourceWidth <= 0 || sourceHeight <= 0) return;
        var rotated = _rotation % 180 != 0;
        if (_fitToWindow)
        {
            var width = Math.Max(1, _imageScroll.ActualWidth - 38); var height = Math.Max(1, _imageScroll.ActualHeight - 38);
            _zoom = Math.Min(width / (rotated ? sourceHeight : sourceWidth), height / (rotated ? sourceWidth : sourceHeight));
            _zoom = Math.Max(.05, Math.Min(1, _zoom));
        }
        _image.Width = sourceWidth * _zoom; _image.Height = sourceHeight * _zoom; _image.LayoutTransform = new RotateTransform(_rotation);
        _zoomLabel.Text = $"{_zoom * 100:0}%";
    }
    private void Zoom(int direction)
    {
        if (_content.Content == _text) { _text.FontSize = Math.Clamp(_text.FontSize + direction, 12, 32); PreviewPreferences.FontSize = _text.FontSize; }
        else if (_document != null) { _zoom = Math.Clamp(_zoom + direction * .1, .5, 2.5); _document.Zoom = _zoom * 100; }
        else { _fitToWindow = false; _zoom = Math.Clamp(_zoom + direction * .1, .1, 4); UpdateImageLayout(); }
        UpdateToolbar();
    }
    private void Fit() { _fitToWindow = true; _zoom = 1; UpdateImageLayout(); }
    private void Search(bool next = false)
    {
        if (_search.Text.Length == 0) return;
        if (_content.Content == _text)
        {
            var offset = next ? _text.SelectionStart + Math.Max(1, _text.SelectionLength) : 0;
            var index = _text.Text.IndexOf(_search.Text, Math.Min(offset, _text.Text.Length), StringComparison.CurrentCultureIgnoreCase);
            if (index < 0 && offset > 0) index = _text.Text.IndexOf(_search.Text, StringComparison.CurrentCultureIgnoreCase);
            if (index < 0) return; _text.Select(index, _search.Text.Length); var line = _text.Document.GetLineByOffset(index); _text.ScrollTo(line.LineNumber, index - line.Offset + 1);
        }
        else if (_document?.Document is {} document)
        {
            for (var pointer = document.ContentStart; pointer.CompareTo(document.ContentEnd) < 0; pointer = pointer.GetNextContextPosition(LogicalDirection.Forward))
            {
                if (pointer.GetPointerContext(LogicalDirection.Forward) != TextPointerContext.Text) continue;
                var text = pointer.GetTextInRun(LogicalDirection.Forward); var index = text.IndexOf(_search.Text, StringComparison.CurrentCultureIgnoreCase);
                if (index < 0) continue; var start = pointer.GetPositionAtOffset(index); var end = start?.GetPositionAtOffset(_search.Text.Length);
                if (start != null && end != null) { _document.Selection.Select(start, end); (start.Parent as FrameworkContentElement)?.BringIntoView(); } break;
            }
        }
    }
    private void Copy()
    {
        try
        {
            GuidedTour.CheckClipboard();
            if (_content.Content == _text || _document != null && IsText(_payload!)) Clipboard.SetText(_source);
            else if (_image?.Source is BitmapSource bitmap) Clipboard.SetImage(bitmap);
            else if (_payload?.Renderer == "svg") Clipboard.SetText(_source);
        }
        catch (Exception error) { _notice.Text = error.Message; _notice.Visibility = Visibility.Visible; }
    }
    private async Task RestoreAsync()
    {
        if(RestoreActivity.IsBusy)return;
        if (AppSession.IsDemo) { var result=await AppSession.Client.RestoreAsync(_entry.Id);_notice.Text=result.Message;_notice.Visibility=Visibility.Visible;return; }
        _restore.IsEnabled = false;
        try { var id=_payload?.EntryId??_entry.Id;var result=await (id==_entry.Id&&AppSession.Client is ICheckedHistoryRestore checkedRestore?checkedRestore.RestoreEntryAsync(_entry):AppSession.Client.RestoreAsync(id));_notice.Text = result.Message; _notice.Visibility = Visibility.Visible; }
        catch (Exception error) { _notice.Text = error.Message; _notice.Visibility = Visibility.Visible; }
        finally { if (!_closed) _restore.IsEnabled = true; }
    }
    private void KeyDownHandler(object sender, KeyEventArgs e)
    {
        if (e.Key is Key.ImeProcessed or Key.DeadCharProcessed || e.IsRepeat) return;
        if (e.Key == Key.Escape) { e.Handled = true; Close(); }
        else if (Keyboard.Modifiers == ModifierKeys.Control && e.Key == Key.F) { e.Handled = true; _search.Focus(); _search.SelectAll(); }
        else if (Keyboard.Modifiers == ModifierKeys.Alt && e.SystemKey is Key.Left or Key.Right) { e.Handled = true; Navigate(e.SystemKey == Key.Left ? -1 : 1); }
        else if (e.Key == Key.Space && Keyboard.Modifiers == ModifierKeys.None && !_search.IsKeyboardFocusWithin && !_text.IsKeyboardFocusWithin && !(_document?.IsKeyboardFocusWithin ?? false) && e.OriginalSource is not Button) { e.Handled = true; Close(); }
    }
    private void RestoreChanged(object? sender,EventArgs e){_restore.IsEnabled=!RestoreActivity.IsBusy;_navigation.IsEnabled=_pdfNavigation.IsEnabled=!RestoreActivity.IsBusy;}
    private void ThemeChanged(object? sender, EventArgs e) { if (_payload != null && IsText(_payload) && _content.Content == _text) ShowText(); }
    private void LanguageChanged(object? sender,EventArgs e)
    {
        Title=Localizer.Text("history.preview.title");
        if(_root.Children.OfType<Grid>().FirstOrDefault() is {} bar&&bar.Children.OfType<TextBlock>().FirstOrDefault() is {} title)title.Text=Title;
        _search.Tag=Localizer.Text("history.preview.search");_restore.Content=Localizer.Text("history.preview.restore");
        _copy.Content=Localizer.Text("history.copyAll");_fit.Content=Localizer.Text("history.preview.fit");
        foreach(var item in _mode.Items.OfType<ComboBoxItem>())item.Content=Localizer.Text((item.Tag as string) switch{"code"=>"history.preview.codeMode","markdown"=>"history.preview.markdownMode",_=>"history.preview.textMode"});
        UpdateToolbar();
    }
    private static Image PlaceholderImage()
    {
        var image=new Image{Width=110,Height=110,Stretch=Stretch.Uniform,HorizontalAlignment=HorizontalAlignment.Center};
        image.SetResourceReference(Image.SourceProperty,"ThemePreviewPlaceholderImage");
        var style=new Style(typeof(Image));var missing=new Trigger{Property=Image.SourceProperty,Value=null};missing.Setters.Add(new Setter(VisibilityProperty,Visibility.Collapsed));style.Triggers.Add(missing);image.Style=style;return image;
    }
    private static StackPanel Placeholder(string message)
    {
        var text=Ui.Text(message,14,"Secondary");text.HorizontalAlignment=HorizontalAlignment.Center;
        var panel=Ui.Stack(Orientation.Vertical,14,PlaceholderImage(),text);panel.HorizontalAlignment=HorizontalAlignment.Center;panel.VerticalAlignment=VerticalAlignment.Center;panel.Margin=new(24);return panel;
    }

    private static string Limit(string source)
    {
        const int maximum = 256 * 1024; if (source.Length <= maximum) return source;
        var boundary = source.LastIndexOf('\n', maximum); return source[..(boundary >= maximum * .75 ? boundary : maximum)];
    }
    private static string FormatSize(long bytes) => bytes >= 1048576 ? $"{bytes / 1048576d:0.#} MB" : bytes >= 1024 ? $"{bytes / 1024d:0.#} KB" : $"{bytes} B";
}

internal static class PreviewPreferences
{
    private static double _fontSize = 15;
    private static bool _loaded;
    internal static double FontSize
    {
        get
        {
            if (_loaded) return _fontSize; _loaded = true;
            if (AppSession.IsDemo || NativeBackend.IsIsolated) return _fontSize;
            try { if (double.TryParse(File.ReadAllText(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TailSync", "wpf-preview-font.txt")), out var size)) _fontSize = Math.Clamp(size, 12, 32); } catch (IOException) { }
            return _fontSize;
        }
        set
        {
            _fontSize = Math.Clamp(value, 12, 32); if (AppSession.IsDemo || NativeBackend.IsIsolated) return;
            try { var directory = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TailSync"); Directory.CreateDirectory(directory); File.WriteAllText(Path.Combine(directory, "wpf-preview-font.txt"), _fontSize.ToString(System.Globalization.CultureInfo.InvariantCulture)); } catch (IOException) { }
        }
    }
}
