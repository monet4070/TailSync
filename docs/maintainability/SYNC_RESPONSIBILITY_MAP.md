# TailSync Maintainability Refactor — SYNC RESPONSIBILITY MAP (T320)

> 生成时间：2026-08-15（T320，Phase 4 R010 入口，T304 后实测）
> 依据：T000 BASELINE §6.1（2344 行时点）+ T101-T111/T107 迁移后复核。本文件行号为 **T320 实测**（sync.rs 2692 行）。
> 只读交付。本图是 R010（T321+ 渐进式拆分）的唯一事实来源。

---

## 1. 文件结构与公共面（FACT，T320 实测）

- sync.rs **2692 行**（T000 2344 → 净 +348：T107 verify_and_commit_received_file/ReceiveSuspendGuard/MAX_FILE_SIZE/import_size_limit 等迁入）；**32 个测试**（T000 24 → +8）全部在单一 `mod tests`（L1612-2692，1080 行 = 40%）。
- cfg：生产代码 **0 个**；仅测试内 3 处（L1875/1879 cfg(windows)/cfg(unix) 符号链接分支 + L1611 cfg(test)）。R003 保持。
- 字符串错误面：**24 个 `Result<…, String>` 签名**（T000 22 → +2）。
- 依赖：`use crate::protocol::{FileChunkPayload, MessageId, TransferId, FILE_CHUNK_SIZE}` + `db::get_incoming_dir()`；**不引用 peer/ 或 delivery**（T000 结论复核仍成立）。

## 2. 常量与静态（FACT）

| 常量 | 值 | 用途 |
|---|---|---|
| SEEN_MESSAGE_RETENTION_SECONDS | 600s | 去重窗口 |
| INCOMPLETE_TRANSFER_RETENTION_SECONDS | 24h | 续传保留期 |
| CANCELLED_BATCH_RETENTION_SECONDS / MAX_ENTRIES | 24h / 1024 | 取消批次剪枝 |
| MAX_FILE_BATCH_COUNT / BYTES | 20 / 1GiB | 批配额 |
| MAX_ACTIVE_BATCHES_PER_PEER / GLOBAL | 2 / 8 | 准入 |
| MAX_ACTIVE_RECEIVES_PER_PEER / GLOBAL | 2 / 8 | 准入 |
| SHADOW_FILTER_TTL / MAX_ENTRIES | 30s / 1024 | 回显抑制 |
| MAX_FILE_SIZE | 1GiB | 单文件上限（T107） |
| FILE_BATCH_ADMISSION_LOCK | LazyLock<tokio Mutex> | 跨连接批准入串行化 |

## 3. 类型（FACT）

- **SyncEngine**（L~460-475）：10 字段 = 6 个 keyed HashMap（seen_messages/active_receives/completed_transfers/incoming_batches/cancelled_batches/completed_batches）+ clipboard_generation + 2×ShadowFilter + platform: Option<Arc<dyn SyncPlatform>>。
- **SyncPlatform trait**（L440-455）：7 方法全 `Result<(), String>`（write_text/write_image/set_file_progress/clear_file_progress/set_file_batch_progress/files_received/file_batch_failed）；两端 sync_adapter.rs byte-identical 实现（R004 遗留候选，T000 §6.1）。
- 载荷/状态结构：FileMeta/FileBatchRef/FileBatchEntry/FileBatchManifest/PreparedFile/PreparedFileBatch（批准备）；ReceivedFile/FileReceiveProgress/PendingReceivedFile/FileReceiveState/CompletedTransfer/PersistedTransfer/PersistedIncomingBatch（接收/续传）；FileBatchProgress（进度）。
- **ShadowFilter**（L35-91）：私有，HashMap<String, Instant> + insert/contains/remove/prune（TTL 30s / 1024 上限）——完全自包含。

## 4. 函数清单（FACT，按领域）

| 领域 | 函数（行号实测） | 自包含度 |
|---|---|---|
| 批准备（纯逻辑） | normalize_transferred_file_name L97、validate_incoming_file_meta L129、prepare_file_batch L246、revalidate_prepared_file L334、modified_nanos L357、hash_source_file L366、short_parent_label L383、collision_safe_name L387 | 高（纯函数 + 文件哈希；FileMeta::validate L207） |
| 文本/图片 | handle_incoming_text L594、restore_text L605、handle_incoming_image L621、restore_image L638、supersede_file_clipboard L655、add/remove/contains_shadow_filter L1346-1373 | 中（经 platform hook） |
| 文件批次 | begin_file_batch L660、has_file_batch L744、pending_file_batch_bytes L752、batch_for_transfer L775、notify_file_batch_failed L781、finish_file_batch L787、cancel_file_batch L828、cancel_file_batch_local L877 | 中（依赖 SyncEngine 字段） |
| 接收状态机（高风险） | begin_file_receive L890（{id}.part + {id}.resume.json）、handle_resumable_file_chunk L1062（严格 offset 门）、handle_file_chunk L1132、finish_file_receive L1148、commit_received_file L1199、discard_received_file L1266、cancel_receive L1271、suspend_receive L1292（ReceiveSuspendGuard L1466）、prune_cancelled_batches L1309、verify_and_commit_received_file L1431 | 低（核心状态机，5 专项续传测试） |
| 续传持久化（纯文件 I/O） | persist_transfer_state L1490、persist_incoming_batch L1509、restore_persisted_received_file L1519、cleanup_expired_transfers L1549、cleanup_expired_transfers_in L1557 | 高（显式 Path/入参，无 SyncEngine 依赖） |
| 去重 | has_seen_message L1331、record_message L1339 | 低（2 行，内联即可） |

