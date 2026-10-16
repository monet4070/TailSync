#!/bin/bash
# TailSync macOS test-evidence collector.
#
# Throwaway helper for manual acceptance testing: dumps process state, the
# relevant settings, and the delivery/shutdown log signals in one screen so a
# restart before/after can be compared without hand-grepping a large log.
#
# Usage:  bash macos/scripts/collect-evidence.sh
set -uo pipefail

DATA_DIR="$HOME/Library/Application Support/com.tailsync.TailSync"
LOG="$HOME/Library/Logs/TailSync/tailsyncd.log"
SOCK="$HOME/Library/Application Support/TailSync/tailsyncd.sock"

count() { grep -c -- "$1" "$LOG" 2>/dev/null || true; }

echo "=== TailSync 证据采集  $(date '+%Y-%m-%d %H:%M:%S') ==="
echo

echo "[进程]"
ps -o pid,ppid,lstart,comm -ax 2>/dev/null \
  | grep -E "MacOS/TailSync$|MacOS/tailsyncd$" | grep -v grep \
  || echo "  未检测到 TailSync / tailsyncd 进程"
[[ -S "$SOCK" ]] && echo "  socket: 存在" || echo "  socket: 不存在"
echo

echo "[配置]"
if [[ -f "$DATA_DIR/config-v2.json" ]]; then
  python3 - "$DATA_DIR/config-v2.json" <<'PY'
import json, sys
c = json.load(open(sys.argv[1]))
q = c.get("storage_quota_bytes", 0)
print(f"  connection_mode       = {c.get('connection_mode')}")
print(f"  storage_quota_bytes   = {q} ({q / 1024 / 1024 / 1024:.2f} GiB)")
print(f"  notifications_enabled = {c.get('notifications_enabled')}")
print(f"  sync_enabled          = {c.get('sync_enabled')}")
keys = c.get("trusted_peer_keys", {})
print(f"  已配对设备 ({len(keys)}): {', '.join(keys) if keys else '无'}")
for host, routes in c.get("trusted_peer_addresses", {}).items():
    parts = ", ".join(f"{k}={v}" for k, v in routes.items())
    print(f"    {host}: {parts}")
PY
else
  echo "  未找到 $DATA_DIR/config-v2.json"
fi
echo

echo "[历史页提示 / 投递重试 日志]  $LOG"
echo "  (日志每次守护进程启动会重置，只覆盖本次运行；不输出 DEBUG)"
if [[ -f "$LOG" ]]; then
  first="$(head -1 "$LOG" 2>/dev/null | python3 -c 'import sys,json
try: print(json.loads(sys.stdin.read())["timestamp"])
except Exception: print("?")')"
  last="$(tail -1 "$LOG" 2>/dev/null | python3 -c 'import sys,json
try: print(json.loads(sys.stdin.read())["timestamp"])
except Exception: print("?")')"
  old="$(count 'ended before the in-flight frame completed')"; old=${old:-0}
  shut="$(count 'abandoned while TailSync was shutting down')"; shut=${shut:-0}
  pool="$(count 'Pool connect to')"; pool=${pool:-0}
  batch="$(count 'File batch delivery to')"; batch=${batch:-0}
  failed="$(count 'File transfer failed')"; failed=${failed:-0}
  echo "  本运行区间: $first  →  $last"
  echo "  旧误报串 'ended before the in-flight frame completed' : $old   (期望 0)"
  echo "  关停丢弃 'abandoned while TailSync was shutting down' : $shut   (期望 0，历史页会显示它)"
  echo "  --- 触发条件代理（这些活动过去会顺带产生误报）---"
  echo "  连接失败重试 'Pool connect to ... failed'            : $pool"
  echo "  文件批次投递失败 'File batch delivery to ... failed' : $batch"
  echo "  文件传输失败 'File transfer failed'                  : $failed"
  if [[ "$old" == "0" && "$shut" == "0" && $((pool + batch)) -gt 0 ]]; then
    echo "  判定: ✅ 有重试活动但没有任何误报 —— 修复生效"
  elif [[ "$old" == "0" && "$shut" == "0" ]]; then
    echo "  判定: ⚠️ 未见误报，但本运行也没有触发活动；需要制造一次重试再看"
  else
    echo "  判定: ❌ 仍出现误报（counts above），请连同最近日志一起反馈"
  fi
  echo
  echo "  最近 12 条 投递/批次/连接 事件:"
  grep -E "Delivery to|was abandoned|File batch|Resumed file batch|Pool connect|delivery session ready" "$LOG" 2>/dev/null \
    | tail -12 | python3 -c '
import sys, json
for line in sys.stdin:
    try:
        d = json.loads(line)
        print("   ", d["timestamp"][11:19], d["level"], (d["fields"].get("message") or "")[:110])
    except Exception:
        print("   ", line.rstrip()[:120])
'
else
  echo "  日志不存在"
fi
echo
echo "判定要点:"
echo "  * 误报已彻底移除：不应再出现 'ended before the in-flight frame completed'。"
echo "  * 只有在真实退出且当时有在途剪贴板帧时，才可能记录一条关停警告。"
echo "  * 对端离线引起的重连、文件批次重试，都不应产生历史页提示。"

