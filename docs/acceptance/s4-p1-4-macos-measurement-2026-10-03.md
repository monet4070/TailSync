# S4-P1-4 macOS 侧锁内进度回调测量（2026-10-03）

> 条目：`S4-P1-4` — 接收引擎锁内的平台进度回调（待测量裁定），台账状态 **`unfixed`**。
> 目的：刷新该条目在 **macOS 原生平台实现**上的可复现数字。
> **本文件不关闭该条目。** 原因见末节。
>
> 后续（2026-10-05）：条目已按 `pass_condition` 的 **B 分支（锁外回调）** 关闭为
> `fixed_gated`，见 `docs/acceptance/s4-p1-4-off-lock-progress-2026-10-05.md`。
> 本文件保留为当时的故障注入测量记录。

## 测量对象

被测量的是真实的 macOS 平台实现 `set_file_batch_progress`，它使用共享的
`FILE_PROGRESS` 存储锁。诊断代码位于
`shared/platform-progress-measurement-tests.rs`，由
`macos/src-tauri/src/api/tests.rs:587` 以 `include!` 引入。

## 命令

```bash
cargo test --manifest-path macos/src-tauri/Cargo.toml --lib \
  -- --ignored platform_progress_callback_contention_measurement --nocapture
```

## 结果（2026-10-03）

```json
{"callback_us":{"max":56739,"p50":55051,"p99":55144},
 "injected_lock_hold_ms":50,"measurement":"real_platform_progress_callback","samples":200}
```

| 指标 | 本次（2026-10-03） | 台账上次记录 |
|---|---|---|
| 样本数 | 200 | 200 |
| 注入持锁 | 50 ms | 50 ms |
| p50 | 55 051 µs | — |
| p99 | **55 144 µs** | 60 117 µs |
| max | **56 739 µs** | 61 882 µs |

结论：**平台进度回调确实会被进度存储锁拖住**，阻塞时长与注入的持锁时间同量级（≈55 ms）。

## 为什么这不能关闭 S4-P1-4

台账的 `pass_condition` 是：

> 锁等待满足冻结预算且进度顺序正确；若超标则锁外回调后达标。

本次是**故障注入实验**，不是真实设备负载采样：

1. 持锁的 50 ms 是人工 `sleep`，不代表真实 `set_file_batch_progress` 调用的持锁时长；
2. 没有真实文件传输驱动，因此没有真实的锁 **wait**（只有 hold）样本；
3. 没有同时验证进度顺序与 terminal 语义；
4. 20 ms 预算仍是暂定值，未被真实链路数据确认或推翻。

## 关闭该条目需要什么

| 路径 | 内容 |
|---|---|
| **A. 真实负载测量** | 在真实 macOS（以及 Windows）文件传输中，分别采样引擎锁的 wait/hold 与回调 wait/hold，各 ≥200 次，并同时断言进度顺序与 terminal 语义；据此确认或修正 20 ms 预算。现有 `shared/rust-core/tests/reliable_event_process.rs` 的两进程真实 TCP/Noise/SQLite 骨架可复用。 |
| **B. 直接改造** | 把平台进度回调与 watch 广播移出引擎锁，使锁等待达标。这是 `pass_condition` 里"若超标则锁外回调后达标"的分支，属于接收热路径改动，需要独立的回归覆盖。 |

在 A 或 B 完成前，台账中的 `unfixed` 状态是准确的，不应改写。
