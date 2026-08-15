# TailSync Maintainability Refactor — ERROR BOUNDARY MAP (T356)

> 生成时间：2026-08-15（T356，R013 输入，T355 后实测）
> 依据：R012 类型化（T351-T354）+ Swift ApiClient 实测（L700-745）。
> 只读交付。本图是 R013（错误边界明确 + Swift 端错误映射）的唯一事实来源。

---

## 1. 分层错误流（FACT）

```
core（类型化枚举，T351-T354）
  └─ Display 字符串（wire 契约，逐字保持）
       ├─ commands.rs / routes.rs（map_err to_string 薄转发，T351-T354）
       │    └─ Windows React UI（原生字符串展示）
       └─ api.rs JSON-lines → response["error"]
            └─ Swift ApiError.serverError(String)（ApiClient.swift:149 等 12 处）
                 └─ pairingErrorDescription 子串匹配（L730-744）→ 本地化键
```

## 2. Swift 侧错误面（FACT，实测）

- `enum ApiError: LocalizedError`：connectionFailed / sendFailed / noResponse / invalidJson / serverError(String) —— 5 case。
- `serverError` 载荷 = Rust 侧 `response["error"]` 原样字符串（12 处 throw 点实测）。
- `pairingErrorDescription`（L730-744）：3 组子串匹配 →
  - `"Pairing window is closed"` → `error.pairingWindowClosed`
  - `"Pairing handshake timed out"` → `error.pairingHandshakeTimedOut`
  - `"Connection reset by peer"` 或 `"early eof"` → `error.pairingConnectionClosed`
  - 其余 → 原样字符串。

## 3. K004 契约字符串 ↔ R012 类型化对照（FACT）

| 子串 | Rust 来源 | R012 后状态 |
|---|---|---|
| "Pairing window is closed" | `PairingError::WindowClosed`（T351） | Display 逐字保持 ✓ |
| "Pairing handshake timed out" | 平台 network/mod.rs（peer 健康监控） | 未类型化（平台层，未动）✓ |
| "Connection reset by peer" / "early eof" | transport/secure 层 io 错误 | 未类型化（未动）✓ |
| "Allow pairing" / "允许配对"（测试断言） | AppBehaviorTests.swift L113-124 | Swift 端逻辑未动 ✓ |

- R012 全程：**没有改变任何 Swift 可见字符串**（T351-T354 逐 Display 比对 + 漂移检查 PASS 实证）。

## 4. R013 判定

- 错误边界现状：core 类型化（R012 收官）→ 平台薄转发保持 String → Swift 子串本地化——**三层边界明确**。
- 已登记的边界决策：
  1. 平台层不引入 `From<TypeError> for String` 隐式转换（T351 起显式 map_err，边界清晰）。
  2. Swift 端子串匹配保留（K004）—— 迁移到结构化错误码需要 Swift + Rust 双端同步改，属未来独立 Task（R013 增强），当前无行为收益。
  3. 新增 Rust 错误串必须逐字评审 Swift 匹配面（本图为评审锚点）。
- 验证：`swift test`（AppBehaviorTests 含 pairingErrorDescription 用例）作为 K004 回归门禁。
