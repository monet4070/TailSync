# TailSync Maintainability Refactor — LOG SURFACE INVENTORY (T400)

> 生成时间：2026-08-15（T400，R018 输入，T354 后实测）
> 依据：T000 BASELINE §11（core 15 处 / 平台 ~84 处时点）+ 本文件计数为 **T400 实测**（生产代码，测试模块剔除）。
> 只读交付。本图是 R018（T401 诊断 schema 设计 → T402+ 导出实现）的唯一事实来源。

---

## 1. 实测分布（FACT，`(?:log::)?(error|warn|info|debug)!` 计数，cfg(test) 剔除）

**core（67 处）：error 2 / warn 30 / info 26 / debug 9**

| 级别 | 主要发射点 |
|---|---|
| error (2) | sync.rs 1、peer/delivery.rs 1 |
| warn (30) | db/migrations.rs 9、db.rs 6、peer/delivery.rs 4、db/lifecycle.rs 3、crypto.rs 2、db/queries.rs 2、其余 4 |
| info (26) | db/migrations.rs 10、sync.rs 5、peer/event_receiver.rs 3、db/lifecycle.rs 3、crypto.rs 2、db/legacy_v1.rs 2、其余 1 |
| debug (9) | peer/delivery.rs 8、peer/event_receiver.rs 1 |

**平台（两端非字节同一，分别实测）：**
- windows：error 37 / warn 53 / info 50 / debug 33 = **173**（top：clipboard.rs e15/w17/i19/d7、lib.rs e9、server.rs w10/d12、iroh.rs w8）
- macos：error 35 / warn 50 / info 46 / debug 30 = **161**（同构文件计数一致，allowed-drift 文件微差）

## 2. 结构化字段现状（FACT）

- **无结构化字段约定**：全部日志为格式化字符串；标识符（transfer_id/peer hostname/batch_id）内联在消息文本中（如 sync.rs `"Receiving file from {}: {}"`）。
- 无 transfer/peer 标识符统一约定；错误上下文靠 String 拼装（R012 类型化后 Display 串仍为唯一载体）。
- 剪贴板内容从不入日志（§18 禁止清单）——现状抽查无违规；R018 导出实现必须延续。

## 3. T401/T402 设计约束（R018 输入）

1. **先设计后实现**：T401 定义 diagnostics schema（transfer_id/peer/session/phase 字段），T402 再导出；schema 审核通过前不写导出代码。
2. **禁止清单**：剪贴板原文/密钥/配对验证码不得入 diagnostics（§18）。
3. **复用 R012 类型化错误**：错误上下文应从类型化枚举（PairingError/PrepareError/ImportError 等）提取结构化字段，而非 Display 字符串解析。
4. 不加 gate（与 CI 门禁分离，R015 不动）。
