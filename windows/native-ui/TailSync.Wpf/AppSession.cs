using TailSync.Native.Core;
namespace TailSync.Wpf;

// Test/showcase state remains isolated; production is explicitly connected at startup.
internal static class AppSession
{
    public static ITailSyncClient Client { get; private set; } = new DemoTailSyncClient();
    public static bool IsDemo { get; private set; } = true;
    internal static void UseProduction(ITailSyncClient client) { Client=client;IsDemo=false; }
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
    public static async Task SetSyncAsync(bool value)
    {
        if(!IsDemo)await NativeBackend.InvokeAsync("set_sync_enabled",new{enabled=value});
        SetSync(value);
    }
    public static void RefreshHistory() => HistoryChanged?.Invoke(null,EventArgs.Empty);
}
