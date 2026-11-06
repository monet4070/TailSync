using TailSync.Native.Core;

internal static class HistoryStateChecks
{
    internal static async Task Run()
    {
        void Check(bool value, string label) { if (!value) throw new InvalidOperationException(label); Console.WriteLine("PASS: " + label); }
        var client = new ControlledClient(); var model = new ShellViewModel(client);
        await model.RefreshAsync();
        Check(model.VisibleEntries.Take(6).Select(x => x.ShortcutNumber).SequenceEqual(Enumerable.Range(1, 6)), "numbered IDs follow committed visible order");
        model.SelectedEntry = model.VisibleEntries[4];
        var gate = new TaskCompletionSource<HistoryPage>(); client.Read = (_, _, _) => gate.Task;
        var page = model.GoToPageAsync(1);
        Check(model.IsLoading && !model.IsQueryCurrent && model.PageIndex == 0, "page turns synchronously disable old actions and retain the committed page");
        await model.GoToPageAsync(2);
        gate.SetException(new IOException("synthetic page failure"));
        try { await page; } catch (IOException) { }
        Check(model.PageIndex == 0 && model.SelectedEntry?.Id == 5 && model.IsQueryCurrent, "failed paging rolls back to the original selection and page");
        client.Read = (_, _, offset) => Task.FromResult(ControlledClient.Page(offset));
        await model.GoToPageAsync(1);
        Check(model.PageIndex == 1 && model.SelectedEntry?.Id == 31 && model.VisibleEntries[0].ShortcutNumber == 1, "new pages select their first entry and restart numbering");
        model.Keyword = "new query"; model.CancelPendingQuery();
        client.Read = (_, _, _) => Task.FromException<HistoryPage>(new IOException("query failure"));
        try { await model.RefreshAsync(); } catch (IOException) { }
        Check(!model.IsQueryCurrent && !model.IsLoading, "failed search leaves old rows disabled");
        var slow = new TaskCompletionSource<HistoryPage>(); client.Read = (_, _, _) => slow.Task;
        var oldRead = model.RefreshAsync(); model.ClearSnapshot();
        client.Read = (_, _, _) => Task.FromResult(new HistoryPage([new() { Id = 77 }], false, 1));
        await model.RefreshAsync(); slow.SetResult(new([new() { Id = 88 }], false, 1)); await oldRead;
        Check(model.SelectedEntry?.Id == 77 && model.IsQueryCurrent, "late results cannot replace a snapshot after a data-mode transition");
        model.Keyword = ""; client.Read = (_, _, _) => Task.FromResult(new HistoryPage([
            new() { Id = 1 }, new() { Id = 2, BatchId = "batch", BatchCount = 5 }, new() { Id = 3, BatchId = "batch", BatchCount = 5 },
            new() { Id = 4, BatchId = "batch", BatchCount = 5 }, new() { Id = 5, BatchId = "batch", BatchCount = 5 },
            new() { Id = 6, BatchId = "batch", BatchCount = 5 }, new() { Id = 7 }], false, 7));
        await model.RefreshAsync();
        Check(model.VisibleEntries.Select(x => x.Id).SequenceEqual(new long[] { 1, 2, 3, 4, 7 }) && model.VisibleEntries[^1].ShortcutNumber == 5, "collapsed batch headers and hidden children consume no number");
        model.ToggleBatch("batch");
        Check(model.VisibleEntries[5].Id == 6 && model.VisibleEntries[5].ShortcutNumber == 6 && model.VisibleEntries[6].ShortcutNumber == 0, "expanding batches atomically updates the six numbered targets");

        // Optimistic removal checks
        var resetFired = false;
        var removeCount = 0;
        model.VisibleEntries.CollectionChanged += (_, e) =>
        {
            if (e.Action == System.Collections.Specialized.NotifyCollectionChangedAction.Reset) resetFired = true;
            if (e.Action == System.Collections.Specialized.NotifyCollectionChangedAction.Remove) removeCount++;
        };
        var toDelete = model.VisibleEntries[2]; // Id == 3
        model.SelectedEntry = toDelete;
        model.RemoveOptimisticEntry(toDelete);
        Check(!resetFired, "optimistic removal does not raise collection reset (preserves scroll offset)");
        Check(removeCount == 1, "optimistic removal raises single item remove action");
        Check(model.SelectedEntry?.Id == 4, "selection maintains index at deleted row position");
        Check(model.VisibleEntries[2].Id == 4 && model.VisibleEntries[2].ShortcutNumber == 3, "remaining items shift up and maintain sequential numbering");
    }
    private sealed class ControlledClient : IHistoryPageClient
    {
        internal Func<HistoryQuery, int, int, Task<HistoryPage>> Read = (_, _, offset) => Task.FromResult(Page(offset));
        internal static HistoryPage Page(int offset) => new(Enumerable.Range(offset + 1, 30).Select(id => new HistoryEntry { Id = id }).ToArray(), offset == 0, 60);
        public Task<HistoryPage> GetPageAsync(HistoryQuery query, int limit, int offset, CancellationToken cancellation = default) => Read(query, limit, offset);
        public async Task<IReadOnlyList<HistoryEntry>> GetHistoryAsync(HistoryQuery query, CancellationToken cancellation = default) => (await GetPageAsync(query, 30, 0, cancellation)).Entries;
        public Task<ActionResult> RestoreAsync(long id, CancellationToken cancellation = default) => throw new NotSupportedException();
        public Task<ActionResult> SetFavoriteAsync(long id, bool value, CancellationToken cancellation = default) => throw new NotSupportedException();
        public Task<ActionResult> DeleteAsync(long id, bool fromFavorites, CancellationToken cancellation = default) => throw new NotSupportedException();
        public Task<ActionResult> ClearHistoryAsync(CancellationToken cancellation = default) => throw new NotSupportedException();
        public Task<ActionResult> RestoreBatchAsync(string batchId, CancellationToken cancellation = default) => throw new NotSupportedException();
        public Task<ActionResult> CancelBatchAsync(string batchId, CancellationToken cancellation = default) => throw new NotSupportedException();
    }
}
