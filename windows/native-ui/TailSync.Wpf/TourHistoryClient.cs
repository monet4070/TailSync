using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using TailSync.Native.Core;

namespace TailSync.Wpf;

// One routing client survives tour transitions. No window captures a stale
// production/demo client, and virtual records never cross the native transport.
internal sealed class TourHistoryClient(IHistoryPageClient production) : IHistoryPageClient, ICheckedHistoryRestore
{
    internal static readonly JsonElement[] Records = ReadRecords();
    private readonly HashSet<string> _hidden = [];
    private readonly Dictionary<string, bool> _favorites = [];
    internal static bool IsVirtual(long id) => id is >= 8_000_000_000 and < 9_000_000_000;
    internal static bool IsVirtualBatch(string id) => id.StartsWith("onboarding-", StringComparison.Ordinal) || Records.Any(r => r.String("batch_id") == id);
    private static JsonElement[] ReadRecords()
    {
        using var stream = typeof(TourHistoryClient).Assembly.GetManifestResourceStream("TailSync.Wpf.OnboardingDemo.json")!;
        using var document = JsonDocument.Parse(stream);
        var records = document.RootElement.GetProperty("entries").EnumerateArray().Select(r => r.Clone()).ToArray();
        foreach (var r in records)
            if (Convert.ToHexString(SHA256.HashData(Convert.FromBase64String(r.String("data_b64")))).ToLowerInvariant() != r.String("sha256"))
                throw new InvalidDataException("Invalid bundled onboarding sample");
        return records;
    }
    internal void Reset() { _hidden.Clear(); _favorites.Clear(); }
    private static string Logical(JsonElement r) => r.String("batch_id") is { Length: > 0 } batch ? batch : r.Long("id").ToString();
    private static JsonElement Record(long id) => Records.FirstOrDefault(r => r.Long("id") == id);
    internal static PreviewPayload Preview(long id, string? batchId)
    {
        var r = Record(id);
        if (r.ValueKind != JsonValueKind.Object) throw new InvalidDataException("Unknown demonstration record");
        var bytes = Convert.FromBase64String(r.String("data_b64"));
        var siblings = Records.Where(x => batchId != null && x.String("batch_id") == batchId).ToArray();
        var index = Array.FindIndex(siblings, x => x.Long("id") == id);
        var batch = index < 0 ? null : new PreviewBatch(batchId!, index, siblings.Length,
            index > 0 ? siblings[index - 1].Long("id") : null, index + 1 < siblings.Length ? siblings[index + 1].Long("id") : null);
        return new(id, r.String("kind"), r.String("name"), bytes.Length, null, null, batch, bytes);
    }
    internal static JsonElement Document(long id) => Record(id).Object("document");
    public async Task<IReadOnlyList<HistoryEntry>> GetHistoryAsync(HistoryQuery query, CancellationToken cancellation = default)
        => (await GetPageAsync(query, ShellViewModel.PageSize, 0, cancellation)).Entries;
    public Task<HistoryPage> GetPageAsync(HistoryQuery query, int limit, int offset, CancellationToken cancellation = default)
    {
        cancellation.ThrowIfCancellationRequested();
        if (!GuidedTour.Active) return production.GetPageAsync(query, limit, offset, cancellation);
        var (start, end) = NativeHistoryClient.DateBounds(query, DateTime.Today);
        var now = DateTimeOffset.Now;
        var rows = Records.Select((r, i) => (r, time: now.AddSeconds(-37 * i)))
            .Where(x => !_hidden.Contains(Logical(x.r)))
            .Where(x => query.Category == "all" || x.r.String("category") == query.Category)
            .Where(x => !query.Favorites || _favorites.GetValueOrDefault(Logical(x.r), x.r.String("category") == "code"))
            .Where(x => query.Keyword.Length == 0 || (x.r.String("description") + x.r.String("name") + Encoding.UTF8.GetString(Convert.FromBase64String(x.r.String("data_b64")))).Contains(query.Keyword, StringComparison.OrdinalIgnoreCase))
            .Where(x => (start == null || x.time >= DateTimeOffset.Parse(start)) && (end == null || x.time < DateTimeOffset.Parse(end)))
            .Select(x => Map(x.r, x.time)).ToArray();
        if (query.Date == "custom" && ((!query.Start.HasValue && !query.End.HasValue) || query.Start > query.End)) rows = [];
        return Task.FromResult(new HistoryPage(rows.Skip(offset).Take(limit).ToArray(), offset + limit < rows.Length, rows.Length));
    }
    private HistoryEntry Map(JsonElement r, DateTimeOffset time)
    {
        var bytes = Convert.FromBase64String(r.String("data_b64")); var category = r.String("category");
        return new() { Id = r.Long("id"), Title = r.String("description"), Description = r.String("description"),
            Body = r.String("kind") == "text" ? Encoding.UTF8.GetString(bytes) : "", Type = r.String("kind"),
            Categories = [category], Preview = category, Timestamp = time, Time = time.ToString("HH:mm"),
            SizeBytes = bytes.Length, SourcePeer = "TailSync Demo", DataHash = r.String("sha256"),
            Pinned = _favorites.GetValueOrDefault(Logical(r), category == "code"),
            BatchId = r.String("batch_id") is { Length: > 0 } batch ? batch : null,
            BatchCount = r.GetNullableInt("batch_count"), BatchTotal = r.GetNullableInt("batch_total"), BatchIndex = r.GetNullableInt("batch_index"),
            ThumbnailEligible = category == "image", ThumbnailBytes = r.String("name").EndsWith(".png") ? bytes : null,
            Icon = category switch { "image" => "image", "code" => "code", "website" => "globe", "path" => "folder", _ => "file" } };
    }
    private static ActionResult Simulated() => new(true, Localizer.Choose("演示操作完成，未更改系统剪贴板或真实数据。", "Demo completed. The system clipboard and real data were unchanged."));
    private static void CheckVirtual(long id)
    {
        if (Record(id).ValueKind != JsonValueKind.Object) throw new InvalidOperationException("Unknown demonstration record");
    }
    public Task<ActionResult> RestoreAsync(long id, CancellationToken cancellation = default)
    {
        if (!GuidedTour.Active) { if (IsVirtual(id)) throw new InvalidOperationException("Demonstration record expired"); return production.RestoreAsync(id, cancellation); }
        CheckVirtual(id); return Task.FromResult(Simulated());
    }
    public Task<ActionResult> RestoreEntryAsync(HistoryEntry entry, CancellationToken cancellation = default)
    {
        if (GuidedTour.Active || IsVirtual(entry.Id)) return RestoreAsync(entry.Id, cancellation);
        return production is ICheckedHistoryRestore checkedRestore ? checkedRestore.RestoreEntryAsync(entry, cancellation) : production.RestoreAsync(entry.Id, cancellation);
    }
    public Task<ActionResult> RestoreFormatAsync(HistoryEntry entry, string format, CancellationToken cancellation = default)
    {
        if (GuidedTour.Active || IsVirtual(entry.Id)) return RestoreAsync(entry.Id, cancellation);
        return production is ICheckedHistoryRestore checkedRestore ? checkedRestore.RestoreFormatAsync(entry, format, cancellation) : production.RestoreAsync(entry.Id, cancellation);
    }
    public Task<ActionResult> SetFavoriteAsync(long id, bool favorite, CancellationToken cancellation = default)
    {
        if (!GuidedTour.Active) { if (IsVirtual(id)) throw new InvalidOperationException("Demonstration record expired"); return production.SetFavoriteAsync(id, favorite, cancellation); }
        CheckVirtual(id); _favorites[Logical(Record(id))] = favorite; return Task.FromResult(Simulated());
    }
    public Task<ActionResult> DeleteAsync(long id, bool fromFavorites, CancellationToken cancellation = default)
    {
        if (!GuidedTour.Active) { if (IsVirtual(id)) throw new InvalidOperationException("Demonstration record expired"); return production.DeleteAsync(id, fromFavorites, cancellation); }
        CheckVirtual(id); var r = Record(id);
        if (!fromFavorites && _favorites.GetValueOrDefault(Logical(r), r.String("category") == "code"))
            return Task.FromResult(new ActionResult(false, Localizer.Text("history.favoriteProtected")));
        _hidden.Add(Logical(r)); return Task.FromResult(Simulated());
    }
    public Task<ActionResult> ClearHistoryAsync(CancellationToken cancellation = default)
    {
        if (!GuidedTour.Active) return production.ClearHistoryAsync(cancellation);
        foreach (var r in Records) if (!_favorites.GetValueOrDefault(Logical(r), r.String("category") == "code")) _hidden.Add(Logical(r));
        return Task.FromResult(Simulated());
    }
    public Task<ActionResult> RestoreBatchAsync(string batchId, CancellationToken cancellation = default)
    {
        if (GuidedTour.Active && IsVirtualBatch(batchId)) return Task.FromResult(Simulated());
        if (GuidedTour.Active || IsVirtualBatch(batchId)) throw new InvalidOperationException("Demonstration batch expired");
        return production.RestoreBatchAsync(batchId, cancellation);
    }
    public Task<ActionResult> CancelBatchAsync(string batchId, CancellationToken cancellation = default)
    {
        if (GuidedTour.Active || IsVirtualBatch(batchId)) return Task.FromResult(Simulated());
        return production.CancelBatchAsync(batchId, cancellation);
    }
}

internal static class DemoJson
{
    internal static int? GetNullableInt(this JsonElement r, string key) => r.TryGetProperty(key, out var value) && value.TryGetInt32(out var number) ? number : null;
}
