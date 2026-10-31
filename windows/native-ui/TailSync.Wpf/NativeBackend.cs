using System.Buffers.Binary;
using System.Diagnostics;
using System.IO;
using System.IO.Pipes;
using System.Security.Cryptography;
using System.Text.Json;
using System.Windows;

namespace TailSync.Wpf;

// Every request uses a fresh authenticated connection, so long polls never block
// mutations. The server also restricts the pipe to the current Windows account.
public static class NativeBackend
{
    internal const int MaximumJsonBytes = 1024 * 1024;
    internal const int MaximumBinaryBytes = 70 * 1024 * 1024;
    private static string? _pipe, _secret;
    private static Process? _process;
    private static CancellationTokenSource? _lifetime;
    private static long _request;
    public static bool IsConnected { get; private set; }
    internal static int ProcessId => _process?.Id ?? 0;
    public static bool IsIsolated { get; private set; }
    public static bool NetworkEnabled { get; private set; }
    public static IReadOnlyList<string> ConnectionModes { get; private set; } = [];
    public static JsonElement RuntimeSnapshot { get; private set; }
    public static event EventHandler? RuntimeChanged;
    public static event EventHandler<JsonElement>? NavigationChanged;
    public static event EventHandler<string>? ConnectionFailed;
    public static JsonSerializerOptions JsonOptions { get; } = new() { PropertyNameCaseInsensitive = true };

    internal static async Task StartAsync(string? executable = null, string? isolatedRoot = null, CancellationToken token = default, bool networkCheck = false)
    {
        if (_process != null) throw new InvalidOperationException("Backend already started");
        executable ??= Environment.GetEnvironmentVariable("TAILSYNC_WPF_BACKEND_EXE");
        executable ??= Path.Combine(AppContext.BaseDirectory, "TailSync.Backend.exe");
        if (!File.Exists(executable)) throw new FileNotFoundException("找不到 TailSync 后台。请运行完整的 WPF 发布目录，或设置 TAILSYNC_WPF_BACKEND_EXE。", executable);
        _pipe = "TailSync.Wpf." + Guid.NewGuid().ToString("N");
        _secret = Convert.ToHexString(RandomNumberGenerator.GetBytes(32)).ToLowerInvariant();
        var start = new ProcessStartInfo(Path.GetFullPath(executable)) { UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = Path.GetDirectoryName(Path.GetFullPath(executable))! };
        start.ArgumentList.Add("--wpf-backend");
        start.ArgumentList.Add("--pipe=" + _pipe);
        start.ArgumentList.Add("--parent-pid=" + Environment.ProcessId);
        start.Environment["TAILSYNC_WPF_SESSION_TOKEN"] = _secret;
        IsIsolated = isolatedRoot != null;
        if (isolatedRoot != null)
        {
            start.ArgumentList.Add("--wpf-isolated");
            start.Environment["TAILSYNC_DATA_DIR"] = Path.GetFullPath(isolatedRoot);
            if (networkCheck) start.ArgumentList.Add("--wpf-network-check");
        }
        _process = Process.Start(start) ?? throw new IOException("无法启动 TailSync 后台");
        try
        {
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(token);
            timeout.CancelAfter(TimeSpan.FromSeconds(40));
            var handshake=SendAsync("handshake",null,timeout.Token);
            var exited=_process.WaitForExitAsync(timeout.Token);
            if(await Task.WhenAny(handshake,exited)==exited)
                throw new IOException("TailSync 后台在连接前退出。请关闭其他 TailSync 实例后重试，并检查后台启动日志。");
            var hello=await handshake;
            timeout.Cancel();
            if (hello.GetProperty("protocolVersion").GetInt32() != 1) throw new IOException("不兼容的 WPF 后台协议版本");
            if (hello.Long("nativeUiContractVersion") < 2 || hello.String("settingsContract") != "field-patch-v1")
                throw new IOException("界面与后台版本不匹配。请从同一个完整发布目录启动 TailSync，更新 WPF 与 Backend 两个程序。");
            IsIsolated = hello.Bool("isolated"); NetworkEnabled = hello.Bool("networkEnabled");
            ConnectionModes = hello.Object("connectionModes").Items().Select(mode => mode.GetString()!).ToArray();
            IsConnected = true;
        }
        catch
        {
            await StopAsync(); throw;
        }
    }

