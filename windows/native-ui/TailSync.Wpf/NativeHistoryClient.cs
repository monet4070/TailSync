using System.Globalization;
using System.IO;
using System.Text.Json;
using TailSync.Native.Core;

namespace TailSync.Wpf;

internal sealed class NativeHistoryClient : IHistoryPageClient, ICheckedHistoryRestore
{
    public async Task<IReadOnlyList<HistoryEntry>> GetHistoryAsync(HistoryQuery query, CancellationToken cancellation = default)
        => (await GetPageAsync(query, ShellViewModel.PageSize, 0, cancellation)).Entries;

    public async Task<HistoryPage> GetPageAsync(HistoryQuery query, int limit, int offset, CancellationToken cancellation = default)
    {
        var (start, end) = DateBounds(query, DateTime.Today);
        if (query.Date == "custom" && ((!query.Start.HasValue && !query.End.HasValue) || query.Start > query.End)) return new([], false, 0);
        var result = await NativeBackend.InvokeAsync<JsonElement>("get_history_page", new
        {
            keyword = string.IsNullOrWhiteSpace(query.Keyword) ? null : query.Keyword.Trim(),
            category = query.Category == "all" ? null : query.Category,
            startTime = start, endTime = end, limit = Math.Clamp(limit, 1, 100), offset = Math.Max(0, offset),
            collection = query.Favorites ? "favorites" : "all"
        }, cancellation);
        if (result.ValueKind != JsonValueKind.Object || !result.TryGetProperty("entries", out var entries) || entries.ValueKind != JsonValueKind.Array)
            throw new InvalidDataException("历史记录响应格式无效");
        return new(entries.EnumerateArray().Select(MapEntry).ToArray(),
            result.TryGetProperty("has_more", out var more) && more.GetBoolean(),
            result.TryGetProperty("total", out var total) && total.ValueKind == JsonValueKind.Number ? total.GetInt64() : null);
    }

    internal static HistoryEntry MapEntry(JsonElement value)
    {
        if (!value.TryGetProperty("id", out var id) || !id.TryGetInt64(out var entryId) || entryId <= 0) throw new InvalidDataException("历史记录 ID 无效");
        var description = Text(value, "description");
        var type = Text(value, "type", "text");
        var categories = value.TryGetProperty("categories", out var items) && items.ValueKind == JsonValueKind.Array
            ? items.EnumerateArray().Where(x => x.ValueKind == JsonValueKind.String).Select(x => x.GetString()!).ToArray() : [];
        if (categories.Length == 0) categories = [Text(value, "category", type)];
        var primary = categories[0];
        var display = value.Object("display");
        var composite = value.Object("composite");
        var hasDisplay = display.Long("version") == 1;
        var timestamp = DateTimeOffset.TryParse(Text(value, "timestamp"), CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var date) ? date : DateTimeOffset.MinValue;
        return new HistoryEntry
        {
            Id = entryId, Description = description, Title = description, Type = type, Categories = categories,
            IsComposite = type == "composite", LocalOnly = composite.Bool("native_only"),
            CompositeIdentity = composite.Object("identity").ValueKind == JsonValueKind.Object ? composite.Object("identity").Clone() : null,
            DisplayCacheKey = hasDisplay ? display.String("cache_key") : "",
            ThumbnailEligible = hasDisplay ? display.Bool("thumbnail_eligible") : type == "image" || composite.Long("image_count") > 0 || composite.Bool("native_only"),
            DisplayKind = hasDisplay ? display.String("kind") : null,
            Timestamp = timestamp, Time = timestamp == DateTimeOffset.MinValue ? "" : timestamp.LocalDateTime.ToString("HH:mm"),
            SourcePeer = Text(value, "source_peer"), SizeBytes = Number(value, "size_bytes") ?? 0,
            DataHash = Text(value, "data_hash"), Pinned = value.TryGetProperty("pinned", out var pinned) && pinned.ValueKind == JsonValueKind.True,
            Icon = primary switch { "website" => "globe", "code" or "command" or "structured_data" => "code", "image" => "image", "path" => "folder", _ => "file" },
            Preview = primary, BatchId = NullableText(value, "batch_id"), BatchCount = (int?)Number(value, "batch_count"),
            BatchTotal = (int?)Number(value, "batch_total"), BatchIndex = (int?)Number(value, "batch_index"), BatchStatus = Text(value, "batch_status", "complete")
        };
    }

