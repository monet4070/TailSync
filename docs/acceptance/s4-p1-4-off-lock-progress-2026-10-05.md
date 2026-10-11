# S4-P1-4 锁外平台进度回调（2026-10-05）

> 条目：`S4-P1-4` — 接收引擎锁内的平台进度回调。
> 本次按 `pass_condition` 的 **B 分支**（“若超标则锁外回调后达标”）直接改造接收热路径，
> 台账状态由 `unfixed` → **`fixed_gated`**。
> 前序测量（故障注入，证明回调可被进度存储锁拖住）见
> `docs/acceptance/s4-p1-4-macos-measurement-2026-10-03.md`。

## 修复机制

锁内只采集**进度快照 payload**，释放 `Arc<Mutex<SyncEngine>>` 后再调用平台回调：

1. `engine_state.rs` 新增 `ProgressUpdate`（File / Batch / Clear）与
   `ProgressDispatch`。`receive_progress_update` / `file_progress_update` /
   `clear_progress_update` 只读取引擎状态并构造快照，**不再触碰平台**。
2. `SyncEngine::progress_dispatch` 在锁内把快照与 `platform` 句柄（`Arc` 克隆）配对；
   `ProgressDispatch::deliver` 在锁外调用
   `platform.set_file_progress` / `set_file_batch_progress` / `clear_file_progress`。
   平台实现内部的 UI revision 广播（`notify_progress_revision` → `bump_runtime_revision`）
   因此也移出引擎锁。
3. 接收热路径全部改为“锁内改状态 + 锁内取快照，锁外 deliver”：
   - `receive_engine.rs`：`begin_file_receive_shared`（含失败 clear）、
     `handle_file_chunk_shared_with_key`（offset 不匹配 / 未完成 / 完成 / finalize 失败）、
     `finish_file_receive` 失败 clear；
   - `discard_received_file` 保留 `&mut self` 兼容形式（调用方不持锁），
     新增 `discard_received_file_shared` 供 `verify_and_commit_received_file` 使用；
   - `batches.rs` 的 `cancel_file_batch` 空批次 clear、`engine_state.rs` 的
     `cancel_receive` / `suspend_receive_epoch` 也改用 `deliver_progress_now`。

## 进度顺序语义

- 每个接收的 chunk 处理全程持有 per-transfer `operation_lock`，快照与 deliver 都在释放该锁之前完成，
  因此同一 transfer 的进度严格单调、每 chunk 一次、不重复。
- 发送端按 batch 顺序逐文件发送（`shared/platform-clipboard-transfer.rs` 的
  `for (file_index, prepared_file) in prepared.files.iter().enumerate()`），
  接收端每连接按帧顺序 await 处理，故 batch 聚合（`completed_files` / `transferred_bytes`）
  也按序产生，不会出现新值后到旧值。
- terminal clear（失败 / discard / cancel）在快照序列里位于进度更新之后，且不丢弃。

## 回归覆盖（真实存在、被 CI 执行）

`shared/rust-core/src/sync/tests.rs`：

- `progress_callbacks_run_outside_the_engine_lock_in_monotonic_order`（gate，`#[tokio::test]`，非 ignored）
  - 用 `ProgressProbePlatform` 在每次回调里 `engine.try_lock()`：
    任一回调在锁内执行即记录 `engine_lock_held = true`。
  - 真实驱动两个 chunk 的单文件接收与两个 chunk 的 batch 接收，再走 discard 的 terminal clear。
  - 断言：回调次数 ≥7；`engine_lock_held == false`；
    单文件 `received == [0, 9, 19]`；batch `transferred == [0, 8, 14]`；最后一个事件是
    `Clear { batch_id: Some(batch_id), device: Some("peer") }`。
  - **变异实测**：把 `handle_file_chunk_shared_with_key` 的未完成分支改回锁内
    `deliver_progress_now`，该测试以
    `platform progress callbacks must run with the engine lock released` 失败；改回后通过。

## 真实运行结果（2026-10-05，本机 macOS）

| 命令 | 结果 |
|---|---|
| `node scripts/run-isolated-tests.mjs -- cargo test --locked --manifest-path shared/rust-core/Cargo.toml` | lib `533 passed; 0 failed; 4 ignored`；`legacy_data_isolation` 1 passed；`overridden_data_dir…` 1 passed |
| `node scripts/run-isolated-tests.mjs -- cargo test --locked --manifest-path macos/src-tauri/Cargo.toml --lib` | `115 passed; 0 failed; 3 ignored` |
| `node scripts/check-remediation-ledger.mjs` | OK：31 entries，baseline `b61a0a04b88f` |
| `node scripts/check-local-contracts.mjs` | OK：schema v1 / wire v5 / 77 Windows + 45 macOS commands |
| `cargo clippy --locked --manifest-path shared/rust-core/Cargo.toml --all-targets -- -D warnings` | 通过 |
| `cargo clippy --locked --manifest-path macos/src-tauri/Cargo.toml --all-targets -- -D warnings` | 通过 |
| `cargo fmt --all -- --check` / `cargo fmt --manifest-path macos/src-tauri/Cargo.toml --all -- --check` | 通过 |

注：在受限沙箱内 `secure` / `iroh_transport` / `crypto` 的 socket/Keychain 用例会以
`Operation not permitted` 失败；上述 rust-core 结果是在沙箱外重跑得到，18 个失败均为沙箱
EPERM，与本次改动无关。

## 未验证 / UNKNOWN

- 未做真实 macOS/Windows 文件传输的引擎锁 wait/hold 与回调 wait/hold ≥200 次采样；
  20ms 预算仍是暂定值，未被真实链路数据确认或修正（`native_acceptance` 保留）。
- 未在原生 Windows 执行（本机只有 macOS；CI 的 `rust-windows` 会在真实 Windows 上跑 gate）。
- 未验证真实双设备 UI 端到端的进度显示与 terminal 语义（只验证了核心 seam 的回调顺序）。
- 未做跨连接并发同 batch 的极端交错压测；顺序保证依赖 per-transfer 操作锁与按帧顺序处理。