    internal static void BeginWatching()
    {
        if (!IsConnected || _lifetime != null) return;
        _lifetime = new();
        _ = RuntimeLoop(_lifetime.Token);
        _ = NavigationLoop(_lifetime.Token);
    }

    public static async Task<T> InvokeAsync<T>(string command, object? args = null, CancellationToken token = default)
    {
        if (!IsConnected) throw new IOException("TailSync 后台未连接");
        var value = await SendAsync(command, args, token);
        if (typeof(T) == typeof(JsonElement)) return (T)(object)value;
        return value.Deserialize<T>(JsonOptions)!;
    }
    public static async Task InvokeAsync(string command, object? args = null, CancellationToken token = default)
        => _ = await InvokeAsync<JsonElement>(command, args, token);

    public static async Task<byte[]> InvokeBinaryAsync(string command, object? args = null, CancellationToken token = default)
    {
        if (!IsConnected) throw new IOException("TailSync 后台未连接");
        var response = await SendCoreAsync(command, args, token);
        return response.Binary ?? throw new IOException("后台未返回二进制内容");
    }

    private sealed record BackendResponse(JsonElement Json, byte[]? Binary = null);

    private static async Task<JsonElement> SendAsync(string command, object? args, CancellationToken cancellation)
    {
        var response = await SendCoreAsync(command, args, cancellation);
        return response.Binary == null ? response.Json : JsonSerializer.SerializeToElement(new { binaryBase64 = Convert.ToBase64String(response.Binary) });
    }

    private static async Task<BackendResponse> SendCoreAsync(string command, object? args, CancellationToken cancellation)
    {
        GuidedTour.CheckCommand(command, args);
        if (_pipe == null || _secret == null) throw new IOException("WPF 后台会话尚未建立");
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellation);
        timeout.CancelAfter(TimeSpan.FromSeconds(125));
        var token = timeout.Token;
        await using var pipe = new NamedPipeClientStream(".", _pipe, PipeDirection.InOut, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        await pipe.ConnectAsync(token);
        var id = Interlocked.Increment(ref _request);
        var bytes = JsonSerializer.SerializeToUtf8Bytes(new { id, token = _secret, command, args = args ?? new { } });
        if (bytes.Length > MaximumJsonBytes) throw new IOException("后台请求超过大小限制");
        var length = new byte[4]; BinaryPrimitives.WriteInt32LittleEndian(length, bytes.Length);
        await pipe.WriteAsync(length, token); await pipe.WriteAsync(bytes, token); await pipe.FlushAsync(token);
        await pipe.ReadExactlyAsync(length, token);
        var count = BinaryPrimitives.ReadInt32LittleEndian(length);
        if (count <= 0 || count > MaximumJsonBytes) throw new IOException("无效的后台响应大小");
        var response = new byte[count]; await pipe.ReadExactlyAsync(response, token);
        using var json = JsonDocument.Parse(response);
        var root = json.RootElement;
        if (root.GetProperty("id").GetInt64() != id) throw new IOException("后台响应标识不匹配");
        if (!root.GetProperty("ok").GetBoolean())
            throw new BackendException(root.TryGetProperty("error", out var error) ? error.Clone() : default);
        if (root.TryGetProperty("binaryLength", out var binaryLength) && binaryLength.GetInt32() > 0)
        {
            var size = binaryLength.GetInt32();
            if (size > MaximumBinaryBytes) throw new IOException("预览超过大小限制");
            var data = new byte[size]; await pipe.ReadExactlyAsync(data, token);
            return new(default, data);
        }
        return new(root.TryGetProperty("result", out var result) ? result.Clone() : JsonSerializer.SerializeToElement<object?>(null));
    }

