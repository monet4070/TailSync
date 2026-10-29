namespace TailSync.Native.Core;

public sealed record HistoryQuery(string Keyword = "", string Category = "all", bool Favorites = false,
    string Date = "all", DateOnly? Start = null, DateOnly? End = null);

public sealed record ActionResult(bool Success, string Message);

// A production Named Pipe adapter will implement this boundary. The prototype
// deliberately uses only DemoTailSyncClient; it cannot discover user data.
public interface ITailSyncClient
{
    Task<IReadOnlyList<HistoryEntry>> GetHistoryAsync(HistoryQuery query, CancellationToken cancellation = default);
    Task<ActionResult> SetFavoriteAsync(long id, bool favorite, CancellationToken cancellation = default);
    Task<ActionResult> DeleteAsync(long id, bool fromFavorites, CancellationToken cancellation = default);
    Task<ActionResult> RestoreAsync(long id, CancellationToken cancellation = default);
}
