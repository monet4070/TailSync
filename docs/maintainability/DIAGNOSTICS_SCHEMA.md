# TailSync Maintainability Refactor — DIAGNOSTICS SCHEMA DESIGN (T401)

> 生成时间：2026-08-15（T401 设计，T400 清单之后；R018 要求先设计后实现）
> 依据：LOGS_INVENTORY.md（T400 实测）+ ERROR_BOUNDARY.md（T356）+ R012 类型化枚举。
> 本任务只设计，**不实现**（T402 实现前须经本 schema 评审）。

---

## 1. 目标

在零结构化字段的现状（core 67 处格式化日志）之上，为**传输/同步关键路径**引入结构化 diagnostics，不改变任何现有日志文本（行为零变化），不增加 CI 门禁。

## 2. Schema（v1 草案）

每条 diagnostics 记录（JSON-lines 导出或结构体）：

```jsonc
{
  "ts_ms": 1755270000000,        // unix 毫秒（现有日志无时间戳 → 导出层补）
  "level": "warn",               // error|warn|info|debug
  "event": "file_receive_failed",// 事件名（枚举，见 §3）
  "peer": "hostname",            // 对端标识（可选）
  "session": {                   // 可选：会话上下文
    "kind": "file_batch|pairing|import|sync",
    "id": "transfer_id_hex|batch_id|import_id"
  },
  "error": {                     // 可选：R012 类型化错误的结构化视图
    "kind": "PrepareError::HashMismatch",
    "message": "import data hash mismatch"   // Display 逐字（K004 不变）
  },
  "message": "原格式化文本（保留，便于对照）"
}
```

## 3. 事件面（v1 范围，按 R014 高风险路径裁剪）

| event | 来源 | 关键字段 |
|---|---|---|
| file_receive_begin / chunk / failed / committed | sync.rs begin_file_receive/handle_file_chunk/verify_and_commit | transfer_id, peer, offset, size |
| file_batch_start / finish / cancel | sync.rs begin_file_batch/finish_file_batch/cancel_file_batch | batch_id, peer, total_bytes |
| pairing_window / session_* | pairing.rs（R012 枚举变体直接映射） | PairingError kind, session_id |
| import_begin / chunk / finalize / commit | import.rs（ImportError 变体直接映射） | import_id, entry_type |
| settings_update_failed | crypto.rs apply_settings_update | SettingsUpdateError kind |

## 4. 禁止清单（§18）

- 剪贴板原文、密钥/DEK、配对验证码、文件内容哈希以外的载荷数据——一律不得进入 diagnostics。
- 文件名可含（无 PII 保证），路径显示与现有日志一致。

## 5. 实现路线（T402 草案）

- 导出形态：core 内新增 `diagnostics` 模块（事件枚举 + 记录结构，`#[derive(Serialize)]`），日志宏位置改调 `diagnostics::record(...)`（内部仍走 log! 保持文本不变）。
- 挂载点：仅 §3 列表内事件；其余 60+ 处日志不动（R022：不全面铺开）。
- 消费端：暂不接收集器（REPORT ONLY 精神——schema 就绪 + 挂载点就绪，导出通道留 T402+）。

## 6. 评审锚点

- schema 评审通过前不写实现（R018 硬约束）。
- 每事件挂载后：core 全绿 + 现有日志文本逐字不变（grep 对照）+ 漂移检查 PASS。

## 7. 实现状态（T402 试点，2026-08-15）

- core 新增 `diagnostics` 模块（`Event` 5 变体 + `Record`/`SessionRef`/`ErrorRef` + `set_collector`/`is_collected`/`record`/`error_ref`）；**无 collector 时零分配零输出**（`is_collected()` 门 + `error_ref` 短路）——生产日志行为逐字不变。
- pairing 域 5 个挂载点（enable→WindowOpened、cancel→WindowClosed、begin_handshake→HandshakeStarted、finish_success→Confirmed（带 peer）、record_failure→Failed（带 error_ref））；pairing.rs 原零 log 调用（实测），无新增日志输出。
- 测试：diagnostics 2 + pairing 生命周期 2（全局 collector 测试锁串行化，仿 T301 storage 锁模式）——**core 283 全绿**（279+4）；clippy 0 warning；macOS 61、Windows host check、漂移检查 PASS。
- 导出通道（T402+）：collector 注册点留给平台/遥测侧；本试点仅挂载 + schema 就绪。
