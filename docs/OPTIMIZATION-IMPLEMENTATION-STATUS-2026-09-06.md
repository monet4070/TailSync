# 优化方案实施状态（更新于 2026-09-19）

本文是 [`OPTIMIZATION-PLAN-2026-09-05.md`](OPTIMIZATION-PLAN-2026-09-05.md) 的实施记录。方案文档保留目标、约束、回滚边界和外部验收要求；本文只记录当前分支已经落地、实际执行过的门禁，以及仍需 GitHub、Windows 或双设备环境完成的事项。

## 当前快照

- 分支：`codex/runtime-contracts-and-recovery`
- 实施起点：`fd9257695a5a33853faa9859ffa86e56c397b24c`（`origin/codex/reliability-hardening`）
- 本轮代码收口提交：`5718b53`（依赖解析、性能基线、64 MiB 预览、CI 汇总门禁）
- 工作区策略：只纳入本方案文件；未清理、重置或提交演示文稿、Pelican 预览及历史审查草稿。
- 版本不变量：应用版本 `2.2.2`、设备间 wire `v4`、数据库 schema `v11` 未改变。
- 本地环境：Apple M4（10 logical CPUs）、16 GiB、macOS arm64；Rust 1.91.0、Node 26.0.0、Swift 6.1.2。

## 工作包状态

| 工作包 | 当前状态 | 已落地内容 | 仍需完成或不能由本机证明的事项 |
|---|---|---|---|
| O00 | 本地完成 | 新增无敏感内容的基线采集脚本，记录提交、工作区差异摘要、工具链、硬件和五份锁文件 hash；测试数据只写隔离临时目录。 | 长期归档应由 GitHub run/artifact 或发布证据仓完成；本地 `/tmp` 证据不是发布凭据。 |
| O01 | 实现完成、产品验收待远端 | Windows local API 使用 `JoinSet` 周期回收并在关闭时有界 drain；10,000 个短生命周期任务验证回收集合归零。 | 当前用例验证回收机制，不等同 10,000 次真实 TCP 请求；需 Windows 原生 CI/长时进程观察资源趋势。 |
| O02 | 代码完成、本地通过 | CI 可被 release 通过 `workflow_call` 复用；新增稳定名 `Required verification` 汇总 job，任一矩阵 job 失败、取消或跳过都会阻断；发布 provenance 校验完整 SHA、锁摘要和产物 hash。 | GitHub ruleset、取消/失败注入 rehearsal、签名账号与正式发布权限只能在远端验证。 |
| O03 | 本地完成 | 同目标比较共享生产依赖闭包的版本与来源；10 个关键依赖严格比较 resolved features；其余 Cargo 全局 feature union 明示但不误报；错误包含最短引入链；例外必须声明 context/target/reason/owner/reviewWhen。 | 依赖升级后仍需执行真实网络/打包回归；当前没有配置例外。 |
| O04 | History 基线完成，其余生命周期基线部分完成 | runtime 输出匿名 queue/lock/hold 时序；固定摘要的 1k/10k/50k 加密历史数据各执行 5 轮，记录冷/热 p95、p99；整次测试峰值 RSS 约 102.1 MiB。 | 该 RSS 是测试进程峰值，不是 Swift+daemon+渲染器进程树；文件 ACK、30 分钟混合负载和跨设备吞吐仍需专用环境。 |
| O05 | 本地完成 | `tailsync-runtime` 统一有界执行、History 分页/恢复/预览/写操作和提交后 revision 通知；平台 Adapter 只做 DTO、剪贴板及窗口接线。 | 真实窗口关闭、文件接收并发、磁盘/数据库故障仍属于 L3/L4 产品验收。 |
| O06 | 本地完成 | 搜索按 128 条/2 MiB 预算准备候选，解密匹配在 DB 锁外执行；读请求可合作取消；revision 变化最多重试 3 次；预览先持锁准备打开的文件，再在锁外读取、校验和编码。 | 真实 UI 快速切换、并发接收与 1/16/64 MiB 端到端进程树指标仍需 L3 验收。 |
| O07 | 本地完成、真实网络待验收 | 单一 `PeerRefresh` worker 统一 mode/epoch、候选合并、探测计划、缓存降级、健康提交、预热和 waiter 完成语义；测试 Adapter 覆盖失败缓存、旧轮次隔离、1,000 次唤醒合并、reset/shutdown。 | LAN/Tailscale/Iroh direct/relay 的真实路径、系统发现 Adapter 和双设备恢复仍是 O09。 |
| O08 | 部分完成 | 24 个生产响应 DTO 从 Rust schema 生成 Swift/TypeScript 解码器和有效/无效 fixtures；RuntimeSnapshot、History、Peer、能力声明及真实命令注册清单已进入检查；生成器可重复且 `git diff --check` 通过。 | 请求 DTO 尚未全量生成；`supports_stable_errors` 仍为 `false`，大部分命令仍返回兼容文本错误；不能宣称稳定错误契约完成。 |
| O09 | 未完成（外部门禁） | L0 Core、L1 runtime、L2 同 host interop/契约入口和 Community macOS bundle smoke 已具备。 | 真实 macOS↔Windows、LAN/Tailscale/Iroh direct/relay、N−1、NSIS、签名安装升级、重启/断线恢复需要 Windows 与两套可销毁设备环境。 |
| O10 | 功能门禁本地完成，比较性能待外部 | macOS 认证 socket 支持 TSPV 二进制预览、严格长度/尺寸/请求标识、断开取消和旧命令兼容；64 MiB 实际 socket、50 次切换取消、描述符复用、畸形/截断及提前断开测试通过。 | 尚未用同一素材比较旧 JSON 与新二进制路径的 Swift+daemon+渲染进程树 RSS，因此不能宣称“降低 20%”达标。 |
| O11 | 未触发（可选） | 网络/更新/剪贴板已有 canonical source，平台文件保持 Adapter 接线。 | 方案触发条件“一个 include 的改动需要同步多个父作用域导入”尚未出现；仍有 `use super::*` 隐式依赖，但不为清零表格强行迁移。 |
| O12 | 未触发（可选） | 未改变 Storage 根目录或 schema。 | 尚无并发多 Storage 实例或产品迁移需求；启动该项前需独立回滚和数据迁移设计。 |

