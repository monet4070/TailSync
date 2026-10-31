using System.IO;
using System.Text.Json;
using TailSync.Native.Core;

namespace TailSync.Wpf;

// Owned by one window and called on its dispatcher. Recycled containers and
// replacement query snapshots share requests by Core's current content key.
internal sealed class HistoryThumbnails
{
    private readonly Func<long, string?, CancellationToken, Task<JsonElement>> _fetch;
    private readonly SemaphoreSlim _slots = new(2);
    private readonly Dictionary<(long, string), Task<byte[]?>> _requests = [];
    private readonly Dictionary<(long, string), byte[]> _cache = [];
    private readonly Queue<(long, string)> _order = [];
    private int _generation, _bytes;

    internal HistoryThumbnails(Func<long, string?, CancellationToken, Task<JsonElement>>? fetch = null)
        => _fetch = fetch ?? ((id, key, token) => NativeBackend.InvokeAsync<JsonElement>("get_image_data", new { id, cacheKey = key }, token));

    internal Task<byte[]?> GetAsync(HistoryEntry entry, CancellationToken token)
    {
        if (TourHistoryClient.IsVirtual(entry.Id)) return Task.FromResult(GuidedTour.Active ? entry.ThumbnailBytes : null);
        if (!entry.ThumbnailEligible || token.IsCancellationRequested) return Task.FromResult<byte[]?>(null);
        var key = (entry.Id, entry.DisplayCacheKey.Length > 0 ? entry.DisplayCacheKey : entry.DataHash);
        if (_cache.TryGetValue(key, out var cached)) return Task.FromResult<byte[]?>(cached);
        if (_requests.TryGetValue(key, out var pending)) return pending;
        if (_requests.Count >= 64) return Task.FromResult<byte[]?>(null);
        var completion = new TaskCompletionSource<byte[]?>(TaskCreationOptions.RunContinuationsAsynchronously);
        _requests.Add(key, completion.Task);
        _ = Load(entry.Id, entry.DisplayCacheKey, key, _generation, completion, token);
        return completion.Task;
    }
    internal void Clear()
    {
        ++_generation; _requests.Clear(); _cache.Clear(); _order.Clear(); _bytes = 0;
    }
    private async Task Load(long id, string expected, (long, string) key, int generation, TaskCompletionSource<byte[]?> completion, CancellationToken token)
    {
        byte[]? image = null; var acquired = false;
        try
        {
            await _slots.WaitAsync(token); acquired = true;
            if (generation != _generation) return;
            JsonElement result = default;
            for (var attempt = 0; attempt < 16; attempt++)
            {
                if (generation != _generation) return;
                try { result = await _fetch(id, expected.Length > 0 ? expected : null, token); break; }
                catch (BackendException error) when (error.Detail.String("code") == "temporarily_busy" && attempt < 15)
                { await Task.Delay(Math.Min(400, 50 * (attempt + 1)), token); }
            }
            token.ThrowIfCancellationRequested();
            if (generation != _generation || expected.Length > 0 && result.String("cache_key") != expected) return;
            var width = result.Long("thumbnail_width"); var height = result.Long("thumbnail_height");
            var encoded = result.String("thumbnail_b64");
            if (width is <= 0 or > 256 || height is <= 0 or > 256 || encoded.Length is 0 or > 350_000)
                throw new InvalidDataException("Invalid thumbnail dimensions or size.");
            var rgba = Convert.FromBase64String(encoded);
            if (rgba.LongLength != width * height * 4) throw new InvalidDataException("Incomplete thumbnail pixels.");
            var png = await Task.Run(() => PreviewRenderers.Png(PreviewRenderers.Rgba(rgba, (int)width, (int)height)), token);
            token.ThrowIfCancellationRequested();
            if (generation != _generation) return;
            image = png; _cache.Add(key, png); _order.Enqueue(key); _bytes += png.Length;
            while (_cache.Count > 64 || _bytes > 4 * 1024 * 1024)
                if (_cache.Remove(_order.Dequeue(), out var removed)) _bytes -= removed.Length;
        }
        catch (OperationCanceledException) { }
        catch (Exception error) when (error is BackendException or IOException or FormatException or ArgumentException or InvalidOperationException)
        { System.Diagnostics.Debug.WriteLine($"Thumbnail {id}: {error.GetType().Name}"); }
        finally
        {
            if (acquired) _slots.Release();
            if (_requests.TryGetValue(key, out var pending) && ReferenceEquals(pending, completion.Task)) _requests.Remove(key);
            completion.TrySetResult(image);
        }
    }
}
