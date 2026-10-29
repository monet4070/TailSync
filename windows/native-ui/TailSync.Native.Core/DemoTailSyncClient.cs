using System.Text.Json;

namespace TailSync.Native.Core;

public sealed class DemoTailSyncClient : ITailSyncClient
{
    private readonly List<HistoryEntry> _entries;

    public DemoTailSyncClient()
    {
        var assembly = typeof(DemoTailSyncClient).Assembly;
        var name = assembly.GetManifestResourceNames().Single(x => x.EndsWith("history.json", StringComparison.Ordinal));
        using var stream = assembly.GetManifestResourceStream(name)!;
        _entries = JsonSerializer.Deserialize<List<HistoryEntry>>(stream) ?? [];
    }

    public Task<IReadOnlyList<HistoryEntry>> GetHistoryAsync(HistoryQuery query, CancellationToken cancellation = default)
    {
        cancellation.ThrowIfCancellationRequested();
        // The demo clock is fixed to match its fixture, independent of the user's clock.
        var today = new DateOnly(2026, 10, 25);
        IEnumerable<HistoryEntry> results = _entries;
        if (query.Favorites) results = results.Where(x => x.Pinned);
        if (query.Category != "all") results = results.Where(x => x.Categories.Contains(query.Category));
        results = results.Where(x => {
            var date = DateOnly.FromDateTime(x.Timestamp.Date);
            return query.Date switch {
                "today" => date == today, "yesterday" => date == today.AddDays(-1),
                "last7" => date >= today.AddDays(-6) && date <= today,
                "last30" => date >= today.AddDays(-29) && date <= today,
                "thisMonth" => date.Year == today.Year && date.Month == today.Month,
                "custom" => (!query.Start.HasValue || date >= query.Start.Value) && (!query.End.HasValue || date <= query.End.Value),
                _ => true
            };
        });
        if (!string.IsNullOrWhiteSpace(query.Keyword))
        {
            var term = query.Keyword.Trim();
            results = results.Where(x => $"{x.Title}\n{x.Description}\n{x.Body}\n{x.SourcePeer}"
                .Contains(term, StringComparison.OrdinalIgnoreCase));
        }
        IReadOnlyList<HistoryEntry> page = results.OrderByDescending(x => x.Timestamp).ThenByDescending(x => x.Id).ToArray();
        return Task.FromResult(page);
    }

    public Task<ActionResult> SetFavoriteAsync(long id, bool favorite, CancellationToken cancellation = default)
    {
        cancellation.ThrowIfCancellationRequested();
        var entry = _entries.Find(x => x.Id == id);
        if (entry is null) return Task.FromResult(new ActionResult(false, "条目已不存在"));
        entry.Pinned = favorite;
        return Task.FromResult(new ActionResult(true, favorite ? "已添加到示例收藏" : "已取消示例收藏"));
    }

    public Task<ActionResult> DeleteAsync(long id, bool fromFavorites, CancellationToken cancellation = default)
    {
        cancellation.ThrowIfCancellationRequested();
        var entry = _entries.Find(x => x.Id == id);
        if (entry is null) return Task.FromResult(new ActionResult(false, "条目已不存在"));
        if (entry.Pinned && !fromFavorites)
            return Task.FromResult(new ActionResult(false, "收藏受到保护，请到收藏中删除"));
        _entries.Remove(entry);
        return Task.FromResult(new ActionResult(true, "已删除示例条目"));
    }

    public Task<ActionResult> RestoreAsync(long id, CancellationToken cancellation = default)
    {
        cancellation.ThrowIfCancellationRequested();
        var entry = _entries.Find(x => x.Id == id);
        return Task.FromResult(entry is null
            ? new ActionResult(false, "条目已不存在")
            : new ActionResult(true, $"已演示恢复「{entry.Title}」；未更改系统剪贴板"));
    }
}
