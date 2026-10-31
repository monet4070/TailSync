using System.Buffers.Binary;
using System.IO;
using System.IO.Compression;
using System.Text;
using System.Text.Json;
using System.Windows.Documents;
using TailSync.Native.Core;

namespace TailSync.Wpf;

// Real native renderers and query state, synthetic inputs only. No backend,
// production configuration, clipboard writes, or network access are needed.
internal static class HistoryPreviewChecks
{
    internal static async Task<IReadOnlyList<string>> RunAsync()
    {
        var checks = new List<string>();
        void Check(bool value, string label) { if (!value) throw new InvalidOperationException(label); checks.Add(label); }
        void Reject(Action operation, string label)
        {
            try { operation(); } catch (Exception error) when (error is InvalidDataException or JsonException or ArgumentException or DecoderFallbackException or System.Xml.XmlException) { checks.Add(label); return; }
            throw new InvalidOperationException("Accepted invalid input: " + label);
        }

        var restoreGate=new TaskCompletionSource<ActionResult>();var competingCalled=false;
        var restoring=RestoreActivity.RunAsync(()=>restoreGate.Task);
        Check(RestoreActivity.IsBusy,"restore activity is visible across all native windows");
        var competing=await RestoreActivity.RunAsync(()=>{competingCalled=true;return Task.FromResult(new ActionResult(true,""));});
        Check(!competing.Success&&!competingCalled,"a concurrent restore is rejected rather than queued with a stale target");
        restoreGate.SetResult(new(true,""));await restoring;
        Check(!RestoreActivity.IsBusy,"restore ownership clears after completion");
        try{await RestoreActivity.RunAsync(()=>throw new InvalidOperationException("synthetic failure"));}catch(InvalidOperationException){}
        Check(!RestoreActivity.IsBusy,"restore ownership also clears after a backend failure");
        var delivery=new WarningDelivery();var toasts=0;var acknowledgements=0;
        var warning=JsonSerializer.SerializeToElement(new{id=1,kind="new_unknown_warning",peer="peer",occurred_at_ms=10});
        await delivery.DeliverAsync(warning,false,_=>toasts++,id=>{acknowledgements++;return Task.FromResult(true);});
        Check(toasts==0&&acknowledgements==0,"a warning with no visible history window is neither shown nor consumed");
        await delivery.DeliverAsync(warning,true,_=>toasts++,id=>{acknowledgements++;throw new IOException("transient ack failure");});
        await delivery.DeliverAsync(warning,true,_=>toasts++,id=>{acknowledgements++;return Task.FromResult(true);});
        await delivery.DeliverAsync(warning,true,_=>toasts++,id=>{acknowledgements++;return Task.FromResult(true);});
        Check(toasts==1&&acknowledgements==2,"failed warning acknowledgements retry without duplicate toasts or consuming newer warnings");
        var warningTwo=JsonSerializer.SerializeToElement(new{id=2,kind="capture_failed",peer="peer",occurred_at_ms=10});
        var ackGate=new TaskCompletionSource<bool>();
        var pendingAck=delivery.DeliverAsync(warningTwo,true,_=>toasts++,id=>ackGate.Task);
        await delivery.DeliverAsync(warningTwo,true,_=>toasts++,id=>throw new InvalidOperationException("duplicate ack"));
        ackGate.SetResult(true);await pendingAck;
        Check(toasts==2,"warning identity, rather than timestamp, distinguishes simultaneous warnings and multiple windows");
        Check(!WarningCoordinator.Message(warning).Contains("new_unknown_warning"),"unknown warning kinds use a friendly localized fallback");

        var viewModel = new ShellViewModel(new RacingClient());
        viewModel.Keyword = "slow"; var slow = viewModel.RefreshAsync();
        viewModel.Keyword = "fast"; var fast = viewModel.RefreshAsync();
        await Task.WhenAll(slow, fast);
        Check(viewModel.Entries.Single().Id == 2, "a stale response cannot replace a newer history search");
        var paged = new ShellViewModel(new PagedClient()); await paged.RefreshAsync();
        Check(paged.Entries.Count == 30 && paged.HasNext && !paged.HasPrevious && paged.PageLabel == "1 / 3", "first page exposes correct bounded paging controls");
        await paged.GoToPageAsync(2);
        Check(paged.Entries.Count == 5 && !paged.HasNext && paged.HasPrevious, "last page has only the remaining records");
        await paged.GoToPageAsync(9);
        Check(paged.PageIndex == 2 && paged.Entries.Count == 5, "an invalidated page corrects to a live last page");
        var batch = new ShellViewModel(new BatchClient()); await batch.RefreshAsync();
        Check(batch.Entries.Count == 5 && batch.VisibleEntries.Count == 3 && batch.VisibleEntries[0].ShowBatchHeader, "collapsed file batch preserves all records and displays its first three");
        batch.ToggleBatch("batch"); Check(batch.VisibleEntries.Count == 5, "expanded file batch reveals every item");
        batch.SelectedEntry = batch.VisibleEntries[^1]; batch.ToggleBatch("batch");
        Check(batch.VisibleEntries.Count == 3 && batch.SelectedEntry != null && batch.VisibleEntries.Contains(batch.SelectedEntry), "collapsing a batch never leaves a hidden restore target");

        var bounds = NativeHistoryClient.DateBounds(new(Date: "custom", Start: new(2026, 10, 29), End: new(2026, 10, 29)), new(2026, 10, 30));
        Check(DateTimeOffset.Parse(bounds.Start!).LocalDateTime.Date == new DateTime(2026, 10, 29) && DateTimeOffset.Parse(bounds.End!).LocalDateTime.Date == new DateTime(2026, 10, 30), "custom date query uses an exclusive next-local-day end");
        var dto = JsonSerializer.SerializeToElement(new { id = 7, description = "A native record", type = "text", categories = new[] { "code", "command" }, timestamp = "2026-10-29T12:00:00Z", size_bytes = 21, source_peer = "peer", pinned = true, batch_id = (string?)null, batch_count = (int?)null, batch_total = (int?)null, batch_index = (int?)null });
        var mapped = NativeHistoryClient.MapEntry(dto);
        Check(mapped.Id == 7 && mapped.Pinned && mapped.Categories.Length == 2 && mapped.BatchId == null && mapped.Icon == "code", "production DTO projection retains multi-label categories and nullable batch fields");
        var illustrated = NativeHistoryClient.MapEntry(JsonSerializer.SerializeToElement(new { id = 8, type = "composite", description = "Image and text", categories = new[] { "image" }, display = new { version = 1, kind = "document", thumbnail_eligible = true, cache_key = "th1:current" } }));
        Check(illustrated.ThumbnailEligible && illustrated.DisplayCacheKey == "th1:current" && illustrated.DisplayKind == "document", "composite thumbnail eligibility and snapshot authority come from Core display metadata");
        var noImage = NativeHistoryClient.MapEntry(JsonSerializer.SerializeToElement(new { id = 9, type = "composite", categories = new[] { "image" }, display = new { version = 1, kind = "document", thumbnail_eligible = false, cache_key = "th1:none" } }));
        Check(!noImage.ThumbnailEligible, "an image category cannot override Core's explicit thumbnail ineligibility");

        var frame = Envelope(new { entry_id = 7, kind = "text", name = "note.txt", size_bytes = 5, width = (int?)null, height = (int?)null, batch = (object?)null }, "hello"u8.ToArray());
        var preview = PreviewEnvelope.Parse(frame);
        Check(preview.EntryId == 7 && preview.Renderer == "text" && Encoding.UTF8.GetString(preview.Data.Span) == "hello", "TSPV v1 parses a real text frame without losing metadata");
        Reject(() => PreviewEnvelope.Parse([0, 1]), "truncated preview headers are rejected");
        var future = frame.ToArray(); future[4] = 2; Reject(() => PreviewEnvelope.Parse(future), "unsupported preview versions are rejected");
        var oversized = frame.ToArray(); BinaryPrimitives.WriteUInt32LittleEndian(oversized.AsSpan(5, 4), 2 * 1024 * 1024); Reject(() => PreviewEnvelope.Parse(oversized), "oversized metadata is rejected before parsing");
        Reject(() => PreviewEnvelope.Parse(Envelope(new { kind = "image", name = "image", size_bytes = 4, width = 1, height = 1 }, [1, 2, 3])), "incomplete RGBA payloads are rejected");
        Reject(() => PreviewEnvelope.Parse(Envelope(new { kind = "text", name = "bad.txt", size_bytes = 2, width = (int?)null, height = (int?)null }, [1])), "preview metadata cannot misstate payload size");
        Reject(()=>PreviewEnvelope.Parse(Envelope(new{kind="text",name="missing.txt",size_bytes=1},[1])),"required preview metadata fields cannot be omitted");
        var rgba = PreviewRenderers.Rgba([255, 0, 0, 255], 1, 1); var pixel = new byte[4]; rgba.CopyPixels(pixel, 4, 0);
        Check(pixel.SequenceEqual(new byte[] { 0, 0, 255, 255 }), "backend RGBA is converted to WPF BGRA without swapping red and blue");
        Check(PreviewRenderers.Png(rgba).Length > 0, "native thumbnails can be encoded and cached as bounded images");
        var markdown = PreviewRenderers.Markdown("# Native heading\n\n**bold** and `code`\n\n- first\n- second\n");
        Check(new TextRange(markdown.ContentStart, markdown.ContentEnd).Text.Contains("Native heading") && markdown.Blocks.OfType<System.Windows.Documents.List>().Any(), "Markdown renders native headings, inline formatting and lists");
        var markdownTable=PreviewRenderers.Markdown("| Column | Value |\n| --- | --- |\n| native | cell |\n");
        Check(markdownTable.Blocks.OfType<System.Windows.Documents.Table>().Any(),"Markdown tables preserve native row and cell structure");
        var svg = PreviewRenderers.Svg(Encoding.UTF8.GetBytes("<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><script>alert(1)</script><image href='file:///never-load.png'/><rect width='20' height='20' fill='red'/></svg>"));
        Check(svg.Width == 20 && svg.Height == 20, "SVG renders native vectors after removing scripts and external resources");
        Reject(() => PreviewRenderers.Svg(Encoding.UTF8.GetBytes("<!DOCTYPE svg [<!ENTITY x SYSTEM 'file:///never-load'>]><svg xmlns='http://www.w3.org/2000/svg'>&x;</svg>")), "SVG external XML entities are rejected");
        foreach(var target in new[]{"http://example.com/image.png","https://localhost/image.png","https://127.0.0.1/image.png","https://10.0.0.1/image.png","https://100.64.1.1/image.png","https://[::1]/image.png","https://[fd00::1]/image.png","https://user:secret@example.com/image.png","file:///image.png"})
            Check(!SvgExternalPolicy.Eligible(new Uri(target)),"SVG origin authorization rejects "+new Uri(target).GetLeftPart(UriPartial.Authority));
        Check(SvgExternalPolicy.Eligible(new Uri("https://example.com/image.png")),"ordinary HTTPS origin is eligible for explicit SVG authorization");
        Check(!SvgExternalPolicy.PublicAddress(System.Net.IPAddress.Parse("192.168.1.1"))&&!SvgExternalPolicy.PublicAddress(System.Net.IPAddress.Parse("::ffff:127.0.0.1")),"DNS connection pinning rejects private and mapped loopback addresses");
        var sources=SvgExternalPolicy.Summarize(Encoding.UTF8.GetBytes("<svg xmlns='http://www.w3.org/2000/svg'><image href='https://example.com/a.png'/><image href='https://example.com/b.png'/></svg>"));
        Check(sources.CanGrant&&sources.Images.Count==2&&sources.AllowedOrigins.SequenceEqual(new[]{"https://example.com"}),"SVG grant is scoped to the disclosed origins and exact raster references");
        Check(!SvgExternalPolicy.Summarize(Encoding.UTF8.GetBytes("<svg xmlns='http://www.w3.org/2000/svg'><image href='https://example.com/a.png'/><image href='file:///b.png'/></svg>")).CanGrant,"a mixed safe/local SVG cannot obtain a partial silent grant");
        Reject(()=>SvgExternalPolicy.NormalizeRaster("not-an-image"u8.ToArray()),"authorized SVG content must decode as a bounded raster image");
        var docx = PreviewRenderers.Docx(CreateDocx());
        Check(new TextRange(docx.ContentStart, docx.ContentEnd).Text.Contains("WPF document") && docx.Blocks.OfType<System.Windows.Documents.Table>().Any(), "DOCX renders native paragraphs and table cells");
        var pdf = CreatePdf();
        Check(PDFtoImage.Conversion.GetPageCount(pdf) == 1, "PDFium opens a synthetic PDF with the expected page count");
        using (var bitmap = PDFtoImage.Conversion.ToImage(pdf, 0, options: new PDFtoImage.RenderOptions(Width: 400)))
            Check(bitmap.Width == 400 && bitmap.Height > 0 && bitmap.ByteCount < 4 * 1024 * 1024, "PDFium rasterizes one bounded page through the native renderer");
        using(var document=UglyToad.PdfPig.PdfDocument.Open(pdf))Check(document.GetPage(1).Text.Contains("TailSync native PDF"),"PDF selectable-text view extracts page content without screen OCR");
        return checks;
    }
    private static byte[] Envelope(object metadata, byte[] data)
    {
        var json = JsonSerializer.SerializeToUtf8Bytes(metadata); var result = new byte[9 + json.Length + data.Length]; "TSPV"u8.CopyTo(result); result[4] = 1;
        BinaryPrimitives.WriteUInt32LittleEndian(result.AsSpan(5, 4), (uint)json.Length); json.CopyTo(result, 9); data.CopyTo(result, 9 + json.Length); return result;
    }
    private static byte[] CreateDocx()
    {
        using var output = new MemoryStream();
        using (var archive = new ZipArchive(output, ZipArchiveMode.Create, true))
        {
            using var writer = new StreamWriter(archive.CreateEntry("word/document.xml").Open(), Encoding.UTF8);
            writer.Write("<w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>WPF document</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Native table</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>");
        }
        return output.ToArray();
    }
    private static byte[] CreatePdf()
    {
        const string stream = "BT /F1 16 Tf 40 150 Td (TailSync native PDF) Tj ET";
        var objects = new[] { "<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>", "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>", $"<< /Length {stream.Length} >>\nstream\n{stream}\nendstream" };
        var text = new StringBuilder("%PDF-1.4\n"); var offsets = new List<int>();
        for (var i = 0; i < objects.Length; i++) { offsets.Add(text.Length); text.Append($"{i + 1} 0 obj\n{objects[i]}\nendobj\n"); }
        var xref = text.Length; text.Append($"xref\n0 {objects.Length + 1}\n0000000000 65535 f \n");
        foreach (var offset in offsets) text.Append($"{offset:0000000000} 00000 n \n");
        text.Append($"trailer\n<< /Size {objects.Length + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"); return Encoding.ASCII.GetBytes(text.ToString());
    }
    private sealed class RacingClient : ITailSyncClient
    {
        public async Task<IReadOnlyList<HistoryEntry>> GetHistoryAsync(HistoryQuery query, CancellationToken cancellation = default) { await Task.Delay(query.Keyword == "slow" ? 50 : 1); return [new() { Id = query.Keyword == "slow" ? 1 : 2 }]; }
        public Task<ActionResult> SetFavoriteAsync(long id, bool favorite, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
        public Task<ActionResult> DeleteAsync(long id, bool fromFavorites, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
        public Task<ActionResult> RestoreAsync(long id, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
    }
    private sealed class PagedClient : IHistoryPageClient
    {
        public Task<HistoryPage> GetPageAsync(HistoryQuery query, int limit, int offset, CancellationToken cancellation = default) => Task.FromResult(new HistoryPage(Enumerable.Range(1, 65).Skip(offset).Take(limit).Select(id => new HistoryEntry { Id = id }).ToArray(), offset + limit < 65, 65));
        public async Task<IReadOnlyList<HistoryEntry>> GetHistoryAsync(HistoryQuery query, CancellationToken cancellation = default) => (await GetPageAsync(query, 30, 0, cancellation)).Entries;
        public Task<ActionResult> SetFavoriteAsync(long id, bool favorite, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
        public Task<ActionResult> DeleteAsync(long id, bool fromFavorites, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
        public Task<ActionResult> RestoreAsync(long id, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
        public Task<ActionResult> ClearHistoryAsync(CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
        public Task<ActionResult> RestoreBatchAsync(string id, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
        public Task<ActionResult> CancelBatchAsync(string id, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
    }
    private sealed class BatchClient : ITailSyncClient
    {
        public Task<IReadOnlyList<HistoryEntry>> GetHistoryAsync(HistoryQuery query, CancellationToken cancellation = default) => Task.FromResult<IReadOnlyList<HistoryEntry>>(Enumerable.Range(1, 5).Select(id => new HistoryEntry { Id = id, BatchId = "batch", BatchCount = 5, BatchTotal = 5 }).ToArray());
        public Task<ActionResult> SetFavoriteAsync(long id, bool favorite, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
        public Task<ActionResult> DeleteAsync(long id, bool fromFavorites, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
        public Task<ActionResult> RestoreAsync(long id, CancellationToken cancellation = default) => Task.FromResult(new ActionResult(true, ""));
    }
}
