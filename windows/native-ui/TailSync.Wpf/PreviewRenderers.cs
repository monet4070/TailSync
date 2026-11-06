using System.IO;
using System.Diagnostics;
using System.IO.Compression;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Xml;
using System.Xml.Linq;
using Markdig;
using Markdig.Syntax;
using Markdig.Syntax.Inlines;
using SharpVectors.Converters;
using SharpVectors.Renderers.Wpf;
using TailSync.Native.Core;
using WpfParagraph = System.Windows.Documents.Paragraph;
using WpfSpan = System.Windows.Documents.Span;

namespace TailSync.Wpf;

internal static class PreviewRenderers
{
    private static readonly XNamespace W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    private static readonly XNamespace R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    private static readonly XNamespace A = "http://schemas.openxmlformats.org/drawingml/2006/main";
    private const long ZipBudget = 128 * 1024 * 1024;

    internal static MemoryStream Stream(ReadOnlyMemory<byte> bytes)
    {
        if (MemoryMarshal.TryGetArray(bytes, out var segment)) return new MemoryStream(segment.Array!, segment.Offset, segment.Count, false);
        return new MemoryStream(bytes.ToArray(), false);
    }
    internal static BitmapSource Rgba(ReadOnlySpan<byte> bytes, int width, int height)
    {
        if (width <= 0 || height <= 0 || (long)width * height * 4 != bytes.Length || bytes.Length > PreviewEnvelope.MaximumBytes)
            throw new InvalidDataException("图片像素数据不完整");
        var bgra = bytes.ToArray();
        for (var i = 0; i < bgra.Length; i += 4) (bgra[i], bgra[i + 2]) = (bgra[i + 2], bgra[i]);
        var bitmap = BitmapSource.Create(width, height, 96, 96, PixelFormats.Bgra32, null, bgra, checked(width * 4));
        bitmap.Freeze(); return bitmap;
    }
    internal static BitmapSource Image(ReadOnlyMemory<byte> bytes)
    {
        // Validate decoded dimensions before WIC allocates a full bitmap. The
        // compressed transport limit alone does not bound decompressed pixels.
        int width,height;
        using(var header=Stream(bytes))using(var codec=SkiaSharp.SKCodec.Create(header)??throw new InvalidDataException("图片内容无法解码"))
        {width=codec.Info.Width;height=codec.Info.Height;}
        if(width<=0||height<=0||width>16384||height>16384||(long)width*height>16*1024*1024)throw new InvalidDataException("图片超出安全像素预算");
        using var stream = Stream(bytes);
        var image = new BitmapImage(); image.BeginInit(); image.CacheOption = BitmapCacheOption.OnLoad;
        if(width>=height)image.DecodePixelWidth=Math.Min(2048,width);else image.DecodePixelHeight=Math.Min(2048,height);
        image.StreamSource = stream;
        try{image.EndInit();image.Freeze();return image;}
        catch(Exception error)when(error is NotSupportedException or FileFormatException)
        {
            // WIC installations vary in WebP support; Skia gives those files a
            // native fallback instead of requiring an installed shell codec.
            using var data=Stream(bytes);using var bitmap=SkiaSharp.SKBitmap.Decode(data)??throw new InvalidDataException("图片内容无法解码");
            using var encoded=bitmap.Encode(SkiaSharp.SKEncodedImageFormat.Png,100);
            using var png=new MemoryStream(encoded.ToArray(),false);var fallback=new BitmapImage();fallback.BeginInit();fallback.CacheOption=BitmapCacheOption.OnLoad;
            if(width>=height)fallback.DecodePixelWidth=Math.Min(2048,width);else fallback.DecodePixelHeight=Math.Min(2048,height);
            fallback.StreamSource=png;fallback.EndInit();fallback.Freeze();return fallback;
        }
    }
    internal static byte[] Png(BitmapSource image)
    {
        var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(image));
        using var stream = new MemoryStream(); encoder.Save(stream); return stream.ToArray();
    }
    internal static DrawingImage Svg(ReadOnlyMemory<byte> bytes,bool materialized=false)
    {
        if(bytes.Length>(materialized?32:8)*1024*1024)throw new InvalidDataException("SVG 超出安全图像预览大小限制");
        using var stream = Stream(bytes);
        var doc = ReadXml(stream, 32 * 1024 * 1024);
        foreach(var instruction in doc.DescendantNodes().OfType<XProcessingInstruction>().ToArray())instruction.Remove();
        if (doc.Root?.Name.LocalName != "svg") throw new InvalidDataException("SVG 根元素无效");
        if (doc.Descendants().Take(20001).Count() > 20000) throw new InvalidDataException("SVG 元素数量过多");
        long imagePixels=0;
        foreach (var node in doc.Descendants().ToArray())
        {
            if (node.Name.LocalName is "script" or "foreignObject" or "iframe" or "audio" or "video" or "animate" or "set" or "animateTransform") { node.Remove(); continue; }
            if (node.Name.LocalName == "style" && (node.Value.Contains('@') || node.Value.Contains('\\') || node.Value.Contains("/*",StringComparison.Ordinal) || UnsafeCss(node.Value))) { node.Remove(); continue; }
            foreach (var attr in node.Attributes().ToArray())
            {
                var name = attr.Name.LocalName;
                if (name.StartsWith("on", StringComparison.OrdinalIgnoreCase) || name is "base" or "srcset" || UnsafeCss(attr.Value) || attr.Value.Contains('\\') || name=="style"&&attr.Value.Contains("/*",StringComparison.Ordinal)) { attr.Remove(); continue; }
                if (name is "href" or "src" && !attr.Value.StartsWith('#'))
                {
                    if(!SafeEmbeddedImage(attr.Value)){attr.Remove();continue;}
                    var embedded=Convert.FromBase64String(attr.Value[(attr.Value.IndexOf(',')+1)..]);
                    using(var data=SkiaSharp.SKData.CreateCopy(embedded))using(var codec=SkiaSharp.SKCodec.Create(data)??throw new InvalidDataException("SVG 内嵌图片无法解码"))
                    {imagePixels+=(long)codec.Info.Width*codec.Info.Height;if(imagePixels>16*1024*1024)throw new InvalidDataException("SVG 内嵌图片总像素超过限制");}
                    if(!materialized)attr.Value="data:image/png;base64,"+Convert.ToBase64String(SvgExternalPolicy.NormalizeRaster(embedded));
                }
            }
        }
        var settings = new WpfDrawingSettings { IncludeRuntime = false, TextAsGeometry = false };
        using var reader = new FileSvgReader(settings);
        using var text = new StringReader(doc.ToString(SaveOptions.DisableFormatting));
        var drawing = reader.Read(text) ?? throw new InvalidDataException("SVG 无法绘制");
        if (drawing.CanFreeze) drawing.Freeze(); var image = new DrawingImage(drawing); if (image.CanFreeze) image.Freeze(); return image;
    }
    private static bool UnsafeCss(string value) => Regex.Matches(value, @"url\s*\(\s*['"" ]?([^)'""\s]+)", RegexOptions.IgnoreCase).Any(match => !match.Groups[1].Value.StartsWith('#')) || value.Contains("@import", StringComparison.OrdinalIgnoreCase);
    private static bool SafeEmbeddedImage(string value) => value.Length <= 6 * 1024 * 1024 && (value.StartsWith("data:image/png;base64,", StringComparison.OrdinalIgnoreCase) || value.StartsWith("data:image/jpeg;base64,", StringComparison.OrdinalIgnoreCase));

    internal static FlowDocument Markdown(string text, double fontSize = 15)
    {
        var document = Document();
        document.FontSize = fontSize;
        document.LineHeight = Math.Round(fontSize * 1.6);
        var parsed = Markdig.Markdown.Parse(text, new MarkdownPipelineBuilder().UseAdvancedExtensions().Build());
        foreach (var block in parsed) AddMarkdown(document.Blocks, block, fontSize);
        return document;
    }
    private static void AddMarkdown(BlockCollection destination, Markdig.Syntax.Block block, double fontSize = 15)
    {
        switch (block)
        {
            case HeadingBlock heading:
                var headingSize = Math.Max(fontSize + 2, Math.Round((30 - heading.Level * 2) * (fontSize / 15d)));
                var title = new WpfParagraph { FontSize = headingSize, FontWeight = FontWeights.SemiBold, Margin = new(0, 14, 0, 7), Tag = heading.Level };
                AddInline(title.Inlines, heading.Inline); destination.Add(title); break;
            case ParagraphBlock paragraph:
                var body = new WpfParagraph { Margin = new(0, 0, 0, 10) }; AddInline(body.Inlines, paragraph.Inline); destination.Add(body); break;
            case CodeBlock code:
                var codeSize = Math.Max(11, Math.Round(13 * (fontSize / 15d)));
                var pre = new WpfParagraph(new Run(code.Lines.ToString())) { FontFamily = new("Cascadia Code, Consolas"), FontSize = codeSize, Padding = new(12), Margin = new(0, 7, 0, 12), Tag = "code" };
                pre.SetResourceReference(TextElement.BackgroundProperty, "InputBrush"); destination.Add(pre); break;
            case QuoteBlock quote:
                var section = new Section { BorderThickness = new(3, 0, 0, 0), Padding = new(12, 0, 0, 0), Margin = new(0, 5, 0, 10) };
                section.SetResourceReference(System.Windows.Documents.Block.BorderBrushProperty, "AccentBrush"); foreach (var child in quote) AddMarkdown(section.Blocks, child, fontSize); destination.Add(section); break;
            case ListBlock list:
                var items = new System.Windows.Documents.List { MarkerStyle = list.IsOrdered ? TextMarkerStyle.Decimal : TextMarkerStyle.Disc, Margin = new(0, 0, 0, 10) };
                foreach (var item in list.OfType<ListItemBlock>()) { var entry = new ListItem(); foreach (var child in item) AddMarkdown(entry.Blocks, child, fontSize); items.ListItems.Add(entry); }
                destination.Add(items); break;
            case ThematicBreakBlock:
                var line = new WpfParagraph { BorderThickness = new(0, 0, 0, 1), Margin = new(0, 8, 0, 10) }; line.SetResourceReference(System.Windows.Documents.Block.BorderBrushProperty, "BorderBrush"); destination.Add(line); break;
            case Markdig.Extensions.Tables.Table markdownTable:
                var table=new System.Windows.Documents.Table{CellSpacing=0,Margin=new(0,8,0,12)};var tableRows=new TableRowGroup();table.RowGroups.Add(tableRows);
                foreach(var markdownRow in markdownTable.OfType<Markdig.Extensions.Tables.TableRow>())
                {
                    var row=new TableRow();tableRows.Rows.Add(row);
                    foreach(var markdownCell in markdownRow.OfType<Markdig.Extensions.Tables.TableCell>())
                    {
                        var cell=new TableCell{Padding=new(8),BorderThickness=new(.5)};cell.SetResourceReference(TableCell.BorderBrushProperty,"BorderBrush");
                        if(markdownRow.IsHeader)cell.FontWeight=FontWeights.SemiBold;
                        foreach(var child in markdownCell)AddMarkdown(cell.Blocks,child,fontSize);row.Cells.Add(cell);
                    }
                }
                destination.Add(table);break;
            case HtmlBlock html:
                // Treat embedded HTML as text; there is no script/HTML host.
                destination.Add(new WpfParagraph(new Run(html.Lines.ToString()))); break;
            case ContainerBlock container:
                foreach (var child in container) AddMarkdown(destination, child, fontSize); break;
            case LeafBlock leaf:
                var fallback = new WpfParagraph(); AddInline(fallback.Inlines, leaf.Inline); destination.Add(fallback); break;
        }
    }
    private static void AddInline(InlineCollection destination, ContainerInline? container)
    {
        if (container == null) return;
        foreach (var inline in container)
        {
            switch (inline)
            {
                case LiteralInline literal: destination.Add(new Run(literal.Content.ToString())); break;
                case CodeInline code:
                    var run = new Run(code.Content) { FontFamily = new("Cascadia Code, Consolas") }; run.SetResourceReference(TextElement.BackgroundProperty, "InputBrush"); destination.Add(run); break;
                case EmphasisInline emphasis:
                    var span = new WpfSpan(); if (emphasis.DelimiterChar == '~') span.TextDecorations = TextDecorations.Strikethrough;
                    else if (emphasis.DelimiterCount >= 2) span.FontWeight = FontWeights.Bold; else span.FontStyle = FontStyles.Italic;
                    AddInline(span.Inlines, emphasis); destination.Add(span); break;
                case LinkInline link when link.IsImage:
                    var alt = new WpfSpan(); AddInline(alt.Inlines, link);
                    if (link.Url is {} data && SafeEmbeddedImage(data))
                    {
                        try { var image = new Image { Source = Image(Convert.FromBase64String(data[(data.IndexOf(',') + 1)..])), MaxWidth = 560, MaxHeight = 600, Stretch = Stretch.Uniform }; destination.Add(new InlineUIContainer(image)); }
                        catch { destination.Add(alt); }
                    }
                    else { destination.Add(new Run("[图片] ")); destination.Add(alt); }
                    break;
                case LinkInline link:
                    var label = WebLink(link.Url); label.SetResourceReference(TextElement.ForegroundProperty, "AccentBrush"); AddInline(label.Inlines, link); destination.Add(label); break;
                case LineBreakInline: destination.Add(new LineBreak()); break;
                case HtmlInline html: destination.Add(new Run(html.Tag)); break;
                case ContainerInline nested: AddInline(destination, nested); break;
                default: destination.Add(new Run(inline.ToString() ?? "")); break;
            }
        }
    }

    internal static FlowDocument Docx(ReadOnlyMemory<byte> bytes)
    {
        using var stream = Stream(bytes); using var archive = new ZipArchive(stream, ZipArchiveMode.Read);
        if (archive.Entries.Count > 4096 || archive.Entries.Sum(entry => entry.Length) > ZipBudget) throw new InvalidDataException("DOCX 解压内容超过限制");
        var entry = archive.GetEntry("word/document.xml") ?? throw new InvalidDataException("DOCX 缺少文档内容");
        if (entry.Length > 8 * 1024 * 1024) throw new InvalidDataException("DOCX 文档 XML 过大");
        using var content = entry.Open(); var doc = ReadXml(content, 8 * 1024 * 1024);
        var relationships = new Dictionary<string, string>();
        var rels = archive.GetEntry("word/_rels/document.xml.rels");
        if (rels != null && rels.Length < 1024 * 1024)
        {
            using var relationshipData = rels.Open();
            foreach (var relation in ReadXml(relationshipData, 1024 * 1024).Root?.Elements() ?? [])
            {
                if ((string?)relation.Attribute("Id") is not {} id || (string?)relation.Attribute("Target") is not {} target)continue;
                if((string?)relation.Attribute("TargetMode")!="External"&&target.StartsWith("media/",StringComparison.Ordinal)&&!target.Contains(".."))relationships[id]="word/"+target;
                else if((string?)relation.Attribute("Type") is {} relationType&&relationType.EndsWith("/hyperlink",StringComparison.Ordinal)&&SafeWebUri(target) is {} uri)relationships[id]=uri.AbsoluteUri;
            }
        }
        var document = Document();
        foreach (var element in doc.Root?.Element(W + "body")?.Elements() ?? [])
            AddDocxBlock(document.Blocks, element, archive, relationships);
        return document;
    }
    private static void AddDocxBlock(BlockCollection blocks, XElement element, ZipArchive archive, Dictionary<string, string> relationships)
    {
        if (element.Name == W + "p")
        {
            var paragraph = new WpfParagraph { Margin = new(0, 0, 0, 8) };
            var properties = element.Element(W + "pPr");
            var style = (string?)properties?.Element(W + "pStyle")?.Attribute(W + "val");
            if (style?.StartsWith("Heading", StringComparison.OrdinalIgnoreCase) == true) { paragraph.FontSize = 22; paragraph.FontWeight = FontWeights.SemiBold; paragraph.Margin = new(0, 14, 0, 8); }
            paragraph.TextAlignment = (string?)properties?.Element(W + "jc")?.Attribute(W + "val") switch { "center" => TextAlignment.Center, "right" => TextAlignment.Right, "both" => TextAlignment.Justify, _ => TextAlignment.Left };
            foreach (var item in element.Descendants().Where(x => x.Name == W + "r"))
            {
                var span = new WpfSpan(); var format = item.Element(W + "rPr");
                if (Enabled(format?.Element(W + "b"))) span.FontWeight = FontWeights.Bold;
                if (Enabled(format?.Element(W + "i"))) span.FontStyle = FontStyles.Italic;
                if (format?.Element(W + "u") != null) span.TextDecorations = TextDecorations.Underline;
                if (format?.Element(W + "strike") != null) span.TextDecorations = TextDecorations.Strikethrough;
                if (double.TryParse((string?)format?.Element(W + "sz")?.Attribute(W + "val"), out var size)) span.FontSize = Math.Clamp(size / 2 * 96 / 72, 8, 96);
                var color = (string?)format?.Element(W + "color")?.Attribute(W + "val");
                if (color is { Length: 6 } && color.All(Uri.IsHexDigit)) span.Foreground = (Brush)new BrushConverter().ConvertFromString("#" + color)!;
                foreach (var child in item.Elements())
                {
                    if (child.Name == W + "t") span.Inlines.Add(new Run(child.Value));
                    else if (child.Name == W + "br" || child.Name == W + "cr") span.Inlines.Add(new LineBreak());
                    else if (child.Name == W + "tab") span.Inlines.Add(new Run("\t"));
                    else if (child.Name == W + "drawing")
                    {
                        var embed = (string?)child.Descendants(A + "blip").FirstOrDefault()?.Attribute(R + "embed");
                        if (embed != null && relationships.TryGetValue(embed, out var target) && archive.GetEntry(target) is {} media && media.Length <= 16 * 1024 * 1024)
                        {
                            using var data = media.Open(); using var output = new MemoryStream(); data.CopyTo(output);
                            try { var image = new Image { Source = Image(output.ToArray()), MaxWidth = 560, MaxHeight = 700, Stretch = Stretch.Uniform }; span.Inlines.Add(new InlineUIContainer(image)); }
                            catch { span.Inlines.Add(new Run("[无法显示的图片]")); }
                        }
                    }
                }
                if(item.Parent?.Name==W+"hyperlink"&&(string?)item.Parent.Attribute(R+"id") is {} linkId&&relationships.TryGetValue(linkId,out var url)&&SafeWebUri(url)!=null)
                {var link=WebLink(url);link.Inlines.Add(span);paragraph.Inlines.Add(link);}
                else paragraph.Inlines.Add(span);
            }
            blocks.Add(paragraph);
        }
        else if (element.Name == W + "tbl")
        {
            var table = new System.Windows.Documents.Table { CellSpacing = 0, Margin = new(0, 8, 0, 12) }; var rows = new TableRowGroup(); table.RowGroups.Add(rows);
            foreach (var row in element.Elements(W + "tr"))
            {
                var tableRow = new TableRow(); rows.Rows.Add(tableRow);
                foreach (var cell in row.Elements(W + "tc"))
                {
                    var tableCell = new TableCell { Padding = new(7), BorderThickness = new(0.5) }; tableCell.SetResourceReference(TableCell.BorderBrushProperty, "BorderBrush");
                    var span = (string?)cell.Element(W + "tcPr")?.Element(W + "gridSpan")?.Attribute(W + "val"); if (int.TryParse(span, out var columns)) tableCell.ColumnSpan = Math.Clamp(columns, 1, 64);
                    foreach (var child in cell.Elements()) AddDocxBlock(tableCell.Blocks, child, archive, relationships);
                    tableRow.Cells.Add(tableCell);
                }
            }
            blocks.Add(table);
        }
    }
    private static bool Enabled(XElement? element) => element != null && (string?)element.Attribute(W + "val") is not ("0" or "false" or "off");
    private static Uri? SafeWebUri(string? target)=>Uri.TryCreate(target,UriKind.Absolute,out var uri)&&uri.Scheme is "https" or "http"&&uri.UserInfo.Length==0?uri:null;
    private static WpfSpan WebLink(string? target)
    {
        if(SafeWebUri(target) is not {} uri)return new WpfSpan();
        var link=new Hyperlink{NavigateUri=uri};
        link.RequestNavigate+=(_,args)=>
        {
            args.Handled=true;
            if(SafeWebUri(args.Uri.AbsoluteUri) is {} safe)
                try{Process.Start(new ProcessStartInfo(safe.AbsoluteUri){UseShellExecute=true});}catch(Exception error)when(error is System.ComponentModel.Win32Exception or InvalidOperationException){}
        };
        return link;
    }
    private static XDocument ReadXml(Stream stream, long maximum)
    {
        using var reader = XmlReader.Create(stream, new XmlReaderSettings { DtdProcessing = DtdProcessing.Prohibit, XmlResolver = null, MaxCharactersInDocument = maximum });
        return XDocument.Load(reader, LoadOptions.PreserveWhitespace);
    }
    private static FlowDocument Document()
    {
        var document = new FlowDocument { PagePadding = new(24), FontSize = 15, LineHeight = 24 };
        TextOptions.SetTextFormattingMode(document, TextFormattingMode.Display);
        TextOptions.SetTextRenderingMode(document, TextRenderingMode.ClearType);
        TextOptions.SetTextHintingMode(document, TextHintingMode.Fixed);
        RenderOptions.SetClearTypeHint(document, ClearTypeHint.Enabled);
        document.SetResourceReference(FlowDocument.BackgroundProperty, "SurfaceBrush");
        document.SetResourceReference(TextElement.FontFamilyProperty, "ReadingFont"); document.SetResourceReference(TextElement.ForegroundProperty, "PrimaryBrush"); return document;
    }
    // Only the authenticated, checksum-verified bundled tour model enters here.
    // DOCX continues through the ZIP/XML renderer above.
    internal static FlowDocument BundledDocument(System.Text.Json.JsonElement model)
    {
        var document = Document();
        foreach (var block in model.GetProperty("blocks").EnumerateArray())
        {
            if (block.String("kind") != "paragraph") continue;
            var paragraph = new WpfParagraph();
            foreach (var item in block.GetProperty("paragraph").GetProperty("runs").EnumerateArray())
            {
                var run = new Run(item.String("text")); var style = item.Object("style");
                if (style.Bool("bold")) run.FontWeight = FontWeights.Bold;
                if (style.Bool("italic")) run.FontStyle = FontStyles.Italic;
                if (style.Bool("underline")) run.TextDecorations = TextDecorations.Underline;
                paragraph.Inlines.Add(run);
            }
            document.Blocks.Add(paragraph);
        }
        return document;
    }
}