    internal static (string? Start, string? End) DateBounds(HistoryQuery query, DateTime today)
    {
        DateTime? start = null, end = null;
        switch (query.Date)
        {
            case "today": start = today; end = today.AddDays(1); break;
            case "yesterday": start = today.AddDays(-1); end = today; break;
            case "last7": start = today.AddDays(-6); end = today.AddDays(1); break;
            case "last30": start = today.AddDays(-29); end = today.AddDays(1); break;
            case "thisMonth": start = new(today.Year, today.Month, 1); end = start.Value.AddMonths(1); break;
            case "custom": start = query.Start?.ToDateTime(TimeOnly.MinValue); end = query.End?.ToDateTime(TimeOnly.MinValue).AddDays(1); break;
        }
        static string? Iso(DateTime? local) => local.HasValue ? DateTime.SpecifyKind(local.Value, DateTimeKind.Local).ToUniversalTime().ToString("O", CultureInfo.InvariantCulture) : null;
        return (Iso(start), Iso(end));
    }

    public async Task<ActionResult> SetFavoriteAsync(long id, bool favorite, CancellationToken cancellation = default)
    {
        await NativeBackend.InvokeAsync<JsonElement>("set_history_favorite", new { id, favorite }, cancellation);
        return new(true, favorite ? Localizer.Choose("已添加收藏", "Added to favorites") : Localizer.Choose("已取消收藏", "Removed from favorites"));
    }
    public async Task<ActionResult> DeleteAsync(long id, bool fromFavorites, CancellationToken cancellation = default)
    {
        if (fromFavorites) await NativeBackend.InvokeAsync<JsonElement>("delete_favorite_entry", new { id }, cancellation);
        else await NativeBackend.InvokeAsync("delete_entry", new { id }, cancellation);
        return new(true, Localizer.Choose("已删除记录", "Entry deleted"));
    }
    public Task<ActionResult> RestoreAsync(long id, CancellationToken cancellation = default) => RestoreActivity.RunAsync(async () =>
    {
        var report = await NativeBackend.InvokeAsync<JsonElement>("restore_entry", new { id }, cancellation);
        return RestoreReport(report);
    });
    public Task<ActionResult> RestoreEntryAsync(HistoryEntry entry, CancellationToken cancellation = default) => RestoreActivity.RunAsync(async () =>
    {
        var report = await NativeBackend.InvokeAsync<JsonElement>("restore_entry", new { id = entry.Id, restoreOriginalId = entry.Id, restoreIdentity = entry.CompositeIdentity }, cancellation);
        return RestoreReport(report);
    });
    internal static ActionResult RestoreReport(JsonElement report)
    {
        if (!report.TryGetProperty("complete", out var complete) || complete.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
            throw new InvalidDataException("Invalid clipboard restore report");
        var missing = report.Long("missing_images");
        if (complete.GetBoolean() && missing == 0) return new(true, Localizer.Text("history.preview.restored"));
        return new(true, Localizer.Choose($"已恢复可用内容，仍缺少 {missing} 张图片。", $"Available content restored; {missing} images are still missing."), false);
    }
    public async Task<ActionResult> ClearHistoryAsync(CancellationToken cancellation = default)
    {
        await NativeBackend.InvokeAsync("clear_history", null, cancellation); return new(true, Localizer.Choose("已清空未收藏的记录", "Unfavorited history cleared"));
    }
    public Task<ActionResult> RestoreBatchAsync(string batchId, CancellationToken cancellation = default) => RestoreActivity.RunAsync(async () =>
    {
        await NativeBackend.InvokeAsync("restore_file_batch", new { batchId }, cancellation); return new(true, Localizer.Text("history.preview.restored"));
    });
    public async Task<ActionResult> CancelBatchAsync(string batchId, CancellationToken cancellation = default)
    {
        await NativeBackend.InvokeAsync("cancel_file_batch", new { batchId }, cancellation); return new(true, Localizer.Choose("已停止传输", "Transfer stopped"));
    }
    private static string Text(JsonElement value, string key, string fallback = "") => value.TryGetProperty(key, out var item) && item.ValueKind == JsonValueKind.String ? item.GetString()! : fallback;
    private static string? NullableText(JsonElement value, string key) => value.TryGetProperty(key, out var item) && item.ValueKind == JsonValueKind.String ? item.GetString() : null;
    private static long? Number(JsonElement value, string key) => value.TryGetProperty(key, out var item) && item.ValueKind == JsonValueKind.Number && item.TryGetInt64(out var number) ? number : null;
}