## 5. 拆分建议（T321+ 输入，BEHAVIOR_CHANGE_ALLOWED=FALSE）

按隔离度 + 风险 + 测试价值排序（§20 Maintainability Decision Test）：

1. **续传持久化簇 → sync/resume.rs**（persist_transfer_state/persist_incoming_batch/restore_persisted_received_file/cleanup_expired_transfers(_in) + PersistedTransfer/PersistedIncomingBatch）：全部显式入参、无 SyncEngine 依赖；R014 高风险区（续传）直接受益；`cleanup_expired_transfers_in(incoming, retention)` 已路径参数化可单测。**→ T321 已完成（见 §0）**
2. **ShadowFilter → sync/shadow.rs**（L35-91 + 2 常量）：完全自包含，最小试点。
3. **批准备纯逻辑 → sync/prepare.rs**（normalize/validate/prepare/revalidate/collision/hash 簇 + PreparedFile/PreparedFileBatch/FileMeta）：纯函数，已有 T107 测试基础。
4. **SyncEngine 方法族（text/image/batch/receive）→ 保持**（共享字段状态机，拆分成本 > 收益；待 1-3 完成后重新评估）。
5. **sync_adapter.rs byte-identical → R004 遗留**（两端同文，抽 core 需先统一 SyncPlatform 写侧，与 T304 restore_entry 结论同源：不抽）。

每步验收：core 测试先行（R014）+ `cargo test --locked` 全绿 + 漂移检查 PASS（sync_adapter 不动则不涉）+ 行为零变化（纯移动）。

---

## 0. 迁移完成状态（T321-T323，2026-08-15）

| # | 目标 | Task | 结果 |
|---|---|---|---|
| 1 | 续传持久化簇 → sync/resume.rs | T321 | 5 函数 + 2 持久化类型原样迁入（`sync/resume.rs`）；sync.rs 根 `mod resume;` + `pub use resume::cleanup_expired_transfers;`（平台 lib.rs 调用面不变）+ 内部 `use resume::…`；字段/函数 `pub(crate)`（原同模块私有语义等价）；既有 cleanup 测试改指 `super::resume::…` |
| 2 | ShadowFilter → sync/shadow.rs | T322 | ShadowEntry/ShadowFilter + TTL/MAX 常量原样迁入（`sync/shadow.rs`，102 行）；`pub(crate)` 方法 + 字段保持私有（2 个直测私有字段的测试随迁入 shadow.rs）；sync.rs 常量与 Instant 导入清理 |
| 3 | 批准备纯逻辑 → sync/prepare.rs | T323 | 8 函数 + 6 类型 + impl validate 原样迁入（`sync/prepare.rs`，~450 行）；sync.rs 根 `pub use prepare::{…11 项}`（平台 `sync::FileMeta/FileBatchManifest/validate_incoming_file_meta/MAX_FILE_SIZE/PreparedFileBatch/FileBatchRef/normalize_transferred_file_name` 等路径不变）+ 内部 `use prepare::hash_source_file;`；`hash_source_file` pub(crate)（resume.rs 复用） |
| 4 | SyncEngine 方法族 | T324 | **保持（评审结论）**：单一 `impl SyncEngine`（L184+，~40 方法）覆盖 10 个共享字段；接收方法族触 ≥7 字段；impl 块分散 = 纯组织变化（零耦合削减），子结构提取 = 重构状态模型（续传序列化/准入计数全动，行为风险，违反 BEHAVIOR_CHANGE_ALLOWED=FALSE）→ R022 判定保持 |

**T321-T324 实测：**
- core：273 测试全绿（T321 +4 新增；T322 2 测试随模块迁移；T323 +5 新增：前缀剥离/碰撞命名/manifest 结构校验/真实文件批构建与哈希断言/符号链接与文件夹拒绝）；clippy 0 warning；fmt 干净。
- macOS crate 61 测试全绿；Windows host check（fmt + check --all-targets + test --no-run）通过；漂移检查 PASS。
- 行为零变化：纯移动 + 可见性收窄为 pub(crate)/pub re-export；`sync::` 公共路径全部保持（平台引用点实测核对）。
- **R010 收官判定**：渐进式拆分完成 —— 3 簇纯逻辑迁出（prepare/resume/shadow），方法族保持评审（T324）；sync.rs 2692 → **2159 行**（-533）；sync/ 目录 = prepare.rs（480）+ resume.rs（323）+ shadow.rs（102）。

## 6. 约束

- R003：新增模块零 cfg；R011：不重构 DB（get_incoming_dir 等保持）；R022：新增 abstraction 必须过 §20-§22 Decision Tests。
- `pub mod sync` 公共面（SyncEngine/SyncPlatform/常量/cleanup_expired_transfers）保持稳定 —— 平台 sync_adapter 与 commands.rs 依赖它。
