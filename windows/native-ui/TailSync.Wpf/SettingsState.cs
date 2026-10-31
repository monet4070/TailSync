using System.Text.Json;
using System.IO;

namespace TailSync.Wpf;

internal static class SettingsState
{
    private static readonly SemaphoreSlim SaveQueue = new(1, 1);
    private static int _reloadGeneration;
    internal static JsonElement Current { get; private set; }
    internal static event EventHandler? Changed;
    private static void Apply(JsonElement settings)
    {
        Current = settings;
        AppSession.SetSync(Current.Bool("sync_enabled", true));
        Localizer.SetLocale(Current.String("language", "zh-CN"));
        Changed?.Invoke(null, EventArgs.Empty);
    }
    internal static async Task ReloadAsync(CancellationToken token = default)
    {
        var generation=Interlocked.Increment(ref _reloadGeneration);
        var settings=await NativeBackend.InvokeAsync<JsonElement>("get_settings", token: token);
        if(generation!=Volatile.Read(ref _reloadGeneration)||token.IsCancellationRequested)return;
        Apply(settings);
    }
    internal static async Task SaveAsync(string field, object value, CancellationToken token = default)
    {
        await SaveQueue.WaitAsync(token);
        try
        {
            // The current backend accepts field intents, never whole snapshots.
            var patch = JsonSerializer.Serialize(new Dictionary<string, object> { [field] = value });
            Interlocked.Increment(ref _reloadGeneration);
            var committed = await NativeBackend.InvokeAsync<JsonElement>("update_settings", new { settingsJson = patch }, token);
            if (committed.ValueKind != JsonValueKind.Object || !committed.TryGetProperty(field, out _))
                throw new IOException("后台未返回已提交的设置，请更新匹配的后台程序。");
            Interlocked.Increment(ref _reloadGeneration);
            Apply(committed);
        }
        catch
        {
            try { await ReloadAsync(token); } catch { }
            throw;
        }
        finally { SaveQueue.Release(); }
    }
}
