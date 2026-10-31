using System.Buffers.Binary;
using System.IO;
using System.IO.Pipes;
using System.Text.Json;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
using Microsoft.Win32;

namespace TailSync.Wpf;

// Activation has a deliberately tiny contract: show history or forward a
// tailsync: invite. It cannot invoke backend commands or receive backend secrets.
internal sealed class WindowsIntegration : IDisposable
{
    private readonly string _pipe;
    private readonly CancellationTokenSource _lifetime = new();
    internal event EventHandler<string?>? Activated;
    internal WindowsIntegration(string user) => _pipe = "TailSync.Wpf.Activate." + System.Diagnostics.Process.GetCurrentProcess().SessionId + "." + user;
    internal void Listen() => _ = ListenAsync(_lifetime.Token);
    private async Task ListenAsync(CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            try
            {
                await using var pipe = new NamedPipeServerStream(_pipe, PipeDirection.In, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
                await pipe.WaitForConnectionAsync(token);
                using var timeout = CancellationTokenSource.CreateLinkedTokenSource(token); timeout.CancelAfter(TimeSpan.FromSeconds(2));
                var length = new byte[4]; await pipe.ReadExactlyAsync(length, timeout.Token);
                var size = BinaryPrimitives.ReadInt32LittleEndian(length);
                if (size is <= 0 or > 16384) continue;
                var body = new byte[size]; await pipe.ReadExactlyAsync(body, timeout.Token);
                using var json = JsonDocument.Parse(body);
                var link = json.RootElement.String("link");
                if (link.Length > 0 && !IsInvite(link)) continue;
                Activated?.Invoke(this, link.Length == 0 ? null : link);
            }
            catch (OperationCanceledException) when (token.IsCancellationRequested) { return; }
            catch (IOException) { if (!token.IsCancellationRequested) await Task.Delay(100, token); }
            catch (OperationCanceledException) { }
            catch (JsonException) { }
        }
    }
    internal async Task ActivateAsync(string? link)
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        await using var pipe = new NamedPipeClientStream(".", _pipe, PipeDirection.Out, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        await pipe.ConnectAsync(timeout.Token);
        if (GetNamedPipeServerProcessId(pipe.SafePipeHandle, out var process)) WindowActivation.GrantForeground(process);
        var bytes = JsonSerializer.SerializeToUtf8Bytes(new { link });
        var size = new byte[4]; BinaryPrimitives.WriteInt32LittleEndian(size, bytes.Length);
        await pipe.WriteAsync(size, timeout.Token); await pipe.WriteAsync(bytes, timeout.Token); await pipe.FlushAsync(timeout.Token);
    }
    internal static bool IsInvite(string value) => value.Length <= 12000 && Uri.TryCreate(value, UriKind.Absolute, out var uri) && uri.Scheme.Equals("tailsync", StringComparison.OrdinalIgnoreCase);
    internal static void RegisterUrlScheme()
    {
        if (NativeBackend.IsIsolated) return;
        using var key = Registry.CurrentUser.CreateSubKey(@"Software\Classes\tailsync");
        key.SetValue("", "URL:TailSync secure pairing"); key.SetValue("URL Protocol", "");
        using var icon = key.CreateSubKey("DefaultIcon"); icon.SetValue("", "\"" + Environment.ProcessPath + "\",0");
        using var command = key.CreateSubKey(@"shell\open\command"); command.SetValue("", "\"" + Environment.ProcessPath + "\" \"%1\"");
    }
    public void Dispose() { _lifetime.Cancel(); _lifetime.Dispose(); }
    [DllImport("kernel32.dll", SetLastError = true)][return:MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeServerProcessId(SafePipeHandle pipe, out uint process);
}
