import subprocess
import json

def get_tailscale_peers():
    """
    通过命令行调用 tailscale status 获取当前在线的节点列表。
    返回格式: (my_info, peers_list)
    """
    try:
        # 执行 tailscale status --json 命令获取节点状态
        # 注意：这要求系统已安装并配置好 Tailscale 路径
        result = subprocess.run(
            ["tailscale", "status", "--json"], 
            capture_output=True, 
            text=True, 
            encoding='utf-8',
            check=True
        )
        data = json.loads(result.stdout)
        
        my_info = {
            "name": data.get("Self", {}).get("HostName"),
            "ip": data.get("Self", {}).get("TailscaleIPs")[0] if data.get("Self", {}).get("TailscaleIPs") else ""
        }
        
        peers = []
        peer_data = data.get("Peer", {})
        for _, info in peer_data.items():
            # 只同步在线且处于运行状态的节点
            if info.get("Online") and info.get("TailscaleIPs"):
                peers.append({
                    "name": info.get("HostName"),
                    "ip": info.get("TailscaleIPs")[0]
                })
        
        return my_info, peers
    except Exception as e:
        print(f"❌ 无法获取 Tailscale 状态: {e}")
        return {"name": "Unknown", "ip": ""}, []