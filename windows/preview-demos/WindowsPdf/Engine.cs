using Windows.Data.Pdf;
using Windows.Storage.Streams;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace TailSync.PreviewDemo;

public static class EngineFactory { public static IPreviewEngine Create() => new WindowsPdfEngine(); }

public sealed class WindowsPdfEngine : IPreviewEngine
{
    private PdfDocument? _document;
    private InMemoryRandomAccessStream? _input;
    public string Name => "Windows PDF";
    public string Version => "Windows.Data.Pdf / " + Environment.OSVersion.Version;
    public string[] Extensions => [".pdf"];
    public bool SupportsText => false;
    public async Task<DocumentInfo> OpenAsync(byte[] bytes, string extension)
    {
        Close();
        _input = new InMemoryRandomAccessStream();
        using (var writer = new DataWriter(_input))
        {
            writer.WriteBytes(bytes); await writer.StoreAsync(); writer.DetachStream();
        }
        _input.Seek(0);
        _document = await PdfDocument.LoadFromStreamAsync(_input);
        return new(checked((int)_document.PageCount), false);
    }
    public async Task<RenderFrame> RenderAsync(int pageIndex, int pixelWidth)
    {
        using var page = (_document ?? throw new InvalidOperationException("No document.")).GetPage((uint)pageIndex);
        var (w, h) = Limits.Dimensions(page.Size.Width, page.Size.Height, pixelWidth);
        using var stream = new InMemoryRandomAccessStream();
        await page.RenderToStreamAsync(stream, new PdfPageRenderOptions { DestinationWidth = (uint)w, DestinationHeight = (uint)h });
        stream.Seek(0);
        using var reader = new DataReader(stream);
        await reader.LoadAsync(checked((uint)stream.Size));
        var encoded = new byte[checked((int)stream.Size)]; reader.ReadBytes(encoded);
        using var memory = new MemoryStream(encoded, false);
        var decoded = BitmapDecoder.Create(memory, BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.OnLoad).Frames[0];
        var bgra = new FormatConvertedBitmap(decoded, PixelFormats.Bgra32, null, 0);
        Limits.CheckPixels(bgra.PixelWidth, bgra.PixelHeight);
        var pixels = new byte[checked(bgra.PixelWidth * bgra.PixelHeight * 4)];
        bgra.CopyPixels(pixels, bgra.PixelWidth * 4, 0);
        return new(pixels, bgra.PixelWidth, bgra.PixelHeight);
    }
    public Task<string> TextAsync(int pageIndex) => throw new NotSupportedException("Windows.Data.Pdf does not expose text extraction.");
    public void Close() { _document = null; _input?.Dispose(); _input = null; }
    public void Dispose() => Close();
}
