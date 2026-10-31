using System.Text.Json;

namespace TailSync.Wpf;

// Rust exposes its warning queue by peek. Only a warning actually displayed
// by a visible native history window is acknowledged. Shared identity prevents
// the history and favorites windows from consuming/showing each other's copy.
internal sealed class WarningDelivery
{
    private readonly HashSet<long> _shown=[],_acknowledged=[],_inFlight=[];
    private readonly Queue<long> _order=[];
    internal async Task DeliverAsync(JsonElement warning,bool visible,Action<string> display,Func<long,Task<bool>> acknowledge)
    {
        if(!visible||warning.ValueKind!=JsonValueKind.Object||!warning.TryGetProperty("id",out var identity)||!identity.TryGetInt64(out var id)||id<0)return;
        if(_acknowledged.Contains(id)||_inFlight.Contains(id))return;
        if(!_shown.Contains(id))
        {
            display(WarningCoordinator.Message(warning));_shown.Add(id);_order.Enqueue(id);
            while(_order.Count>128){var oldest=_order.Dequeue();_shown.Remove(oldest);_acknowledged.Remove(oldest);}
        }
        _inFlight.Add(id);
        try{if(await acknowledge(id))_acknowledged.Add(id);}
        catch(Exception error)when(error is not OutOfMemoryException){/* The next snapshot retries acknowledgement without another toast. */}
        finally{_inFlight.Remove(id);}
    }
}
internal static class WarningCoordinator
{
    private static readonly WarningDelivery Delivery=new();
    internal static Task DeliverAsync(JsonElement warning,bool visible,Action<string> display,CancellationToken token)
        =>Delivery.DeliverAsync(warning,visible,display,id=>NativeBackend.InvokeAsync<bool>("ack_sync_warning",new{id},token));
    internal static string Message(JsonElement warning)
    {
        var peer=warning.String("peer");
        var key=warning.String("kind")switch{"expired_event"=>"history.syncExpired","delivery_stalled"=>"history.syncStalled","delivery_shutdown"=>"history.syncShutdown","delivery_expired"=>"history.syncDeliveryExpired",_=>""};
        if(key.Length>0)return Localizer.Text(key).Replace("{peer}",peer);
        var chinese=warning.String("kind")switch
        {
            "capture_not_saved"=>"这次复制未能保存到历史，请检查存储状态后从来源应用重新复制。",
            "receive_not_saved"=>"接收的内容未能保存到历史，请检查存储后到发送设备重新复制。",
            "capture_failed"=>"未能完整读取这次复制的内容，请从来源应用重试。",
            "capture_origin_unknown"=>"这次复制的来源无法确认，部分格式可能不可用。",
            "capture_not_shared"=>"这次复制已保存到历史，但尚未准备好同步内容。",
            "clipboard_unavailable"=>"系统剪贴板暂时不可用，请稍后重试。",
            "file_selection_symlink"=>"所选文件包含符号链接，未发送该项目。",
            "file_selection_folder"=>"所选内容包含文件夹，当前仅支持直接选择文件。",
            "delivery_rejected"=>$"设备 {peer} 未接受这次同步，请检查兼容性后重试。",
            "composite_incomplete" or "composite_partial"=>$"与 {peer} 的图文未完整应用；完整内容保留在历史中。",
            "composite_apply_unknown"=>$"无法确认 {peer} 是否已应用图文内容，可到接收设备检查。",
            "composite_superseded"=>$"已保留 {peer} 上较新的剪贴板内容，这次图文没有覆盖它。",
            _=>"剪贴板同步未能完整完成，请检查连接及历史记录后重试。"
        };
        var english=warning.String("kind")switch
        {
            "capture_not_saved"=>"This copy could not be saved to history. Check storage, then copy it again from the source app.",
            "receive_not_saved"=>"Received content could not be saved. Check storage and copy it again on the sending device.",
            "capture_failed"=>"This copy could not be read completely. Try copying again in the source app.",
            "capture_origin_unknown"=>"The source of this copy could not be verified; some formats may be unavailable.",
            "capture_not_shared"=>"This copy is saved in history, but its sync content could not be prepared.",
            "clipboard_unavailable"=>"The system clipboard is temporarily unavailable. Try again shortly.",
            "file_selection_symlink"=>"The selection contains a symbolic link, which was not sent.",
            "file_selection_folder"=>"The selection contains a folder. Select individual files to send them.",
            "delivery_rejected"=>$"{peer} did not accept this sync. Check compatibility and try again.",
            "composite_incomplete" or "composite_partial"=>$"Rich content was not fully applied on {peer}; the full content remains in history.",
            "composite_apply_unknown"=>$"It is unclear whether {peer} applied the rich content. Check the receiving device.",
            "composite_superseded"=>$"Newer clipboard content on {peer} was preserved instead of being overwritten.",
            _=>"Clipboard sync did not complete. Check the connection and history, then try again."
        };
        return Localizer.Choose(chinese,english);
    }
}
