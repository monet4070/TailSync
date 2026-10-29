using MuPDF.NET;

namespace TailSync.PreviewDemo;

public static class EngineFactory { public static IPreviewEngine Create() => new MuPdfEngine(); }

public sealed class MuPdfEngine : IPreviewEngine
{
    private Document? _document;
    public string Name => "MuPDF";
    public string Version => "MuPDF.NET 3.28.2.4 / MuPDF.NativeAssets 1.28.2";
    public string[] Extensions => [".pdf", ".docx", ".xlsx", ".pptx"];
    public bool SupportsText => true;
    public Task<DocumentInfo> OpenAsync(byte[] bytes, string extension)
    {
        Close();
        // Native wrapper uses null for an absent resource archive despite its non-null annotation.
        _document = new Document(bytes, extension.TrimStart('.'), width: 595, height: 842, fontSize: 11f, archive: null!);
        return Task.FromResult(new DocumentInfo(_document.PageCount, _document.IsReflowable));
    }
    public Task<RenderFrame> RenderAsync(int pageIndex, int pixelWidth)
    {
        using var page = (_document ?? throw new InvalidOperationException("No document.")).LoadPage(pageIndex);
        var (w, _) = Limits.Dimensions(page.Width, page.Height, pixelWidth);
        float scale = (float)((w - 0.001) / page.Width); // avoid a rounding-induced extra column
        using var pixmap = page.GetPixmap(matrix: new Matrix(scale, scale), alpha: false);
        Limits.CheckPixels(pixmap.Width, pixmap.Height);
        if (pixmap.N != 3) throw new InvalidDataException("Expected an RGB MuPDF pixmap.");
        var rgb = pixmap.Samples;
        var bgra = new byte[checked(pixmap.Width * pixmap.Height * 4)];
        for (int y = 0; y < pixmap.Height; y++)
            for (int x = 0; x < pixmap.Width; x++)
            {
                int input = y * pixmap.Stride + x * 3, output = (y * pixmap.Width + x) * 4;
                bgra[output] = rgb[input + 2]; bgra[output + 1] = rgb[input + 1];
                bgra[output + 2] = rgb[input]; bgra[output + 3] = 255;
            }
        return Task.FromResult(new RenderFrame(bgra, pixmap.Width, pixmap.Height));
    }
    public Task<string> TextAsync(int pageIndex)
    {
        using var page = (_document ?? throw new InvalidOperationException("No document.")).LoadPage(pageIndex);
        string text = page.GetText("text");
        return Task.FromResult(text.Length <= Limits.MaxTextChars ? text : text[..Limits.MaxTextChars]);
    }
    public void Close() { _document?.Dispose(); _document = null; }
    public void Dispose() => Close();
}
