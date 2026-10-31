using System.Windows;
using TailSync.Native.Core;

namespace TailSync.Wpf;

// Restore writes have process-wide ownership. A second action is rejected,
// never queued behind a write with a stale history/preview target.
public static class RestoreActivity
{
    private static int _busy;
    public static bool IsBusy => Volatile.Read(ref _busy) != 0;
    public static event EventHandler? Changed;
    public static async Task<ActionResult> RunAsync(Func<Task<ActionResult>> operation)
    {
        if (Interlocked.CompareExchange(ref _busy, 1, 0) != 0)
            return new(false, Localizer.Choose("正在恢复剪贴板，请稍候", "A clipboard restore is already in progress"));
        Notify();
        try { return await operation(); }
        finally { Volatile.Write(ref _busy, 0); Notify(); }
    }
    private static void Notify()
    {
        if (Application.Current?.Dispatcher is {} dispatcher && !dispatcher.CheckAccess()) dispatcher.BeginInvoke(() => Changed?.Invoke(null, EventArgs.Empty));
        else Changed?.Invoke(null, EventArgs.Empty);
    }
}