## 2026-09-19 History 性能证据

命令使用 release 构建、生产加密/schema、固定合成内容和隔离临时数据库。每组 5 轮；在 5 个样本下 p95/p99 都等于该组最大值，不能把小样本误写成长期统计分布。

| 行数 | 固定数据摘要前缀 | 首屏 p95/p99 | 命中 50 条 p95/p99 | 无匹配全扫 p95/p99 | 冷无匹配 |
|---:|---|---:|---:|---:|---:|
| 1,000 | `0739291e8046` | 0.31 / 0.31 ms | 2.92 / 2.92 ms | 3.78 / 3.78 ms | 5.11 ms |
| 10,000 | `17d6a40ac747` | 0.83 / 0.83 ms | 4.29 / 4.29 ms | 54.07 / 54.07 ms | 52.33 ms |
| 50,000 | `85053d1d7125` | 2.28 / 2.28 ms | 11.10 / 11.10 ms | 600.79 / 600.79 ms | 609.68 ms |

整次 1k/10k/50k 测试进程由 `/usr/bin/time -l` 记录 `maximum resident set size = 107,069,440 bytes`，`peak memory footprint = 90,309,304 bytes`。该数字只用于后续同机回归，不代表产品进程树内存。

## 已执行的本地门禁

以下结果都对应本分支当前实现；忽略项和环境边界单独列出。

```text
node --test scripts/*.test.mjs                         53 passed
node scripts/check-local-contracts.mjs --root .        24 DTO / schema v1 / wire v4 passed
node scripts/check-shared-resolution.mjs --root .      macOS + Windows identity/feature policy passed
node windows/scripts/check_cross_platform_sync.mjs ...  60 Swift API commands passed
cargo test --locked --manifest-path shared/rust-core/Cargo.toml -- --test-threads=1
                                                        385 passed, 1 manual perf test ignored
cargo test --locked --manifest-path shared/tailsync-runtime/Cargo.toml
                                                        17 passed
cargo clippy --locked (Core + runtime + macOS product)  passed with -D warnings
bash scripts/check-windows-host.sh                      fmt/check/test --no-run passed
npm test -- --run (windows)                            214 passed
npm run build (windows + site)                         passed
TAILSYNC_WEB_SVG_RENDER_TESTS=1 swift test ...          181 passed
cargo test --locked --manifest-path macos/src-tauri/Cargo.toml --lib
                                                        77 passed, 1 live Tailscale test ignored
./macos/build-dmg.sh                                    Community arm64 DMG built and hdiutil verified
bash macos/scripts/verify_macos_bundle.sh               packaged daemon/API smoke passed
git diff --check                                        passed
```

Swift 测试输出中的 Contacts/CoreData XPC 错误来自测试主机没有可用联系人服务；181 个测试均通过。Windows `oxlint` 返回成功，但仍报告既有的 `set-state-in-effect` 和生成 decoder `double-comparisons` warning；这不是零 warning 证明。live Tailscale 测试按声明忽略，不能由其它绿灯替代。

生成的 Community DMG 为 ad-hoc、未 notarize 构建：`TailSync-2.2.2-macOS-arm64.dmg`，本轮 SHA-256 为 `f4c9b9f0408104e6c68b740170c2a6464e9535156848dc376c45b71bfd70cb31`。验证前临时退出已安装 TailSync，完成后已恢复应用及 TCP 19890 监听。

## Go/No-Go

**本地合并候选：Go。** O01–O07、O10 的实现与本地门禁可以进入远端 CI；O08 必须按“部分完成”评估。没有数据库 schema 或设备间 wire 变更。

**公开发布：No-Go。** 在 `Required verification` 的 GitHub run、Windows 原生打包/烟测、真实双系统与网络路径、N−1、签名/notarization、安装升级，以及 O10 新旧路径进程树 RSS 对比全部留存证据前，不能把阶段 C 或正式发布标记为通过。
