# DeepSeek 只读审查报告：2026-10-06 composite clipboard 全部改动

## 审查范围与方法
- 工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，分支 `codex/composite-clipboard`，HEAD `f719d581`（未变，工作树脏）。
- 增量边界：`artifacts/clipboard-improvement/2026-10-06-review-cleanup/incremental.patch`（cleanup）与 `2026-10-06-o10/incremental.patch`（O10）。我已逐字节比对，两份 patch 的 `after` 侧与当前生产源码一致；O01/O02/O06/S0/O11 只有指纹快照、无 patch，故直接阅读当前生产源码。
- 未修改/暂存/提交/reset/clean，未访问系统剪贴板，未启动 App，未实施 O03。
- 已核对真实生产调用链：`macos|windows/src-tauri/src/clipboard.rs`、`shared/platform-clipboard-composite.rs`、`shared/rust-core/src/{composite/*,db/composite.rs,db/reads.rs,peer/composite_receiver.rs,sync*.rs}`、`shared/tailsync-runtime/src/history.rs`、两端 UI 与 i18n。

## 结论摘要

| 维度 | 结论 |
|---|---|
| 正确性 | 未发现确定的崩溃/损坏级缺陷；O11 身份、O06 计划、O10 因果核心逻辑自洽 |
| 并发因果 | 主体正确；**发现 1 处“读后-保存前”丢弃导致用户复制丢失**（见 P1-1） |
| 原始数据完整性 | 归档/原件/派生分离扎实；上述丢弃路径是唯一与“原始数据完整性”冲突处 |
| 无用代码 | cleanup 清理干净，但 `ClipboardPort::{publish_composite,publish_native}` 与 `SyncEngine::publish_composite` 仍无调用（P2-2） |
| 测试冗余 | 未发现新增虚假覆盖；有 1 处可再参数化 |
| 证据/验收声明 | 与我复算的 `validation-results.json` 一致，无夸大；硬件/真实应用仍 UNKNOWN |

## 确定缺陷

**P1-1 读后即被新复制超越的稳定采集被直接丢弃，未进入历史**
- 位置：`macos/src-tauri/src/clipboard.rs:292-298`、`windows/src-tauri/src/clipboard.rs:278-284`（`is_current` 丢弃分支）。
- 触发条件：用户复制 C（changeCount=C）→ 监控开始采集并完整读出 C（`matches_snapshot` 通过，说明 `before==after==C`，内容有效）→ 在 `spawn_blocking` 返回后、写库之前，用户又复制 D（changeCount=D）。此时 `snapshot_retry.is_current(C, revision())` 为 false → `snapshot_retry.clear(); continue;`。C 既不写历史也不广播。
- 实际影响：C 作为一次合法用户复制被永久丢失（既不进历史，也无法手动恢复/同步）。这与 ADR-005 第 272 行“历史保存后可以保留已完成的旧事件，但自动发送前仍需当前 revision/generation”的意图相反——ADR 明确说“历史保存后”仍可保留旧事件，而这里在保存前就丢弃。也与同函数保存后的处理不一致：保存后 revision 变化时只跳过广播（`clipboard.rs:357-365`），并不会删除已保存事件。
- 最小修复建议：把 `is_current` 的语义拆开——`revision()!=observed` 时不要 `continue` 丢弃，而是照常走保存分支，仅在保存后跳过 broadcast 与不 acknowledge 新 revision（即复用保存后那道 gate）；只有 `CLIPBOARD_RECOVERY_GENERATION != recovery_generation`（wake/reset）才应丢弃，因为检测器已重建。两端保持一致。
- 回归建议：在采集层注入“稳定快照 + 返回后 revision 前进”的确定性场景，断言该事件**写入历史**且**不广播**、且不消耗新 revision 的重试预算；同时断言 wake generation 变化时确实丢弃。

## 待验证怀疑（需确认设计意图后再定级）

- **P2-1（低置信）** 若“最新复制优先”是刻意的数据策略，则 P1-1 应降级为“文档/实现不一致”，但仍建议至少在 ADR-005 明确“保存前被超越的稳定采集按设计丢弃”，否则与第 272 行自相矛盾。
- **P2-2（中置信）** `record_capture_not_saved` 文案改为“请从来源应用重新复制”（`windows/src/i18n/en.json:63`、`zh-CN.json:63`、`Loc.swift:629`），但该 kind 也被**接收侧**存储失败复用（`shared/rust-core/src/peer/event_receiver.rs:200,256`）。此时用户并没有“复制”，提示方向错误。建议接收侧改用独立 kind，或文案去掉“从来源应用复制”。
- **P3-1** `capture_incomplete` 持久故障（`clipboard.rs:374`）与 `record_saved_capture` 的 `read_incomplete`（`sync_warning.rs:120-129`）都会落到同一 `capture_failed` 提示；因 `record()` 按 `(kind,peer)` 去重（`sync_warning.rs:200-207`）当前无重复气泡，但 `capture_incomplete` 这一事实对 UI 不可见。非缺陷，建议评估是否需要区分。

