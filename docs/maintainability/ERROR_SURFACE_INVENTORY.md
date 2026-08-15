# TailSync Maintainability Refactor — ERROR SURFACE INVENTORY (T350)

> 生成时间：2026-08-15（T350，R012/R013 输入，T324 后实测）
> 依据：T000 BASELINE §11（66 Result<_,String> + ~142 map_err/to_string 时点）+ 本文件计数为 **T350 实测**。
> 只读交付。本图是 R012（T351+ 逐 domain 类型化）与 R013（Swift 错误边界）的唯一事实来源。

---

## 1. 类型化先例（FACT，可仿照模式）

| 类型 | 位置 | 样式 |
|---|---|---|
| ProtocolError | protocol.rs:338（thiserror） | 协议帧/命令解析 |
| IdentityError | identity.rs:13-34（thiserror：NotFound/AccessDenied/Io/Corrupt/Crypto/Generation） | 身份 I/O + 加密 |
| KeyStoreError | crypto.rs:462（thiserror，pub(crate)） | 密钥库 |
| DeliveryError | peer/delivery.rs（类型化，thiserror 风格 Display） | 帧记账/ACK/连接竞速 |

## 2. 字符串错误面（FACT，`Result<…, String>` 签名计数实测）

**core（103 处）：**

| domain | 文件 | 计数 |
|---|---|---|
| sync 接收/状态机 | sync.rs | 16 |
| sync 批准备 | sync/prepare.rs | 6 |
| sync 续传持久化 | sync/resume.rs | 2 |
| settings/密钥 | crypto.rs | 12 |
| 身份 | identity.rs | 8 |
| 配对 | pairing.rs | 5 |
| 传输 | iroh_transport.rs | 9 |
| 导入 | import.rs | 6 |
| db 路径/存储 | db/paths.rs 6 + db/storage.rs 6 | 12 |
| peer 投递 | peer/delivery.rs | 17 |
| peer 目录 | peer/directory.rs | 4 |
| peer 事件接收 | peer/event_receiver.rs | 4 |
| peer 其余 | inbound_source 1 + rate_limit 1 | 2 |

**平台（两端非字节同一文件，分别计数）：** windows commands.rs 46 + api.rs 9 + api/imports.rs 5 = **60**；macos commands.rs 27 + api.rs 9 = **36**。

**K004 跨层契约（FACT）：** SwiftUI 用 `message.contains(...)` 子串匹配原始 Rust 错误字符串（ApiClient.swift L730-744，匹配 "Pairing window is closed"/"Pairing handshake timed out"/"Connection reset by peer"/"early eof"）—— R012 类型化迁移**不得改变这些 Display 输出**，或需 R013 同步迁移 Swift 端匹配。

## 3. domain 排序（T351+ 输入）

按 R012 traceability（"一次一个 domain，先 sync 或 pairing"）+ P0/P1（行为不变优先）+ 测试充分度：

1. **pairing（5）**：最小面；PairingManager 状态机错误已高度集中；PairingError 仿 IdentityError 模式。**→ T351 已完成（见 §0）**
2. **sync 批准备/续传（prepare 6 + resume 2）**：已随 T321/T323 迁入独立模块，错误字符串即 wire 契约（validate_incoming_file_meta/FileBatchManifest::validate）——类型化需保持 Display 逐字。**→ T352 已完成（见 §0）**
3. **peer/delivery（17）**：17 处多为 `ConnectionAdapter` trait 签名（write_frame/read_frame，传输层字符串契约的刻意设计面）+ 测试基础设施；trait 契约变更 = 大重构（平台实现 + 测试双端），R012 价值/风险比低 —— 评估保持，并入 R013 边界设计时再议。
4. **crypto（12）/identity（8）/import（6）**：半类型化（KeyStoreError/IdentityError 已存在），迁移剩余签名。
5. **iroh_transport（9）/db（12）**：db 层 R011 约束（不重构 DB），仅允许错误类型化不触及架构；iroh 依赖 iroh crate 错误映射。
6. **平台 96 处**：R013 配套（Swift 子串契约 + 命令层薄转发错误透传）。

每 domain 验收：其全部测试通过；测试断言从子串匹配改为错误类型匹配（K004 约束，但 Display 串保持）；漂移检查 PASS（命令层/api 层契约字符串不变）。

---

## 0. 迁移完成状态（T351-T353，2026-08-15）

| domain | 结果 |
|---|---|
| pairing | `PairingError`（thiserror，10 变体）；5 个签名转换；Display 串逐字保持（Swift 子串契约 L730-744 不受影响）；平台 8 处调用点显式 map_err；不引入隐式 From（R013 边界清晰） |
| sync 批准备/续传 | `PrepareError`（23 变体）+ `ResumeError`（2 变体）；8 个签名转换；Display 逐字保持（wire 契约）；平台 6 处调用点适配 |
| identity 密钥解码 | `IdentityKeyError`（InvalidBase64/WrongLength 2 变体）；decode_public_key/canonical_public_key 转换；trust_peer 映射 Key(e.to_string())（Display 逐字）；secure::decode_trusted_key 透传 map_err；directory.rs 调用点零改动（`.ok()`/`unwrap_or_default`） |
| crypto settings | `SettingsValidationError`（7 变体）+ `SettingsUpdateError`（Validation/Persist/Database 3 变体）；validate_user_values/prepare_user_update/apply_settings_update 转换；平台 4 处调用点（commands W/M + routes W/M）显式 to_string |
| import | `ImportError`（24 变体：类型/描述/哈希/会话/偏移/大小/io 全量消息逐字）；6 个签名转换（import_size_limit/begin_import/append_import_chunk/finalize_import/commit_import + finalize 内闭包）；平台 api/imports.rs（W/M byte-identical）5 处适配点 map_err；10 处测试断言升级类型匹配 |

**T351-T354 实测：**
- core：273 测试全绿；断言升级（K004 类型匹配 + Display 双断言：T351 3 处、T352 11 处、T353 2 处、T354 10 处）；clippy 0 warning；fmt 干净。
- macOS crate 61 测试全绿 + clippy 0 warning；Windows host check PASS；漂移检查 PASS（38 Swift 命令面 + wire 字符串不变）。
- **R012 收官判定**：5/6 domain 类型化（pairing、sync prepare/resume、identity、crypto settings、import——core 生产面 `Result<_, String>` 签名从 103 → 25，其中 13 属 iroh_transport/db（R011 约束区/iroh 依赖面）与平台钩子签名）；delivery 保持评估登记（ConnectionAdapter trait 契约面）；iroh/db 域按 R011 约束 + iroh crate 依赖评估为低优先保持。

## 4. 约束

- R012 不改变产品行为：用户可见错误串是 Swift/前端契约，类型化仅改变内部表示。
- R011：db 层仅错误类型化，不重构架构。
- R022：新错误枚举必须通过 §20 Maintainability Decision Test（先例 IdentityError 的枚举粒度即参照）。
