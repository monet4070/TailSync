using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace TailSync.PreviewDemo;

public sealed class Telemetry : IDisposable
{
    private readonly StreamWriter _writer;
    private readonly object _sync = new();
    private readonly Stopwatch _clock = Stopwatch.StartNew();
    private readonly CancellationTokenSource _stop = new();
    private readonly Task _sampler;
    private readonly Process _process = Process.GetCurrentProcess();
    private bool _disposed;
    public double ElapsedMs => _clock.Elapsed.TotalMilliseconds;
    public string Output { get; }
    public Telemetry(string output, IPreviewEngine engine, Options options)
    {
        Output = output;
        Directory.CreateDirectory(output);
        // Each run is immutable. Accidental reuse must not overwrite a previous measurement.
        _writer = new StreamWriter(new FileStream(Path.Combine(output, "events.jsonl"), FileMode.CreateNew, FileAccess.Write, FileShare.Read));
        Event("environment", new
        {
            schema = 1, engine = engine.Name, engine_version = engine.Version, pid = Environment.ProcessId,
            os = RuntimeInformation.OSDescription, arch = RuntimeInformation.ProcessArchitecture.ToString(),
            framework = RuntimeInformation.FrameworkDescription, processors = Environment.ProcessorCount,
            process_start_utc = _process.StartTime.ToUniversalTime(), build = "Release recommended",
            ui = "shared WPF viewer", text_supported = engine.SupportsText, extensions = engine.Extensions,
            options, sample_interval_ms = 250, no_frame_cache = true,
            notes = "In-process engine; samples include viewer and engine. Working set is not private working set. No forced GC or working-set trim. Render tick is not physical presentation."
        });
        _sampler = Task.Run(async () =>
        {
            using var timer = new PeriodicTimer(TimeSpan.FromMilliseconds(250));
            try { do { Sample(); } while (await timer.WaitForNextTickAsync(_stop.Token)); }
            catch (OperationCanceledException) { }
        });
    }
    public void Event(string kind, object? data = null)
    {
        lock (_sync)
        {
            if (_disposed) return;
            _writer.WriteLine(JsonSerializer.Serialize(new { kind, elapsed_ms = ElapsedMs, utc = DateTime.UtcNow, data }));
            _writer.Flush();
        }
    }
    public void Sample()
    {
        try
        {
            _process.Refresh();
            var native = new MemoryCounters { Size = (uint)Marshal.SizeOf<MemoryCounters>() };
            bool hasExtended = GetProcessMemoryInfo(_process.Handle, ref native, native.Size);
            Event("resource", new
            {
                private_bytes = _process.PrivateMemorySize64,
                working_set_bytes = _process.WorkingSet64,
                private_working_set_bytes = hasExtended ? (long?)native.PrivateWorkingSet : null,
                peak_working_set_bytes = _process.PeakWorkingSet64,
                cpu_total_ms = _process.TotalProcessorTime.TotalMilliseconds,
                handles = _process.HandleCount, threads = _process.Threads.Count,
                managed_heap_bytes = GC.GetTotalMemory(false)
            });
        }
        catch (Exception error) { Event("sample_error", new { error = error.Message }); }
    }
    public void Dispose()
    {
        _stop.Cancel(); _sampler.GetAwaiter().GetResult(); Sample();
        lock (_sync) { _disposed = true; _writer.Dispose(); }
        _process.Dispose(); _stop.Dispose();
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct MemoryCounters
    {
        public uint Size, PageFaultCount;
        public nuint PeakWorkingSet, WorkingSet, QuotaPeakPagedPool, QuotaPagedPool,
            QuotaPeakNonPagedPool, QuotaNonPagedPool, Pagefile, PeakPagefile, PrivateUsage, PrivateWorkingSet;
        public ulong SharedCommit;
    }
    [DllImport("psapi.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetProcessMemoryInfo(IntPtr process, ref MemoryCounters counters, uint size);
}
