using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json.Serialization;

namespace TailSync.Native.Core;

// Production wire fields retain their existing names. Title, Time, Icon and
// Preview describe demo presentation and are not new local-contract fields.
public sealed class HistoryEntry : INotifyPropertyChanged
{
    private bool _pinned;
    [JsonPropertyName("id")] public long Id { get; init; }
    [JsonPropertyName("composite")] public bool IsComposite { get; init; }
    [JsonPropertyName("local_only")] public bool LocalOnly { get; init; }
    [JsonPropertyName("display_kind")] public string? DisplayKind { get; init; }
    public bool HasFormatMenu => IsComposite && !LocalOnly;
    [JsonPropertyName("type")] public string Type { get; init; } = "text";
    [JsonPropertyName("title")] public string Title { get; init; } = "";
    [JsonPropertyName("description")] public string Description { get; init; } = "";
    [JsonPropertyName("body")] public string Body { get; init; } = "";
    [JsonPropertyName("timestamp")] public DateTimeOffset Timestamp { get; init; }
    [JsonPropertyName("time")] public string Time { get; init; } = "";
    [JsonPropertyName("source_peer")] public string SourcePeer { get; init; } = "";
    [JsonPropertyName("size_bytes")] public long SizeBytes { get; init; }
    [JsonPropertyName("categories")] public string[] Categories { get; init; } = [];
    [JsonPropertyName("icon")] public string Icon { get; init; } = "text";
    [JsonPropertyName("preview")] public string Preview { get; init; } = "text";
    [JsonPropertyName("batch_id")] public string? BatchId { get; init; }
    [JsonPropertyName("batch_total")] public int? BatchTotal { get; init; }
    [JsonPropertyName("pinned")]
    public bool Pinned
    {
        get => _pinned;
        set
        {
            if (_pinned == value) return;
            _pinned = value;
            OnPropertyChanged();
            OnPropertyChanged(nameof(FavoriteLabel));
            OnPropertyChanged(nameof(FavoriteGlyph));
        }
    }

    public double RowHeight => LocalOnly ? 66 : 75;
    public string RowSizeLine => LocalOnly ? "仅本机恢复  " + SizeLabel : SizeLabel;
    public string FavoriteLabel => Pinned ? "取消收藏" : "添加收藏";
    public string FavoriteGlyph => Pinned ? "\uE735" : "\uE734";
    public string SourceLine => $"{SourcePeer}  ·  {Time}";
    public string Glyph => Icon switch
    {
        "globe" => "\uE774", "code" => "\uE943", "image" => "\uEB9F",
        "folder" => "\uE8B7", "file" => "\uE8A5", _ => "\uE8E9"
    };
    public string KindLabel => DisplayKind ?? (Categories.FirstOrDefault() switch
    {
        "website" => "链接", "code" => "代码", "image" => "图片",
        "file" => "文件", "path" => "路径", _ => "文本"
    });
    public string SizeLabel => SizeBytes >= 1024 * 1024
        ? $"{SizeBytes / 1048576d:0.#} MB" : SizeBytes >= 1024
        ? $"{SizeBytes / 1024d:0.#} KB" : $"{SizeBytes} B";

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new(name));
}
