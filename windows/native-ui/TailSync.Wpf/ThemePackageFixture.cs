using System.IO;
using System.IO.Compression;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace TailSync.Wpf;

// Integration-test package only. Its destination must be an explicitly supplied
// test directory; neither the installed theme folder nor user preferences are read.
internal static class ThemePackageFixture
{
    internal const string ThemeId = "custom:wpf.migration";
    internal const string DisplayFont = "Georgia";

    internal static string Create(string directory, string version)
    {
        if (!Regex.IsMatch(version, @"^[0-9A-Za-z.+-]+$")) throw new ArgumentException("Invalid fixture version", nameof(version));
        Directory.CreateDirectory(directory);
        var path = Path.Combine(Path.GetFullPath(directory), "wpf-migration-" + version + "-" + Guid.NewGuid().ToString("N") + ".tailsync-theme");
        var manifest = new
        {
            formatVersion = 2, id = ThemeId, version, minCoreVersion = "0.1.0",
            name = new Dictionary<string, string> { ["en"] = "WPF migration fixture", ["zh-CN"] = "WPF 迁移测试主题" },
            extends = "builtin:canvas@1",
            light = new { colors = new { background = new { canvas = "#ffffff" }, text = new { primary = "#111111" }, accent = new { @default = version == "1.0.0" ? "#315b82" : "#72519c" } } },
            dark = new { colors = new { background = new { canvas = "#111111" }, text = new { primary = "#ffffff" } } },
            foundation = new
            {
                typography = new
                {
                    ui = new { families = new[] { "Segoe UI", "Microsoft YaHei UI", "sans-serif" }, size = 14, lineHeight = 22 },
                    display = new { families = new[] { DisplayFont, "Microsoft YaHei UI", "serif" } },
                    reading = new { families = new[] { "Consolas", "Microsoft YaHei UI", "monospace" } },
                    search = new { size = 20, useDisplayFont = true }, history = new { size = 15 }
                },
                shape = new { controlRadius = 6 }
            },
            assetSlots = new { logo = "assets/logo.png" }
        };
        using var output = File.Create(path);
        using var archive = new ZipArchive(output, ZipArchiveMode.Create);
        using (var data = archive.CreateEntry("theme.json").Open()) JsonSerializer.Serialize(data, manifest);
        // A real decodable PNG also exercises the binary asset command, unlike
        // the intentionally truncated header used by several Core parser tests.
        var bitmap = BitmapSource.Create(2, 2, 96, 96, PixelFormats.Bgra32, null,
            new byte[] { 130, 91, 49, 255, 255, 255, 255, 255, 255, 255, 255, 255, 130, 91, 49, 255 }, 8);
        var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap));
        using var encoded = new MemoryStream(); encoder.Save(encoded); encoded.Position = 0;
        using (var image = archive.CreateEntry("assets/logo.png").Open()) encoded.CopyTo(image);
        return path;
    }
}