## 无用代码与测试冗余

- **P2-3 确证死代码（cleanup 遗漏）**：`ClipboardPort::publish_composite`（`shared/rust-core/src/sync/port.rs:88,180`）与 `publish_native`（`:93,194`）在全仓（排除 `target/`）**零调用**；`SyncEngine::publish_composite`（`shared/rust-core/src/sync/clipboard.rs:223`）同样零调用。全仓实际只用 `publish_restore_plan_admitted` / `write_text_admitted` / `write_image_admitted`。cleanup 删掉了方法梯但保留了这几个“失效抽象层”，与本轮“清理失效抽象层”的目标不一致。最小修复：删除这三个方法，或在 trait 上以 `#[doc(hidden)]`+注释标注“为 O03/O04 预留、当前无消费者”，避免被误读为已生效能力。
- 其余 cleanup 删除项（`publish_*_admitted`、`publish_composite_cancellable/admitted/shared_admitted`、`set_history_ref`、`is_proven_no_effect`、`unresolved_intent_age`、`HelperPath`、`input_digest`、`payload_digest`、`RestoreInput.identity/source_archive_version`）经全仓引用搜索确认**无残留调用**，清理正确。
- **P2-4 格式准入事实源三处并行**：`shared/rust-core/src/db/composite.rs:653-671` 的 native-only 白名单、`shared/platform-clipboard-composite.rs:233-267` 的 `supports_native`（Windows 分支）、`shared/tailsync-protocol/src/native_policy.rs:19-32` 的 `macos_publishable` 各维护一份格式策略。当前三者对 macOS 不透明格式**不一致**：`supports_native`/`macos_publishable` 放行不透明格式，而 `add_native_snapshot` 的白名单会拒绝。现状 macOS 走 `add_native_archive`（`archive.is_some()` 跳过该校验）故未暴露，属潜在不一致与维护风险。建议把该白名单抽成单一策略函数并加注释说明“仅用于无 archive 的 legacy/CNS1 路径”。
- 测试：未发现新增 ignore/skip；`shared/rust-core/src/sync/port.rs` 的取消测试已按上轮建议合并为 `o06_cancelled_plan_keeps_owned_output_fifo_and_memory_until_worker_stops`。可再参数化项不构成阻塞。

## 证据与验收声明核对（已复算）

- 指纹链自洽：O06 final `303fc44c…`(770) = cleanup `source_before`；cleanup final `145c47d9c…`(770) = O10 `source_before`；O10 final `e9d431aff…`(770)。各 `validation-results.json` 的 `source_sha256` 与对应检查一致。
- 数字一致：O06 “26 命令 25 PASS/1 FAIL” ↔ `production-features` `exit_code:1, status:FAIL, known_existing_failure:true`；O10 “18 命令全部 exit 0” ↔ `final_checks` 全 `exit:0`（含 657/45/118/127/49/13）；cleanup `features-final` 等全 `exit:0`。
- 声明属实：`shared/rust-core/Cargo.toml:65` 与 `shared/tailsync-runtime/Cargo.toml:13` 已改为 `repair-regressions = []`，CI 显式加 `test-support`，因此 `check-production-features.mjs` 由 O06 的 FAIL 转为 PASS——与 O10 第 13.5 节一致，不是放宽门禁。
- 新增提示双端可达：`capture_origin_unknown`/`capture_not_shared` 在 `sync_warning.rs`、Swift `HistoryView.swift:743-744`+`Loc.swift:625-626,971-972`、React `History.tsx:223-224`+`en/zh-CN.json:351-352` 均已分派。
- 我未把任何证据快照当作代码；上述结论均来自当前生产源码阅读。

## 缺失硬件/设备证据（UNKNOWN，按文档边界保留）
- 未覆盖：真实 App GUI、真实 QQ/目标应用粘贴、Windows 原生运行（仅 macOS 宿主交叉编译）、Mac↔Windows/双设备、hosted CI、物理断电/满盘（O10 用 SQLite `query_only` 合成拒绝代替 ENOSPC）、O04 公开资源下载与文件 materializer lease/GC。
- 因此 O10/O06/cleanup 的 `LOCAL_COMPLETE` 与 Release **NO-GO** 判定我予以认可，不据本地 PASS 外推。

## 建议的下一步（按价值排序）
1. 确认 P1-1 的取舍：若接受“最新优先”，改 ADR-005 措辞；否则改为“保存但不广播”，并加回归。
2. 删除或标注 P2-3 的三个零调用方法。
3. 收敛 P2-4 的格式白名单为单一事实源。
4. 修 P2-2 接收侧提示文案/kind。
5. 按计划推进 O03（本轮未启动），并保持 Windows 原生与双设备验收为独立证据门。
