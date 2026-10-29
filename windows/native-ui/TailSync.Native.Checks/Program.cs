using TailSync.Native.Core;

var client = new DemoTailSyncClient();
var viewModel = new ShellViewModel(client);
await viewModel.RefreshAsync();
Check(viewModel.Entries.Count == 8, "fixture has eight visible logical entries");
Check(viewModel.SelectedEntry?.Id == 1, "newest item is selected");

viewModel.Keyword = "FLUENT";
await viewModel.RefreshAsync();
Check(viewModel.Entries.Count == 1 && viewModel.SelectedEntry?.Id == 2, "case-insensitive content search");
Check(!(await viewModel.DeleteSelectedAsync()).Success, "favorite cannot be deleted from history");
viewModel.Keyword = "";
viewModel.Category = "image";
await viewModel.RefreshAsync();
Check(viewModel.Entries.Count == 1 && viewModel.SelectedEntry?.Id == 4, "image category filter");

viewModel.Category = "all";
viewModel.Date = "today";
await viewModel.RefreshAsync();
Check(viewModel.Entries.Count == 5, "date filter uses fixture date");
viewModel.Date = "all";
await viewModel.NavigateAsync("favorites");
Check(viewModel.Entries.Count == 3, "favorites are a protected history subset");
Check((await viewModel.DeleteSelectedAsync()).Success, "favorite deletion is explicit in favorites");
Check(viewModel.Entries.Count == 2, "favorite delete refreshes selection");
await viewModel.ToggleFavoriteAsync();
Check(viewModel.Entries.Count == 1, "unfavoriting removes item from favorites immediately");

viewModel.Keyword = "nonexistent";
await viewModel.RefreshAsync();
Check(viewModel.SelectedEntry is null && !(await viewModel.RestoreSelectedAsync()).Success, "empty query has no stale restore target");
viewModel.Keyword = "";
await viewModel.NavigateAsync("history");
var restored = await viewModel.RestoreSelectedAsync();
Check(restored.Success && restored.Message.Contains("未更改系统剪贴板"), "demo restore does not claim a real clipboard operation");

using var cancellation = new CancellationTokenSource();
cancellation.Cancel();
try { await client.GetHistoryAsync(new(), cancellation.Token); throw new Exception("cancellation ignored"); }
catch (OperationCanceledException) { Console.WriteLine("PASS: query cancellation"); }
Check(MacDesign.WindowSize("history") == (400, 600) && MacDesign.WindowSize("settings") == (540, 500), "Mac window geometry is shared with preview");
Check(MacDesign.Root.GetProperty("sections").EnumerateArray().Select(x => x.GetProperty("id").GetString()).SequenceEqual(new[] { "general", "updates", "history", "storage", "manageConnections", "appearance" }), "all Mac setting groups keep source order");
var fresh = new DemoTailSyncClient();
Check((await fresh.GetHistoryAsync(new(Date: "yesterday"))).Count == 3, "yesterday uses fixture calendar");
Check((await fresh.GetHistoryAsync(new(Date: "custom", Start: new(2026, 10, 25), End: new(2026, 10, 25)))).Count == 5, "custom date range is inclusive");
Check((await fresh.GetHistoryAsync(new(Date: "custom", Start: new(2026, 10, 26), End: new(2026, 10, 25)))).Count == 0, "inverted range has no stale entries");
var demo = await fresh.GetHistoryAsync(new());
Check(demo.Single(x => x.Id == 1).IsComposite && !demo.Single(x => x.Id == 1).HasFormatMenu && demo.Single(x => x.Id == 2).HasFormatMenu, "native-only diagnostics do not expose format menu");
Console.WriteLine("All native presentation checks passed.");

static void Check(bool condition, string scenario)
{
    if (!condition) throw new InvalidOperationException(scenario);
    Console.WriteLine($"PASS: {scenario}");
}
