using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Threading;

namespace TailSync.Wpf;

internal static class WindowActivation
{
    internal static void Raise(Window window)
    {
        if (!window.IsEnabled && window.OwnedWindows.Cast<Window>().LastOrDefault(owned => owned.IsVisible && owned.IsEnabled) is { } modal)
        { Raise(modal); return; }
        if (window.WindowState == WindowState.Minimized) window.WindowState = WindowState.Normal;
        if (!window.IsVisible) window.Show();
        void Activate()
        {
            if (!window.IsVisible || !window.IsEnabled) return;
            var handle = new WindowInteropHelper(window).Handle;
            if (handle == 0) return;
            BringWindowToTop(handle); window.Activate(); SetForegroundWindow(handle);
        }
        Activate();
        // Finish after native tray/menu dispatch without toggling Topmost or
        // joining another process's input queue.
        window.Dispatcher.BeginInvoke(DispatcherPriority.Input, Activate);
    }
    internal static bool IsForeground(Window window) => new WindowInteropHelper(window).Handle == GetForegroundWindow();
    internal static bool GrantForeground(uint process) => process != 0 && AllowSetForegroundWindow(process);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)] private static extern bool BringWindowToTop(nint handle);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)] private static extern bool SetForegroundWindow(nint handle);
    [DllImport("user32.dll")] private static extern nint GetForegroundWindow();
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)] private static extern bool AllowSetForegroundWindow(uint process);
}
