using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace TailSync.Native.Core;

public sealed class ShellViewModel(ITailSyncClient client) : INotifyPropertyChanged
{
    private HistoryEntry? _selectedEntry;
    private int _generation;
    private CancellationTokenSource? _queryCancellation;
    private HistoryQuery? _committedQuery;
    private bool _queryInvalidated = true;
    private bool _isLoading;
    public bool IsLoading { get => _isLoading; private set { _isLoading = value; OnPropertyChanged(); OnPropertyChanged(nameof(IsQueryCurrent)); } }
    private HistoryQuery CurrentQuery => new(Keyword, Category, Page == "favorites", Date, Start, End);
    public bool IsQueryCurrent => !IsLoading && !_queryInvalidated && _committedQuery == CurrentQuery;
    private readonly HashSet<string> _expandedBatches = [];
    public ObservableCollection<HistoryEntry> Entries { get; } = [];
    public ObservableCollection<HistoryEntry> VisibleEntries { get; } = [];
    public int PageIndex { get; private set; }
    public const int PageSize = 30;
    public long? Total { get; private set; }
    public bool HasNext { get; private set; }
    public bool HasPrevious => PageIndex > 0;
    public string PageLabel => Total.HasValue ? $"{PageIndex + 1} / {Math.Max(1, (Total.Value + PageSize - 1) / PageSize)}" : $"{PageIndex + 1}";
    public bool GroupDates { get; set; }
    public string Page { get; private set; } = "history";
    public string Keyword { get; set; } = "";
    public string Category { get; set; } = "all";
    public string Date { get; set; } = "all";
    public DateOnly? Start { get; set; }
    public DateOnly? End { get; set; }
    public string CountLabel => $"{Entries.Count} 条内容";
    public string Heading => Page == "favorites" ? "收藏" : "剪贴板历史";
    public HistoryEntry? SelectedEntry
    {
        get => _selectedEntry;
        set { if (_selectedEntry == value) return; _selectedEntry = value; OnPropertyChanged(); }
    }

    public async Task NavigateAsync(string page)
    {
        Page = page;
        OnPropertyChanged(nameof(Heading));
        if (page is "history" or "favorites") await RefreshAsync();
    }

    public Task RefreshAsync() => LoadPageAsync(PageIndex, false);

    private async Task LoadPageAsync(int requestedPage, bool selectFirst)
    {
        var generation = ++_generation;
        _queryCancellation?.Cancel();
        _queryCancellation?.Dispose();
        _queryCancellation = new();
        var cancellation = _queryCancellation.Token;
        var selectedId = SelectedEntry?.Id;
        var query = CurrentQuery;
        IsLoading = true;
        long? total;
        bool hasNext;
        IReadOnlyList<HistoryEntry> entries;
        try
        {
            if (client is IHistoryPageClient paged)
            {
                var result = await paged.GetPageAsync(query, PageSize, requestedPage * PageSize, cancellation);
                if (generation != _generation) return;
                if (result.Entries.Count == 0 && requestedPage > 0)
                {
                    var normalized = Math.Max(0, result.Total.HasValue ? (int)Math.Min(requestedPage - 1, Math.Max(0, (result.Total.Value - 1) / PageSize)) : requestedPage - 1);
                    await LoadPageAsync(normalized, true); return;
                }
                entries = result.Entries; total = result.Total; hasNext = result.HasMore;
            }
            else
            {
                entries = await client.GetHistoryAsync(query, cancellation);
                total = entries.Count; hasNext = false;
            }
        }
        catch (OperationCanceledException) when (cancellation.IsCancellationRequested) { if (generation == _generation) IsLoading = false; return; }
        catch { if (generation == _generation) IsLoading = false; throw; }
        if (generation != _generation || cancellation.IsCancellationRequested || query != CurrentQuery) { if (generation == _generation) IsLoading = false; return; }
        PageIndex = requestedPage; Total = total; HasNext = hasNext;
        Entries.Clear();
        foreach (var entry in entries) Entries.Add(entry);
        BuildVisibleEntries();
        SelectedEntry = (selectFirst ? null : VisibleEntries.FirstOrDefault(x => x.Id == selectedId)) ?? VisibleEntries.FirstOrDefault();
        _committedQuery = query; _queryInvalidated = false; IsLoading = false;
        OnPropertyChanged(nameof(CountLabel));
        OnPropertyChanged(nameof(PageLabel));OnPropertyChanged(nameof(HasNext));OnPropertyChanged(nameof(HasPrevious));
    }

