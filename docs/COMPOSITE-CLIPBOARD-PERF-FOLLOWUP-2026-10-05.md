# 单 Mac 性能补齐与主代理复核（2026-10-05）

初始 cache/helper 测量使用旧 `c022bcb8…` 源码的独立插桩副本。随后主代理修复正式源码的 cache 预算与轮询间隔，并在独立 debug 测量副本加入私有 board detector/helper 适配，实际驱动复合采集。测试源码和最终包由单 Mac 总报告中的当前 R6 指纹绑定。原始样本从未改写；此前分析与报告保存在 `2026-10-05-single-mac-supervision/performance-*-before-root-correction.*`。

## 实际缓存占用

真实 `deliver_pending_frame → deliver_composite` 入口发现不可压缩缓存漏计每片 76 B header；3/9/33/51/62 片分别超出旧 lease 228/684/2508/3876/4712 B。容器、Arc 与实际 Vec capacity 也必须纳入预算。可压缩 1 MiB 对照缓存仅 10,270 B，重试复用且最终归零。

正式修复先预留完整编码上限，再按真实 capacity 缩减。正式回归驱动真实投递入口，覆盖不可压缩、可压缩、重试只编码一次、最后 owner 释放与总 budget 归零。修复前、首个测试代码编译错误、磁盘满、修复后日志都保留在 supervision 目录。旧 cache 样本用于证明缺陷，不冒充修复后原生包测量。

## 后台 timer 与真实采集

旧复合分支提前 continue，导致 250 ms idle 不可达。正式修复为无待处理 revision 时 250 ms、新 revision/待重试/wake reset 时 50 ms，helper unavailable 时 250 ms；没有提前确认 revision。旧 `timer-probe-1.jsonl` 的 1334 个全 50 ms 样本保留为修复前证据。

新测量 `tick` 记录本次 sleep；`body` 与同一个 tick ID 连接，表示该次唤醒后的循环体。采集通常发生在 250 ms idle 唤醒后，随后选择下一次 50 ms sleep。以前把上一 tick body 连接到下一 tick，已纠正，`tools/analyze_perf.py` 可独立复算。sleep overshoot 反映迟到；gap 还包含上次 body，不能当纯 timer 抖动。

| 样本 | 本次 sleep | tick 数 | overshoot p50 / p95 / max（µs） | 同 tick body p50 / p95 / max（µs） |
|---|---|---|---|---|
| timer-measure-1.jsonl | 50 ms | 15 | 2166 / 2391 / 2391 | 186 / 277 / 277 |
| timer-measure-1.jsonl | 250 ms | 166 | 2178 / 3099 / 3361 | 449 / 35577 / 63506 |
| timer-measure-2.jsonl | 50 ms | 50 | 2150 / 2404 / 2633 | 178 / 377 / 609 |
| timer-measure-2.jsonl | 250 ms | 192 | 2233 / 2858 / 10738 | 521 / 48203 / 61714 |

measure-1 的 driver 15/15 成功、日志 14 次真实 capture；measure-2 driver 50/50、日志 49 次真实 capture。驱动成功数与捕获数不能混用。两组 sleep/gap 原始样本对应约 43.27 / 52.98 秒有效观测，不以脚本设定窗口或 daemon 最后一条非 tick 日志推导持续时间。

这些是 debug、私有 named-board 适配、有 driver 负载的测量，包含真实采集 body；不等于 release/general-board 的同环境测量，也不覆盖 5 秒异常等待。release 同环境 timer 测量仍 UNKNOWN。

## 发布串行与等待

产品没有专门的 helper 调度队列。真实 PUBLISH mutex 并发 4/8/16 次全部成功，active 与 helper 峰值均 1，最终都归零。atomic 的未取得 permit 尝试峰值为 4/7/15；取得 permit 时读取的 snapshot 最大值为 3/7/14。这两列口径不同。主代理另重复 8 次真实发布：8/8、atomic peak 8、snapshot max 7、active/helper peak 1、最后 0。

`analysis.json`、`root-verification.json` 和 `root-timer-review.json` 交叉验证。首次失败、插桩 diff、构建日志与原始样本保留。测量副本不得替换正式 App。完整 Windows/双设备/R7 仍未验收，Release NO-GO。
