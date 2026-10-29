using System.Security.Cryptography;
using System.Text.Json;
using TailSync.PreviewDemo;

// Native-engine smoke check only. It is not a Windows UI or performance result.
if (args.Length < 1) throw new ArgumentException("Pass a fixture file.");
using var engine = EngineFactory.Create();
foreach (var path in args)
{
    var data = await Limits.ReadFileAsync(path);
    for (int cycle = 0; cycle < 3; cycle++)
    {
        var info = await engine.OpenAsync(data, Path.GetExtension(path));
        if (info.PageCount < 1) throw new Exception("No pages.");
        foreach (var index in new[] { 0, info.PageCount - 1 }.Distinct())
        {
            var frame = await engine.RenderAsync(index, 1200); frame.Validate();
            var text = engine.SupportsText ? await engine.TextAsync(index) : "";
            Console.WriteLine(JsonSerializer.Serialize(new { engine = engine.Name, file = Path.GetFileName(path), cycle, page = index + 1, pages = info.PageCount, reflowed = info.Reflowed, frame.Width, frame.Height, text_chars = text.Length, pixel_sha256 = Convert.ToHexString(SHA256.HashData(frame.Bgra)) }));
        }
        engine.Close();
    }
}
