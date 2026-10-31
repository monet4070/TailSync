using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace TailSync.Wpf;

// Offline visual comparison: synthetic text, no backend or clipboard access.
internal static class SerifSpecimen
{
    internal static async Task Run(App app, string directory)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        ThemeService.Apply("light", "canvas");
        var root = new StackPanel { Margin = new(24) };
        root.Children.Add(Ui.Text("Canvas · 宋体候选实测", 24));
        var note = Ui.Text("同一 WPF 绘制路径 · ClearType / Display · 未缩放的 15–16px 正文", 13, "Secondary"); note.Margin = new(0, 6, 0, 18); root.Children.Add(note);
        var samples = new List<(string Family, FontWeight Weight, TextBlock Text)>();
        foreach (var (family, weight, title) in new[] {
            ("SimSun", FontWeights.Normal, "宋体 · SimSun / Regular"),
            ("STZhongsong", FontWeights.Normal, "华文中宋 · STZhongsong / Regular"),
            ("Noto Serif SC", FontWeights.Normal, "Noto Serif SC · Regular / 400"),
            ("Noto Serif SC", FontWeights.Medium, "Noto Serif SC · Medium / 500") })
        {
            var body = new StackPanel { Margin = new(16, 12, 16, 12) };
            var heading = Ui.Text(title, 13, "Secondary"); heading.Margin = new(0, 0, 0, 9); body.Children.Add(heading);
            foreach (var (size, text) in new[] {
                (15d, "15px  文字、图片与文件，在设备之间安静地同步。"),
                (16d, "16px  清晰的历史记录，让每次复制都有迹可循。"),
                (16d, "Canvas · 剪贴板历史 0123456789 / code-mlx-8bit") })
            {
                var label = Ui.Text(text, size); label.FontFamily = new(family); label.FontWeight = weight;
                label.Margin = new(0, 3, 0, 3); body.Children.Add(label); samples.Add((family, weight, label));
            }
            var card = Ui.Surface(body); card.Margin = new(0, 0, 0, 12); root.Children.Add(card);
        }
        var window = new MacWindow { Content = root, Width = 760, SizeToContent = SizeToContent.Height, ShowActivated = false, ShowInTaskbar = false,
            WindowStartupLocation = WindowStartupLocation.Manual, Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100, Top = 50 };
        window.Show(); await Task.Delay(120); window.UpdateLayout();
        QualityChecks.Capture(window, Path.Combine(directory, "serif-candidates.png"));
        static IEnumerable<GlyphRun> Runs(Drawing? drawing)
        {
            if (drawing is GlyphRunDrawing glyph) yield return glyph.GlyphRun;
            if (drawing is DrawingGroup group) foreach (var child in group.Children) foreach (var run in Runs(child)) yield return run;
        }
        var observations = samples.Select(sample => new { sample.Family, requestedWeight = sample.Weight.ToOpenTypeWeight(), sample.Text.FontSize,
            actualFaces = Runs(VisualTreeHelper.GetDrawing(sample.Text)).Select(run => new { file = run.GlyphTypeface.FontUri.ToString(),
                weight = run.GlyphTypeface.Weight.ToOpenTypeWeight(), missingGlyph = run.GlyphIndices.Any(index => index == 0) }).ToArray() }).ToArray();
        await File.WriteAllTextAsync(Path.Combine(directory, "serif-candidates.json"), JsonSerializer.Serialize(new { dpi = VisualTreeHelper.GetDpi(window).DpiScaleX, observations }, new JsonSerializerOptions { WriteIndented = true }));
        window.Close(); app.Shutdown();
    }
}
