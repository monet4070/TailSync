using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;

namespace TailSync.Wpf;

// Remember only a live external foreground window. Never activate a handle
// that has been recycled to another process since history was opened.
internal static class ForegroundTracker
{
    private static nint _window;
    private static uint _process;
    internal static void Capture()
    {
        var window=GetForegroundWindow();
        if(window==0)return;
        GetWindowThreadProcessId(window,out var process);
        if(process==Environment.ProcessId)return;
        _window=window;_process=process;
    }
    internal static void Restore()
    {
        var window=_window;var process=_process;_window=0;_process=0;
        if(window==0||!IsWindow(window))return;
        GetWindowThreadProcessId(window,out var current);
        if(current!=process)return;
        if(IsIconic(window))ShowWindow(window,9);
        SetForegroundWindow(window);
    }
    [DllImport("user32.dll")]private static extern nint GetForegroundWindow();
    [DllImport("user32.dll")]private static extern uint GetWindowThreadProcessId(nint window,out uint process);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)]private static extern bool IsWindow(nint window);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)]private static extern bool IsIconic(nint window);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)]private static extern bool ShowWindow(nint window,int command);
    [DllImport("user32.dll")][return:MarshalAs(UnmanagedType.Bool)]private static extern bool SetForegroundWindow(nint window);
}
