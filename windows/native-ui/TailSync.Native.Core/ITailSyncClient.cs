namespace TailSync.Native.Core;

public sealed record HistoryQuery(string Keyword = "", string Category = "all", bool Favorites = false,
    string Date = "all", DateOnly? Start = null, DateOnly? End = null);

public sealed record ActionResult(bool Success, string Message, bool Complete = true);

public sealed record HistoryPage(IReadOnlyList<HistoryEntry> Entries, bool HasMore, long? Total);

public interface ICheckedHistoryRestore
{
    Task<ActionResult> RestoreEntryAsync(HistoryEntry entry, CancellationToken cancellation = default);
    Task<ActionResult> RestoreFormatAsync(HistoryEntry entry, string format, CancellationToken cancellation = default);
}

// Production paging remains optional so existing platform/demo adapters retain
// their small read/write surface. The WPF shell uses it when available.
public interface IHistoryPageClient : ITailSyncClient
{
    Task<HistoryPage> GetPageAsync(HistoryQuery query, int limit, int offset, CancellationToken cancellation = default);
    Task<ActionResult> ClearHistoryAsync(CancellationToken cancellation = default);
    Task<ActionResult> RestoreBatchAsync(string batchId, CancellationToken cancellation = default);
    Task<ActionResult> CancelBatchAsync(string batchId, CancellationToken cancellation = default);
}

// A production Named Pipe adapter will implement this boundary. The prototype
// deliberately uses only DemoTailSyncClient; it cannot discover user data.
public interface ITailSyncClient
{
    Task<IReadOnlyList<HistoryEntry>> GetHistoryAsync(HistoryQuery query, CancellationToken cancellation = default);
    Task<ActionResult> SetFavoriteAsync(long id, bool favorite, CancellationToken cancellation = default);
    Task<ActionResult> DeleteAsync(long id, bool fromFavorites, CancellationToken cancellation = default);
    Task<ActionResult> RestoreAsync(long id, CancellationToken cancellation = default);
}