    public void ResetPage() { PageIndex = 0; _expandedBatches.Clear(); }
    public Task GoToPageAsync(int page) => IsLoading || !IsQueryCurrent ? Task.CompletedTask : LoadPageAsync(Math.Max(0, page), true);
    public void CancelPendingQuery() { ++_generation; _queryCancellation?.Cancel(); _queryInvalidated = true; IsLoading = false; }
    public void ClearSnapshot() { CancelPendingQuery(); Entries.Clear(); VisibleEntries.Clear(); SelectedEntry = null; ResetPage(); }
    public bool IsBatchExpanded(string batchId) => _expandedBatches.Contains(batchId);
    public void ToggleBatch(string batchId)
    {
        if (!_expandedBatches.Remove(batchId)) _expandedBatches.Add(batchId);
        var selected = SelectedEntry?.Id;
        BuildVisibleEntries();
        SelectedEntry = VisibleEntries.FirstOrDefault(x => x.Id == selected) ?? VisibleEntries.FirstOrDefault();
    }
    public void RefreshPresentation()
    {
        var selected=SelectedEntry?.Id;BuildVisibleEntries();foreach(var entry in Entries)entry.RefreshPresentation();
        SelectedEntry=VisibleEntries.FirstOrDefault(entry=>entry.Id==selected)??VisibleEntries.FirstOrDefault();
    }
    private void BuildVisibleEntries()
    {
        VisibleEntries.Clear();
        var positions = new Dictionary<string,int>();
        var now = DateTime.Today;
        string? lastGroup = null, lastBatch = null;
        foreach (var entry in Entries)
        {
            var days = (now - entry.Timestamp.LocalDateTime.Date).TotalDays;
            var groupKey = days == 0 ? "today" : days == 1 ? "yesterday" : days <= 7 ? "thisWeek" : days <= 30 ? "thisMonth" : "older";
            var group = HistoryEntry.Localize?.Invoke("history.group."+groupKey) ?? (days == 0 ? "今天" : days == 1 ? "昨天" : days <= 7 ? "近一周" : days <= 30 ? "近一月" : "更早");
            var position = 0;
            if (entry.BatchId is { } id)
            {
                position = positions.GetValueOrDefault(id); positions[id] = position + 1;
                if ((entry.BatchCount ?? entry.BatchTotal ?? 1) > 3 && !_expandedBatches.Contains(id) && position >= 3) continue;
            }
            entry.DateGroup = group; entry.ShowDateGroup = GroupDates && group != lastGroup;
            entry.ShowBatchHeader = entry.HasBatch && (entry.BatchId != lastBatch || group != lastGroup);
            entry.BatchToggleLabel = entry.BatchId is { } batch && _expandedBatches.Contains(batch) ? HistoryEntry.Localize?.Invoke("history.showLess") ?? "收起" : $"{HistoryEntry.Localize?.Invoke("history.showMore") ?? "展开"} ({Math.Max(0, (entry.BatchCount ?? entry.BatchTotal ?? 1) - 3)})";
            VisibleEntries.Add(entry); lastGroup = group; lastBatch = entry.BatchId;
        }
        for (var i = 0; i < VisibleEntries.Count; i++) VisibleEntries[i].ShortcutNumber = Page == "history" && i < 6 ? i + 1 : 0;
    }

    public async Task<ActionResult> ToggleFavoriteAsync()
    {
        if (SelectedEntry is not { } entry) return new(false, "请先选择一条内容");
        var result = await client.SetFavoriteAsync(entry.Id, !entry.Pinned);
        if (result.Success) await RefreshAsync();
        return result;
    }

    public async Task<ActionResult> DeleteSelectedAsync()
    {
        if (SelectedEntry is not { } entry) return new(false, "请先选择一条内容");
        var result = await client.DeleteAsync(entry.Id, Page == "favorites");
        if (result.Success) await RefreshAsync();
        return result;
    }

    public Task<ActionResult> RestoreSelectedAsync() => SelectedEntry is { } entry
        ? client is ICheckedHistoryRestore checkedRestore ? checkedRestore.RestoreEntryAsync(entry) : client.RestoreAsync(entry.Id)
        : Task.FromResult(new ActionResult(false, "请先选择一条内容"));

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new(name));
}
