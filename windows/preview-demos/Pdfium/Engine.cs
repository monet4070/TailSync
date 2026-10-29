using System.Runtime.InteropServices;
using System.Text;

namespace TailSync.PreviewDemo;

public static class EngineFactory { public static IPreviewEngine Create() => new PdfiumEngine(); }

public sealed class PdfiumEngine : IPreviewEngine
{
    private IntPtr _document;
    private GCHandle _bytes;
    private bool _initialized;
    public string Name => "PDFium";
    public string Version => "bblanchon.PDFium.Win32 158.0.8111";
    public string[] Extensions => [".pdf"];
    public bool SupportsText => true;

    public Task<DocumentInfo> OpenAsync(byte[] bytes, string extension)
    {
        Close();
        if (!_initialized) { Native.FPDF_InitLibrary(); _initialized = true; }
        _bytes = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        _document = Native.FPDF_LoadMemDocument64(_bytes.AddrOfPinnedObject(), (nuint)bytes.Length, null);
        if (_document == IntPtr.Zero) { var error = Native.FPDF_GetLastError(); Close(); throw new InvalidDataException($"PDFium load error {error} (3: format, 4: password)."); }
        return Task.FromResult(new DocumentInfo(Native.FPDF_GetPageCount(_document), false));
    }
    public Task<RenderFrame> RenderAsync(int pageIndex, int pixelWidth)
    {
        var page = Native.FPDF_LoadPage(_document, pageIndex);
        if (page == IntPtr.Zero) throw new InvalidDataException("PDFium could not load the page.");
        try
        {
            var (w, h) = Limits.Dimensions(Native.FPDF_GetPageWidthF(page), Native.FPDF_GetPageHeightF(page), pixelWidth);
            var pixels = new byte[checked(w * h * 4)];
            var pinned = GCHandle.Alloc(pixels, GCHandleType.Pinned);
            try
            {
                var bitmap = Native.FPDFBitmap_CreateEx(w, h, 4, pinned.AddrOfPinnedObject(), w * 4); // BGRA
                if (bitmap == IntPtr.Zero) throw new InvalidOperationException("PDFium bitmap allocation failed.");
                try
                {
                    Native.FPDFBitmap_FillRect(bitmap, 0, 0, w, h, 0xffffffff);
                    Native.FPDF_RenderPageBitmap(bitmap, page, 0, 0, w, h, 0, 1 | 0x200); // annotations + bounded image cache
                }
                finally { Native.FPDFBitmap_Destroy(bitmap); }
            }
            finally { pinned.Free(); }
            return Task.FromResult(new RenderFrame(pixels, w, h));
        }
        finally { Native.FPDF_ClosePage(page); }
    }
    public Task<string> TextAsync(int pageIndex)
    {
        var page = Native.FPDF_LoadPage(_document, pageIndex);
        if (page == IntPtr.Zero) throw new InvalidDataException("PDFium could not load the page.");
        try
        {
            var textPage = Native.FPDFText_LoadPage(page);
            if (textPage == IntPtr.Zero) throw new InvalidDataException("PDFium could not load the text layer.");
            try
            {
                int count = Math.Clamp(Native.FPDFText_CountChars(textPage), 0, Limits.MaxTextChars);
                var buffer = new ushort[count + 1];
                int written = Native.FPDFText_GetText(textPage, 0, count, buffer);
                return Task.FromResult(new string(buffer.Take(Math.Max(0, written - 1)).Select(x => (char)x).ToArray()));
            }
            finally { Native.FPDFText_ClosePage(textPage); }
        }
        finally { Native.FPDF_ClosePage(page); }
    }
    public void Close()
    {
        if (_document != IntPtr.Zero) { Native.FPDF_CloseDocument(_document); _document = IntPtr.Zero; }
        if (_bytes.IsAllocated) _bytes.Free();
    }
    public void Dispose() { Close(); if (_initialized) { Native.FPDF_DestroyLibrary(); _initialized = false; } }

    private static class Native
    {
        private const string Dll = "pdfium";
        [DllImport(Dll)] internal static extern void FPDF_InitLibrary();
        [DllImport(Dll)] internal static extern void FPDF_DestroyLibrary();
        [DllImport(Dll)] internal static extern IntPtr FPDF_LoadMemDocument64(IntPtr bytes, nuint size, [MarshalAs(UnmanagedType.LPStr)] string? password);
        [DllImport(Dll)] internal static extern uint FPDF_GetLastError();
        [DllImport(Dll)] internal static extern int FPDF_GetPageCount(IntPtr doc);
        [DllImport(Dll)] internal static extern IntPtr FPDF_LoadPage(IntPtr doc, int page);
        [DllImport(Dll)] internal static extern void FPDF_ClosePage(IntPtr page);
        [DllImport(Dll)] internal static extern void FPDF_CloseDocument(IntPtr doc);
        [DllImport(Dll)] internal static extern float FPDF_GetPageWidthF(IntPtr page);
        [DllImport(Dll)] internal static extern float FPDF_GetPageHeightF(IntPtr page);
        [DllImport(Dll)] internal static extern IntPtr FPDFBitmap_CreateEx(int w, int h, int format, IntPtr pixels, int stride);
        [DllImport(Dll)] internal static extern void FPDFBitmap_Destroy(IntPtr bitmap);
        [DllImport(Dll)] internal static extern void FPDFBitmap_FillRect(IntPtr bitmap, int left, int top, int w, int h, uint color);
        [DllImport(Dll)] internal static extern void FPDF_RenderPageBitmap(IntPtr bitmap, IntPtr page, int x, int y, int w, int h, int rotate, int flags);
        [DllImport(Dll)] internal static extern IntPtr FPDFText_LoadPage(IntPtr page);
        [DllImport(Dll)] internal static extern void FPDFText_ClosePage(IntPtr page);
        [DllImport(Dll)] internal static extern int FPDFText_CountChars(IntPtr page);
        [DllImport(Dll)] internal static extern int FPDFText_GetText(IntPtr page, int start, int count, [Out] ushort[] buffer);
    }
}
