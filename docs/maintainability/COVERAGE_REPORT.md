# TailSync Maintainability Refactor — CORE COVERAGE REPORT (T357 / T903 刷新)

> 生成时间：2026-08-15（T357 首报，R016 实现：REPORT ONLY，不加 gate，不改 CI；T903 刷新：T902 后 core +2 测试 = 285，同方法重测）
> 工具链：Apple LLVM 17（xcrun llvm-cov/llvm-profdata）+ rustc `-Cinstrument-coverage`，**零新增依赖**。
> 数据：core `cargo test --lib`（285 测试全绿）单次插桩运行。

---

## 1. 汇总（FACT）

| 指标 | 值（T903 刷新） | T357 基线 |
|---|---|---|
| 行覆盖 | **87.66%**（16860 行，missed 2080） | 87.34%（16607 行，missed 2103） |
| 区域覆盖 | 86.02%（26490 区域，missed 3704） | 85.64%（26141 区域） |
| 函数覆盖 | 80.84%（1602 函数，missed 307） | 80.34%（1577 函数，missed 310） |
| 测试 | 285 passed / 1 ignored（覆盖运行与常规运行一致） | 279 passed / 1 ignored |

## 2. 分文件行覆盖（FACT，按覆盖降序）

| 覆盖 | 文件 |
|---|---|
| 100% | peer/admission.rs、peer/inbound_source.rs、sync/shadow.rs |
| 95-99% | peer/types.rs 99.56、diagnostics.rs 98.74、db/schema.rs 98.04、db/queries.rs 97.21、db/file_encryption.rs 96.54、peer/connection_limiter.rs 96.34、db/legacy_v1.rs 95.64 |
| 90-94% | protocol.rs 94.89、secure.rs 93.85、sync_warning.rs 93.33、sync/prepare.rs 91.87、history_classifier.rs 91.74、db.rs 90.49、pairing.rs 90.43、peer/rate_limit.rs 90.00 |
| 85-89% | import.rs 89.18、peer/health.rs 88.79、sync/resume.rs 88.21、db/lifecycle.rs 86.96、peer/directory.rs 86.28、sync.rs 86.20、peer/delivery.rs 85.67、crypto.rs 84.55 |
| 80-84% | peer/event_receiver.rs 83.48、identity.rs 83.22、db/storage.rs 80.00 |
| 70-79% | db/paths.rs 78.69、db/file_storage.rs 73.18、db/migrations.rs 72.76、iroh_transport.rs 70.72 |

## 3. 观察（INFERENCE，非门禁）

- 三项指标较 T357 全部略升（行 87.34→87.66、区域 85.64→86.02、函数 80.34→80.84），与 core 279→285（T402 +4 / T902 +2）及 refactor 测试增益一致。
- R014 高风险路径覆盖良好：sync.rs 86.20（接收状态机，不变）、protocol.rs 94.89（帧解码，T441 proptest 加持）、pairing.rs 90.43（较 T357 87.20 上升）、resume.rs 88.21。
- diagnostics.rs 98.74（T902 新增重入/锁外调用 2 测试；T402 挂载试点以来持续覆盖）。
- 低覆盖区与已知保持决策一致：iroh_transport.rs 70.72（K005 ignored 测试 + iroh 依赖面）、db/migrations.rs 72.76（历史迁移分支）、db/file_storage.rs 73.18（平台路径差异）。
- 无新增 gate：本报告仅为可观察性交付；CI 行为未变（R015）。

## 4. 复现命令（REPORT ONLY）

```bash
cd shared/rust-core
CARGO_INCREMENTAL=0 RUSTFLAGS="-Cinstrument-coverage" LLVM_PROFILE_FILE="target/cover-%p-%m.profraw" cargo test --lib
xcrun llvm-profdata merge -sparse target/cover-*.profraw -o target/cover.profdata
xcrun llvm-cov report --instr-profile target/cover.profdata \
  --object target/debug/deps/tailsync_core-<hash> \
  --ignore-filename-regex='(\.cargo/registry|rustc/)'
```
