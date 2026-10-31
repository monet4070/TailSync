using System.IO;
using System.Text.Json;
using System.Windows.Media.Imaging;

namespace TailSync.Wpf;

internal static class ThemeV2Assets
{
    internal static readonly string[] Slots = ["logo", "emptyState", "previewPlaceholder"];
    internal static string ResourceKey(string slot) => "Theme" + char.ToUpperInvariant(slot[0]) + slot[1..] + "Image";

    internal static async Task<Dictionary<string, BitmapImage>> LoadAsync(JsonElement resolved, string? previewPath, CancellationToken token)
    {
        var result = new Dictionary<string, BitmapImage>();
        foreach (var slot in Slots)
        {
            token.ThrowIfCancellationRequested();
            if (ThemeV2.Value(resolved, "assetSlots/" + slot) is not { ValueKind: JsonValueKind.Object } descriptor) continue;
            // The Core decoder also validates image MIME/dimensions and the
            // package digest. Keep a renderer-side allocation bound as well.
            if (ThemeV2.Number(descriptor, "bytes") is not (> 0 and <= 10 * 1024 * 1024)) continue;
            try
            {
                var digest = ThemeV2.Text(resolved, "digest");
                var response = previewPath == null
                    ? await NativeBackend.InvokeAsync<JsonElement>("get_theme_asset_slot", new { themeId = ThemeV2.Text(resolved, "id"), digest, slot }, token)
                    : await NativeBackend.InvokeAsync<JsonElement>("preview_theme_asset_slot", new { path = previewPath, digest, slot }, token);
                token.ThrowIfCancellationRequested();
                var encoded = ThemeV2.Text(response, "binaryBase64");
                if (encoded.Length is 0 or > 14 * 1024 * 1024) continue;
                var bytes = Convert.FromBase64String(encoded);
                if (bytes.Length > 10 * 1024 * 1024) continue;
                using var stream = new MemoryStream(bytes, false);
                var bitmap = new BitmapImage(); bitmap.BeginInit(); bitmap.CacheOption = BitmapCacheOption.OnLoad;
                var width = ThemeV2.Number(descriptor, "width") ?? 512;
                var height = ThemeV2.Number(descriptor, "height") ?? 512;
                if (width >= height) bitmap.DecodePixelWidth = (int)Math.Clamp(width, 1, 512);
                else bitmap.DecodePixelHeight = (int)Math.Clamp(height, 1, 512);
                bitmap.StreamSource = stream; bitmap.EndInit(); bitmap.Freeze();
                result[slot] = bitmap;
            }
            catch (OperationCanceledException) { throw; }
            catch (Exception)
            {
                // An optional image cannot invalidate an otherwise valid theme.
            }
        }
        return result;
    }
}
