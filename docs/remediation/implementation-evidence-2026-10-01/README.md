# 本轮整改证据（2026-10-01）

> 此目录是上一轮源码的历史证据。2026-10-01 后续修复及重新取证见 [后续复核](../FOLLOW-UP-2026-10-01.md)，不能用这里的 final-* 认证后续改动。

起点914638d；认证范围为当时本地未提交工作树。结论见 [执行报告](../IMPLEMENTATION-2026-09-30.md) 与 [结构化摘要](verification-summary.json)。不认证当前代码的GitHub CI、原生Windows执行或双设备行为。

## 最终验证

| 证据 | 范围 |
|---|---|
| [gate-results/rust-macos](gate-results/rust-macos) | Core、runtime、macOS、Swift四份实际执行JSON与日志；同一SHA及tracked源码指纹，所有适用具名gate通过 |
| [final-ledger-execution.log](final-ledger-execution.log) | 台账31项、rust-macos适用28个gate核对通过；accepted/partial的适用gate亦计入，并非28个完整修复 |
| [final-core.log](final-core.log)、[final-runtime.log](final-runtime.log)、[final-mac.log](final-mac.log)、[final-swift.log](final-swift.log) | 全套实际执行输出，ignored/skipped照实保留 |
| [final-frontend.log](final-frontend.log)、[final-frontend-build.log](final-frontend-build.log)、[final-frontend-lint.log](final-frontend-lint.log) | 246前端测试、build与lint；旧warnings未抹去 |
| [final-core-clippy.log](final-core-clippy.log)、[final-mac-clippy.log](final-mac-clippy.log) | workspace/macOS all-targets严格Clippy |
| [final-production-features.log](final-production-features.log) | 9个产品Cargo清单均保持test-support为dev-only；历史探针清单按.txt快照保存 |
| [final-scripts.log](final-scripts.log)、[final-contracts.log](final-contracts.log)、[final-cross-platform-green.log](final-cross-platform-green.log) | 116脚本测试、生成契约、跨平台字段/命令检查 |
| [final-windows-host-compile.log](final-windows-host-compile.log)、[final-windows-private-fs-types.log](final-windows-private-fs-types.log) | host编译与实际private_fs模块（含tests）的Windows目标类型检查。**均不等于Windows原生执行** |
| [final-mac-bundle-build.log](final-mac-bundle-build.log)、[final-mac-bundle-inspection.log](final-mac-bundle-inspection.log)、[final-mac-dmg.log](final-mac-dmg.log)、[final-mac-dmg-content.log](final-mac-dmg-content.log) | release `.app`/Community DMG的构建、严格签名、校验与只读内容核对 |
| [final-mac-bundle-smoke-blocked.log](final-mac-bundle-smoke-blocked.log) | 已安装TailSync占用19890，产品smoke在前置检查处退出1；未执行后续clipboard/helper/API运行认证 |
| [source-snapshot.json](source-snapshot.json) | 最终源码SHA256，含新增未跟踪源码；不只依赖既有tracked指纹工具 |

实际执行结果可在同一未改动工作树用 `node scripts/check-remediation-ledger.mjs --job rust-macos --results-dir docs/remediation/implementation-evidence-2026-10-01/gate-results` 再核对。提交/源码改变后须重新取证，不能复用此结果认证新SHA。

## 性能与错误观测

[worker-baseline.json](worker-baseline.json) / [原始日志](worker-baseline.log)：合法16文件/256MiB，生产pool/worker/executor、Noise和Core接收，三个FileMeta ACK延迟场景各三轮。固定计划1200文本/60秒，逐轮核对attempted/enqueued/received/ACKed；九轮通过。平台系统剪贴板/历史回调为stub，Windows与真实设备待验。性能运行在末次checkpoint重开边界补强和UI类型别名整理之前；R5生产worker及基线逻辑未变，最终普通全量门禁随后重新执行。

[无效驱动日志](worker-baseline-invalid-skip.log)：第一版单线程/Skip少尝试文本（第一轮692/1200），已中断。保留该错误，不能以它宣称满足零丢或60秒负载。

[platform callback诊断](progress-callback-mac.log)：200次实际macOS API进度回调，FILE_PROGRESS持锁50ms注入。只量callback总耗时，未分开引擎wait/hold；不是设备20ms预算裁定。S4-P1-4仍unfixed。

## 回归与变异

[mutations/results.json](mutations/results.json)保存8项坏行为变异的source/mutation SHA256、test与red=101/green=0；[源快照](mutations/source-snapshots)为这些记录对应的精确中间版本字节，各红/绿日志均保留。中间版本与最终源码可能有注释、checkpoint重开等后续改动，不能把中间SHA写成最终SHA。历史runner固定临时副本路径/目标缓存，仅说明执行方式，未来复现须新建隔离副本并更新路径，不能在主工作树直接运行它。

另保留R1原始反例、pairing-red/pairing-intermediate-42-tests（R2/R3/R4及R9，中间版本42例）、notice-red/green（R6）、notification-red/api-green（R7）、checkpoint-red/编译错误日志 checkpoint-intermediate-compile-error.log（不是通过证据）与checkpoint-reopen-red/green（R8）、ack-hol-red/green（R5）。红日志表示坏行为可达或变异被抓住，不是修复通过。早期/tmp符号链接导致private_fs拒绝的环境错误也保留。final-*与gate-results负责当前最终复验。

## 原生待验

本机release包的运行与托盘/VoiceOver尚未认证：已安装实例持端口/全局单实例锁，Computer Use读取已安装TailSync返回timeoutReached。未停止或覆盖它。Windows native CI/DACL、两设备恢复/续传/网络变化、引擎lock wait/hold与真实平台callback预算、macOS发现工厂等价gate、旧无PID临时文件的排他清理仍按执行报告保留。

旧 worker-baseline.json 中 rejected/expired 的字面零不是计数器观测；peak_priority_depth 仅测隔离后的事件队列，不能用于签署共享队列水位裁定。旧运行发生在后续源码变更前，保留作历史诊断。
