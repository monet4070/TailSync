namespace TailSync.PreviewDemo;

public record DocumentInfo(int PageCount, bool Reflowed);
public record RenderFrame(byte[] Bgra, int Width, int Height)
{
    public int Stride => checked(Width * 4);
    public void Validate()
    {
        Limits.CheckPixels(Width, Height);
        if (Bgra.Length != checked(Stride * Height)) throw new InvalidDataException("Invalid BGRA frame length.");
    }
}

// Every call, including disposal, is serialized by the viewer. Native engine work never runs on the UI thread.
public interface IPreviewEngine : IDisposable
{
    string Name { get; }
    string Version { get; }
    string[] Extensions { get; }
    bool SupportsText { get; }
    Task<DocumentInfo> OpenAsync(byte[] bytes, string extension);
    Task<RenderFrame> RenderAsync(int pageIndex, int pixelWidth);
    Task<string> TextAsync(int pageIndex);
    void Close();
}

public static class Limits
{
    public const long MaxFileBytes = 64 * 1024 * 1024;
    public const long MaxPixels = 16_000_000;
    public const int MaxSide = 8192;
    public const int MaxTextChars = 250_000;
    public static (int Width, int Height) Dimensions(double width, double height, int requestedWidth)
    {
        if (!double.IsFinite(width) || !double.IsFinite(height) || width <= 0 || height <= 0)
            throw new InvalidDataException("Invalid page geometry.");
        if (requestedWidth < 128 || requestedWidth > 4096) throw new ArgumentOutOfRangeException(nameof(requestedWidth));
        double scale = requestedWidth / width;
        var w = Math.Ceiling(width * scale);
        var h = Math.Ceiling(height * scale);
        if (w > MaxSide || h > MaxSide || w * h > MaxPixels)
            throw new InvalidDataException("Page exceeds the 16 MP / 8192 px limit at this zoom. Reduce zoom.");
        return ((int)w, (int)h);
    }
    public static void CheckPixels(int width, int height)
    {
        if (width <= 0 || height <= 0 || width > MaxSide || height > MaxSide || (long)width * height > MaxPixels)
            throw new InvalidDataException("Invalid or oversized page bitmap.");
    }
    public static async Task<byte[]> ReadFileAsync(string path)
    {
        await using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, 65536, true);
        if (stream.Length <= 0 || stream.Length > MaxFileBytes) throw new InvalidDataException("File must be between 1 byte and 64 MiB.");
        var data = new byte[checked((int)stream.Length)];
        await stream.ReadExactlyAsync(data);
        if (stream.ReadByte() != -1) throw new IOException("File changed while reading.");
        return data;
    }
}

public record Options(string? File, string Output, bool Benchmark, int Cycles, int Width, int DwellMs, int IdleMs, int[] Pages)
{
    public static Options Parse(string[] args)
    {
        var values = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var arg in args)
        {
            if (arg == "--benchmark") { values.Add("benchmark", "true"); continue; }
            if (!arg.StartsWith("--") || !arg.Contains('=')) throw new ArgumentException("Use --name=value arguments.");
            var at = arg.IndexOf('='); values.Add(arg[2..at], arg[(at + 1)..]);
        }
        string[] allowed = ["file", "output", "benchmark", "cycles", "width", "dwell-ms", "idle-ms", "pages"];
        foreach (var key in values.Keys) if (!allowed.Contains(key)) throw new ArgumentException($"Unknown option: {key}");
        int Number(string key, int fallback, int min, int max)
        {
            if (!values.TryGetValue(key, out var text)) return fallback;
            if (!int.TryParse(text, out var n) || n < min || n > max) throw new ArgumentException($"Invalid {key}: {text}");
            return n;
        }
        var pages = values.GetValueOrDefault("pages", "1,2,3").Split(',').Select(int.Parse).ToArray();
        if (pages.Length is < 1 or > 100 || pages.Any(x => x is < 1 or > 100000)) throw new ArgumentException("Invalid pages.");
        bool bench = values.ContainsKey("benchmark");
        string? file = values.GetValueOrDefault("file");
        if (bench && string.IsNullOrWhiteSpace(file)) throw new ArgumentException("--benchmark requires --file.");
        return new(file is null ? null : Path.GetFullPath(file),
            Path.GetFullPath(values.GetValueOrDefault("output", Path.Combine(AppContext.BaseDirectory, "results", DateTime.UtcNow.ToString("yyyyMMdd-HHmmss-fff") + "-" + Environment.ProcessId))),
            bench, Number("cycles", 3, 1, 1000), Number("width", 1200, 128, 4096),
            Number("dwell-ms", 500, 100, 60000), Number("idle-ms", 5000, 100, 60000), pages);
    }
}
