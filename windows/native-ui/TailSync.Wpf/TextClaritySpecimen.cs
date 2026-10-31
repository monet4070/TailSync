using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace TailSync.Wpf;

// Compare identical synthetic copy before changing production typography.
internal static class TextClaritySpecimen
{
    internal static async Task Run(App app, string directory, bool review)
    {
        Directory.CreateDirectory(directory); app.ShutdownMode = ShutdownMode.OnExplicitShutdown;
        ThemeService.Apply("light", "canvas");
        var root = new StackPanel { Margin = new(20) };
        root.Children.Add(Ui.Text("文字清晰度对比 · 合成样例", 22));
        var samples = new WrapPanel { Margin = new(0, 12, 0, 0) }; root.Children.Add(samples);
        var labels = new List<(string Candidate, TextBlock Text)>();
        foreach (var (title, weight, formatting, rendering) in new[] {
            ("当前 · 400 / Display", FontWeights.Normal, TextFormattingMode.Display, TextRenderingMode.ClearType),
            ("500 / Display", FontWeights.Medium, TextFormattingMode.Display, TextRenderingMode.ClearType),
            ("Noto Sans SC 500 / Display", FontWeights.Medium, TextFormattingMode.Display, TextRenderingMode.ClearType) })
        {
            var body = new StackPanel { Margin = new(16) };
            var card = Ui.Surface(body); card.Width = 366; card.Margin = new(0, 0, 12, 12); samples.Children.Add(card);
            body.Children.Add(Ui.Text(title, 14));
            foreach (var size in new[] { 12d, 13d, 14d })
            {
                var label = Ui.Text($"{size}px  管理同步、快捷键和应用偏好", size, "Secondary");
                label.FontWeight = weight; label.Margin = new(0, 14, 0, 0); body.Children.Add(label);
                label.FontFamily = title.StartsWith("Noto") ? new("Noto Sans SC, Segoe UI Variable Text, Segoe UI, Microsoft YaHei UI") : new("Segoe UI Variable Text, Segoe UI, Microsoft YaHei UI");
                TextOptions.SetTextFormattingMode(label, formatting); TextOptions.SetTextRenderingMode(label, rendering);
                labels.Add((title, label));
            }
            Ui.Divider(body);
            foreach (var color in new[] { "Primary", "Secondary" })
            {
                var label = Ui.Text("清晰的历史记录，让每次复制都有迹可循。", 16, color);
                label.SetResourceReference(TextBlock.FontFamilyProperty, "HistoryFont"); label.FontWeight = weight;
                label.Margin = new(0, 14, 0, 0); body.Children.Add(label);
                TextOptions.SetTextFormattingMode(label, formatting); TextOptions.SetTextRenderingMode(label, rendering);
                labels.Add((title, label));
            }
            var mixed = Ui.Text("Canvas · 图文记录 / history-012345", 16); mixed.SetResourceReference(TextBlock.FontFamilyProperty, "HistoryFont"); mixed.FontWeight = weight;
            mixed.Margin = new(0, 14, 0, 0); body.Children.Add(mixed);
            TextOptions.SetTextFormattingMode(mixed, formatting); TextOptions.SetTextRenderingMode(mixed, rendering);
        }
        var window = new MacWindow { Title = "文字清晰度 · 隔离样例", Content = root, Width = 1180, SizeToContent = SizeToContent.Height,
            ShowActivated = review, ShowInTaskbar = review, WindowStartupLocation = review ? WindowStartupLocation.CenterScreen : WindowStartupLocation.Manual,
            ResizeMode = ResizeMode.NoResize };
        if (!review) { window.Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100; window.Top = 50; }
        window.Show(); await Task.Delay(120); window.UpdateLayout();
        QualityChecks.Capture(window, Path.Combine(directory, "clarity-comparison.png"));
        static IEnumerable<GlyphRun> Runs(Drawing? drawing)
        {
            if (drawing is GlyphRunDrawing glyph) yield return glyph.GlyphRun;
            if (drawing is DrawingGroup group) foreach (var child in group.Children) foreach (var run in Runs(child)) yield return run;
        }
        var observations = labels.Select(sample => new { sample.Candidate, sample.Text.FontSize, requestedWeight = sample.Text.FontWeight.ToOpenTypeWeight(),
            glyphs = Runs(VisualTreeHelper.GetDrawing(sample.Text)).Select(run => new { file = run.GlyphTypeface.FontUri.ToString(), actualWeight = run.GlyphTypeface.Weight.ToOpenTypeWeight(), missing = run.GlyphIndices.Any(index => index == 0) }).ToArray() }).ToArray();
        await File.WriteAllTextAsync(Path.Combine(directory, "glyphs.json"), JsonSerializer.Serialize(observations, new JsonSerializerOptions { WriteIndented = true }));
        await File.WriteAllTextAsync(Path.Combine(directory, "ready.json"), JsonSerializer.Serialize(new { pid = Environment.ProcessId,
            dpi = VisualTreeHelper.GetDpi(window).DpiScaleX, window.Width, window.ActualHeight, source = "synthetic", review }));
        if (review) { var closed = new TaskCompletionSource(); window.Closed += (_, _) => closed.TrySetResult(); await closed.Task; }
        else window.Close();
        app.Shutdown();
    }
}
