using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json.Serialization;

namespace TailSync.Native.Core;

// Production wire fields retain their existing names. Title, Time, Icon and
// Preview describe demo presentation and are not new local-contract fields.
public sealed class HistoryEntry : INotifyPropertyChanged
{
    public static Func<string,string>? Localize { get; set; }
    private bool _pinned;
    private int _shortcutNumber;
    public int ShortcutNumber { get => _shortcutNumber; set { _shortcutNumber = value; OnPropertyChanged(); OnPropertyChanged(nameof(HasShortcutNumber)); } }
    public bool HasShortcutNumber => ShortcutNumber > 0;
    [JsonIgnore] public System.Text.Json.JsonElement? CompositeIdentity { get; init; }
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
    [JsonPropertyName("batch_count")] public int? BatchCount { get; init; }
    [JsonPropertyName("batch_index")] public int? BatchIndex { get; init; }
    [JsonPropertyName("batch_status")] public string BatchStatus { get; init; } = "complete";
    [JsonPropertyName("data_hash")] public string DataHash { get; init; } = "";
    // Native presentation of Core's versioned `display` metadata. The DTO is
    // projected by the platform client; these are not replacement wire fields.
    [JsonIgnore] public string DisplayCacheKey { get; init; } = "";
    [JsonIgnore] public bool ThumbnailEligible { get; init; }
    [JsonIgnore] public string AllCategoryLabels => string.Join(" · ", Categories.Select(CategoryLabel));
    [JsonIgnore] public bool HasBatch => !string.IsNullOrWhiteSpace(BatchId);
    [JsonIgnore] public string BatchSummary => $"{BatchCount ?? BatchTotal ?? 1}/{BatchTotal ?? 1} {Localize?.Invoke("history.files") ?? "文件"}" + (BatchStatus == "incomplete" ? " · " + (Localize?.Invoke("history.incomplete") ?? "尚未完成") : "");
    [JsonIgnore] public string DateGroup { get; set; } = "";
    [JsonIgnore] public bool ShowDateGroup { get; set; }
    [JsonIgnore] public bool ShowBatchHeader { get; set; }
    [JsonIgnore] public string BatchToggleLabel { get; set; } = "展开";
    [JsonIgnore] public bool ShowBatchToggle => ShowBatchHeader && (BatchCount ?? BatchTotal ?? 1) > 3;
    private byte[]? _thumbnailBytes;
    [JsonIgnore] public byte[]? ThumbnailBytes { get => _thumbnailBytes; set { _thumbnailBytes = value; OnPropertyChanged(); } }
    private double _favoriteProgress;
    [JsonIgnore] public double FavoriteProgress { get => _favoriteProgress; set { _favoriteProgress=value;OnPropertyChanged(); } }
    private int _favoriteCompletion;
    [JsonIgnore] public int FavoriteCompletion { get => _favoriteCompletion; set { _favoriteCompletion=value;OnPropertyChanged(); } }
    public static string CategoryLabel(string category) => Localize?.Invoke("history.category." + category) ?? (category switch
    {
        "website" => "链接", "code" => "代码", "command" => "命令", "structured_data" => "结构化数据",
        "image" => "图片", "file" => "文件", "path" => "路径", _ => "文本"
    });
    public void RefreshPresentation() => OnPropertyChanged("");
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
    public string KindLabel => DisplayKind ?? CategoryLabel(Categories.FirstOrDefault() ?? "text");
    public string SizeLabel => SizeBytes >= 1024 * 1024
        ? $"{SizeBytes / 1048576d:0.#} MB" : SizeBytes >= 1024
        ? $"{SizeBytes / 1024d:0.#} KB" : $"{SizeBytes} B";

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new(name));
}
