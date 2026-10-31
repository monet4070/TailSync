using System.Buffers.Binary;
using System.Text;
using System.Text.Json;

namespace TailSync.Native.Core;

public sealed record PreviewBatch(string BatchId, int ItemIndex, int ItemCount, long? PreviousId, long? NextId);
public sealed record PreviewPayload(long? EntryId, string Kind, string Name, long SizeBytes, int? Width, int? Height,
    PreviewBatch? Batch, ReadOnlyMemory<byte> Data)
{
    public string Extension => Path.GetExtension(Name).TrimStart('.').ToLowerInvariant();
    public string Renderer => Kind == "image" ? "image" : Kind == "text" ? "text" : Extension switch
    {
        "png" or "jpg" or "jpeg" or "gif" or "webp" => "image",
        "svg" => "svg", "md" or "markdown" => "markdown", "pdf" => "pdf", "docx" => "docx", "txt" => "text",
        "c" or "cc" or "cpp" or "cs" or "css" or "go" or "h" or "hpp" or "html" or "java" or "js" or "jsx" or "json" or "kt" or "kts" or "mjs" or "php" or "ps1" or "py" or "rb" or "rs" or "sh" or "sql" or "swift" or "toml" or "ts" or "tsx" or "xml" or "yaml" or "yml" => "code",
        _ => "unsupported"
    };
}

public static class PreviewEnvelope
{
    public const int MaximumBytes = 64 * 1024 * 1024;
    public static PreviewPayload Parse(byte[] bytes)
    {
        if (bytes.Length < 9 || !bytes.AsSpan(0, 4).SequenceEqual("TSPV"u8) || bytes[4] != 1)
            throw new InvalidDataException("预览数据头无效或版本不受支持");
        var length = BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(5, 4));
        if (length == 0 || length > 1024 * 1024 || length > bytes.Length - 9) throw new InvalidDataException("预览元数据长度无效");
        var start = 9 + (int)length;
        var payloadLength = bytes.Length - start;
        if (payloadLength > MaximumBytes) throw new InvalidDataException("预览超过 64 MB 限制");
        // Invalid UTF-8 must never silently become replacement characters.
        var jsonText = new UTF8Encoding(false, true).GetString(bytes, 9, (int)length);
        using var json = JsonDocument.Parse(jsonText);
        var meta = json.RootElement;
        if (meta.ValueKind != JsonValueKind.Object) throw new InvalidDataException("预览元数据无效");
        var allowed = new HashSet<string>(["entry_id", "kind", "name", "size_bytes", "width", "height", "batch"]);
        var properties=meta.EnumerateObject().ToArray();
        if (properties.Any(property => !allowed.Contains(property.Name))||properties.Select(property=>property.Name).Distinct().Count()!=properties.Length) throw new InvalidDataException("预览包含未知或重复元数据字段");
        if(new[]{"kind","name","size_bytes","width","height"}.Any(key=>!meta.TryGetProperty(key,out _)))throw new InvalidDataException("预览缺少必要元数据字段");
        var kind = meta.GetProperty("kind").GetString(); var name = meta.GetProperty("name").GetString();
        var size = meta.GetProperty("size_bytes").GetInt64();
        if (kind is not ("image" or "text" or "file") || string.IsNullOrEmpty(name) || size < 0 || size > MaximumBytes) throw new InvalidDataException("预览元数据值无效");
        var width = NullableInt(meta, "width"); var height = NullableInt(meta, "height");
        if (kind == "image")
        {
            if (width is not > 0 || height is not > 0 || (long)width * height * 4 != payloadLength) throw new InvalidDataException("图片尺寸与 RGBA 数据不一致");
        }
        else if (width.HasValue || height.HasValue || size != payloadLength) throw new InvalidDataException("预览大小与数据不一致");
        var entryId = NullableLong(meta, "entry_id");
        if (entryId < 0) throw new InvalidDataException("预览记录 ID 无效");
        PreviewBatch? batch = null;
        if (meta.TryGetProperty("batch", out var navigation) && navigation.ValueKind != JsonValueKind.Null)
        {
            var batchId = navigation.GetProperty("batch_id").GetString();
            var index = navigation.GetProperty("item_index").GetInt32(); var count = navigation.GetProperty("item_count").GetInt32();
            var previous = NullableLong(navigation, "previous_entry_id"); var next = NullableLong(navigation, "next_entry_id");
            if (string.IsNullOrEmpty(batchId) || batchId.Length > 256 || index < 0 || count <= index || previous <= 0 || next <= 0 || navigation.GetProperty("first_entry_id").GetInt64() <= 0 || navigation.GetProperty("last_entry_id").GetInt64() <= 0)
                throw new InvalidDataException("预览批次导航无效");
            batch = new(batchId, index, count, previous, next);
        }
        return new(entryId, kind, name, size, width, height, batch, bytes.AsMemory(start));
    }
    private static int? NullableInt(JsonElement value, string key) => value.TryGetProperty(key, out var item) && item.ValueKind != JsonValueKind.Null ? item.GetInt32() : null;
    private static long? NullableLong(JsonElement value, string key) => value.TryGetProperty(key, out var item) && item.ValueKind != JsonValueKind.Null ? item.GetInt64() : null;
}
