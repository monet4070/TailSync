using TailSync.Native.Core;
namespace TailSync.Wpf;

// Demo state belongs to this process only. No production settings or clipboard access.
internal static class AppSession
{
    public static DemoTailSyncClient Client { get; } = new();
    private static readonly Dictionary<string,bool> Flags = [];
    public static int HistoryLimit { get; set; } = 100;
    public static int QuotaGiB { get; set; } = 10;
    public static string ConnectionMode { get; set; } = "auto";
    public static Dictionary<string,string[]> Shortcuts { get; } = [];
    public static bool Sync { get; private set; } = true;
    public static event EventHandler? SyncChanged;
    public static event EventHandler? HistoryChanged;
    public static bool Flag(string name,bool fallback) => Flags.GetValueOrDefault(name,fallback);
    public static void SetFlag(string name,bool value) => Flags[name]=value;
    public static void SetSync(bool value) { if(Sync==value)return;Sync=value;SyncChanged?.Invoke(null,EventArgs.Empty); }
    public static void RefreshHistory() => HistoryChanged?.Invoke(null,EventArgs.Empty);
}
