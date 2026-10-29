using TailSync.PreviewDemo;

int checks = 0;
void Check(bool value, string name) { if (!value) throw new Exception(name); checks++; Console.WriteLine("PASS " + name); }
void Reject(Action action, string name)
{
    try { action(); } catch (Exception) { checks++; Console.WriteLine("PASS " + name); return; }
    throw new Exception("Accepted invalid input: " + name);
}
Check(Limits.Dimensions(595, 842, 1200) == (1200, 1699), "PDF aspect ratio and pixel dimensions");
Check(Limits.Dimensions(842, 595, 1200) == (1200, 848), "landscape dimensions");
Reject(() => Limits.Dimensions(0, 100, 1200), "zero page width");
Reject(() => Limits.Dimensions(double.NaN, 100, 1200), "NaN dimensions");
Reject(() => Limits.Dimensions(1, 100000, 4096), "extreme aspect ratio before allocation");
Reject(() => Limits.CheckPixels(8192, 8192), "decoded pixel budget");
Reject(() => new RenderFrame(new byte[1], 100, 100).Validate(), "invalid pixel buffer");
Reject(() => Options.Parse(["--benchmark"]), "benchmark without input");
Reject(() => Options.Parse(["--cycles=0"]), "zero cycles");
Reject(() => Options.Parse(["--width=9000"]), "unbounded raster size");
Reject(() => Options.Parse(["--pages=0"]), "invalid page number");
Reject(() => Options.Parse(["--typo=1"]), "unknown benchmark option");
Reject(() => Options.Parse(["--width=1200", "--width=2400"]), "duplicate option");
var options = Options.Parse(["--benchmark", "--file=测试 file.pdf", "--cycles=2", "--pages=3,1,2"]);
Check(options.Cycles == 2 && options.Pages.SequenceEqual([3, 1, 2]) && options.File!.EndsWith("测试 file.pdf"), "Unicode filename and explicit page sequence");
var temp = Path.Combine(Path.GetTempPath(), "tailsync-preview-check-" + Guid.NewGuid());
try
{
    await File.WriteAllBytesAsync(temp, [1, 2, 3]);
    Check((await Limits.ReadFileAsync(temp)).SequenceEqual(new byte[] { 1, 2, 3 }), "exact byte input");
    using (var stream = File.Create(temp)) stream.SetLength(Limits.MaxFileBytes + 1);
    bool rejected = false;
    try { await Limits.ReadFileAsync(temp); } catch (InvalidDataException) { rejected = true; }
    Check(rejected, "oversized file rejected before reading");
}
finally { File.Delete(temp); }
Console.WriteLine($"{checks} checks passed.");
