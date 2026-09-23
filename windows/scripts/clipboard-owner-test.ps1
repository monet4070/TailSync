param(
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, 2000)]
    [int]$HoldMilliseconds,

    [Parameter(Mandatory = $true)]
    [string]$ReadyFile
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class TailSyncClipboardTestOwner
{
    [DllImport("user32.dll", EntryPoint = "CreateWindowExW", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateWindowEx(
        int exStyle, string className, string windowName, int style,
        int x, int y, int width, int height, IntPtr parent,
        IntPtr menu, IntPtr instance, IntPtr parameter);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool DestroyWindow(IntPtr window);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool OpenClipboard(IntPtr owner);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool CloseClipboard();

    public static void Hold(string readyFile, int milliseconds)
    {
        IntPtr window = CreateWindowEx(
            0, "STATIC", "TailSync clipboard acceptance owner", 0,
            0, 0, 0, 0, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
        if (window == IntPtr.Zero)
            throw new InvalidOperationException("Test owner could not create hidden window.");
        try
        {
            if (!OpenClipboard(window))
                throw new InvalidOperationException("Test owner could not open the Windows clipboard.");
            try
            {
                System.IO.File.WriteAllText(readyFile, "ready");
                System.Threading.Thread.Sleep(milliseconds);
            }
            finally
            {
                if (!CloseClipboard())
                    throw new InvalidOperationException("Test owner could not close the Windows clipboard.");
            }
        }
        finally
        {
            DestroyWindow(window);
        }
    }
}
'@

[TailSyncClipboardTestOwner]::Hold($ReadyFile, $HoldMilliseconds)
