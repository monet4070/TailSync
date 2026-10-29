using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace TailSync.Native.Core;

public sealed class ShellViewModel(ITailSyncClient client) : INotifyPropertyChanged
{
    private HistoryEntry? _selectedEntry;
    private int _generation;
    public ObservableCollection<HistoryEntry> Entries { get; } = [];
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

    public async Task RefreshAsync()
    {
        var generation = ++_generation;
        var selectedId = SelectedEntry?.Id;
        var entries = await client.GetHistoryAsync(new(Keyword, Category, Page == "favorites", Date, Start, End));
        if (generation != _generation) return;
        Entries.Clear();
        foreach (var entry in entries) Entries.Add(entry);
        SelectedEntry = Entries.FirstOrDefault(x => x.Id == selectedId) ?? Entries.FirstOrDefault();
        OnPropertyChanged(nameof(CountLabel));
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
        ? client.RestoreAsync(entry.Id) : Task.FromResult(new ActionResult(false, "请先选择一条内容"));

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new(name));
}