    private static async Task RuntimeLoop(CancellationToken token)
    {
        long revision = 0, notification = 0;
        try
        {
            while (!token.IsCancellationRequested)
            {
                var snapshot = await InvokeAsync<JsonElement>("wait_runtime_snapshot", new { sinceRevision = revision, waitMs = 2500, sinceNotificationId = notification }, token);
                revision = snapshot.Long("revision");
                if (snapshot.TryGetProperty("notifications", out var notices) && notices.ValueKind == JsonValueKind.Array)
                    foreach (var notice in notices.EnumerateArray()) notification = Math.Max(notification, notice.Long("id"));
                await Application.Current.Dispatcher.InvokeAsync(() => { RuntimeSnapshot = snapshot; RuntimeChanged?.Invoke(null, EventArgs.Empty); });
            }
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { }
        catch (Exception error) { if(!token.IsCancellationRequested)await ReportFailure(error); }
    }
    private static async Task NavigationLoop(CancellationToken token)
    {
        long revision = 0;
        try
        {
            while (!token.IsCancellationRequested)
            {
                var events = await InvokeAsync<JsonElement>("wait_wpf_events", new { sinceRevision = revision, waitMs = 2500 }, token);
                revision = events.Long("revision");
                if (events.TryGetProperty("events", out var items) && items.ValueKind == JsonValueKind.Array)
                    foreach (var item in items.EnumerateArray())
                    {
                        var navigation = item.Clone();
                        await Application.Current.Dispatcher.InvokeAsync(() => NavigationChanged?.Invoke(null, navigation));
                    }
            }
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { }
        catch (Exception error) { if(!token.IsCancellationRequested)await ReportFailure(error); }
    }
    private static async Task ReportFailure(Exception error)
    {
        if (!IsConnected) return;
        IsConnected = false; _lifetime?.Cancel();
        await Application.Current.Dispatcher.InvokeAsync(() => ConnectionFailed?.Invoke(null, error.Message));
    }
    internal static async Task StopAsync()
    {
        _lifetime?.Cancel(); _lifetime?.Dispose(); _lifetime = null;
        if (_process != null)
        {
            if (!_process.HasExited)
            {
                try { using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2)); await SendAsync("shutdown_wpf_backend", null, timeout.Token); } catch { }
                try { using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(4)); await _process.WaitForExitAsync(timeout.Token); }
                catch (OperationCanceledException) { if (!_process.HasExited) _process.Kill(entireProcessTree: true); }
            }
            _process.Dispose(); _process = null;
        }
        IsConnected = false; NetworkEnabled = false; ConnectionModes = []; _secret = null; _pipe = null;
    }
}

public sealed class BackendException(JsonElement detail) : Exception(MessageFor(detail))
{
    public JsonElement Detail { get; } = detail;
    private static string MessageFor(JsonElement value) => value.ValueKind switch
    {
        JsonValueKind.String => value.GetString() ?? "后台操作失败",
        JsonValueKind.Object => ObjectMessage(value),
        _ => "后台操作失败"
    };
    private static string ObjectMessage(JsonElement value)
    {
        var key=value.String("message_key");
        if(key.StartsWith("error.",StringComparison.Ordinal))
        {
            var translated=Localizer.Text(key);
            if(translated!=key)return translated;
        }
        return value.String("message",Localizer.Choose("无法完成这项操作，请重试。","This operation could not be completed. Try again."));
    }
}

internal static class JsonValues
{
    internal static string String(this JsonElement value, string key, string fallback = "") => value.ValueKind == JsonValueKind.Object && value.TryGetProperty(key, out var item) && item.ValueKind == JsonValueKind.String ? item.GetString() ?? fallback : fallback;
    internal static long Long(this JsonElement value, string key, long fallback = 0) => value.ValueKind == JsonValueKind.Object && value.TryGetProperty(key, out var item) && item.ValueKind == JsonValueKind.Number && item.TryGetInt64(out var number) ? number : fallback;
    internal static bool Bool(this JsonElement value, string key, bool fallback = false) => value.ValueKind == JsonValueKind.Object && value.TryGetProperty(key, out var item) && item.ValueKind is JsonValueKind.True or JsonValueKind.False ? item.GetBoolean() : fallback;
    internal static JsonElement Object(this JsonElement value, string key) => value.ValueKind == JsonValueKind.Object && value.TryGetProperty(key, out var item) ? item : default;
    internal static IEnumerable<JsonElement> Items(this JsonElement value) => value.ValueKind == JsonValueKind.Array ? value.EnumerateArray() : [];
}
