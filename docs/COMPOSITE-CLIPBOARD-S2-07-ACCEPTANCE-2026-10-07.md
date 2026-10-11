# S2-07 整体验收、性能与封存

日期：2026-10-07，Asia/Shanghai。

**结论：本轮本机自动验收与无界面 helper 微基准完成；S2-07 整体仍为 PARTIAL，完整 S2 未完成，Release NO-GO。** 真实 QQ/A02/A07、GUI、Windows 原生、双设备、hosted CI 与实际 App 性能没有新增现场证据，不以自动测试或合成样本冒充完成。

## 1. 范围、流程与源码绑定

本轮沿用一个 DeepSeek/high 子代理实施，完整交接并关闭后，父代理独立复现、二次修复、冻结同源码全量验证和封存。

- 唯一工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`。
- 分支：`codex/composite-clipboard`；HEAD：`f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。HEAD 不代表这份含大量未提交工作的完整候选。
- 实施/首次复核 run：`artifacts/clipboard-improvement/2026-10-07-s2-07-acceptance-r1/`；最终修复后全量复验与封存 run：`artifacts/clipboard-improvement/2026-10-07-s2-07-acceptance-r2/`。除明确说明 r1 的记录外，下文证据路径相对于 r2。
- 子代理：Hegel，`01a11580-ca21-7ee3-9e1b-c9c35dec6faa`；实际模型 `cline-pass/deepseek-v4.1-flash`，high，关闭历史 fork。完整明文任务、有效配置、任务/工作树 ACK 与完成交接分别保存在 `assignment.txt`、`delegation-config.json`、`delegation-receipt.json`、`review/child-completion.json` 与 `agent/HANDOFF-S2-07-PERFORMANCE.md`。

| 冻结点 | 文件数 | 源码指纹 |
|---|---:|---|
| 本轮开始 / S2-06 最终 | 835 | `b110e01de38da668da433328ce3c4b1972136f958db48f145d1ead8a8b8f93ce` |
| 子代理交接 | 837 | `43a51cb9c1fafcf70bf533c5896f380c676e4833aeb9916c08c2b5273901fa18` |
| 父第一次二修 / r1 被替代版本 | 837 | `02706fe63b8b31c49d8e2172d438478e7c0c9e644cc801da56fdfeb6dc9f4779` |
| 父最终追加二修 / r2 | 837 | `139adff2062ecc42bf89413c1fabff9999b58d1a0357f81076b7ebd31495ff01` |

四个冻结点分别保存完整文件清单与 `.txt` 字节副本；r1 被替代版本在其只读封存中，r2 重存初始/交接/最终三份完整快照。指纹覆盖产品/测试/工具/配置/锁文件；Markdown 和其他 artifacts 不参与该指纹，分别封存。子代理只在 r1 实施一次，r2 由父复验；递送记录与交接是带来源复制，不伪称重新派发。

本轮仅新增 `scripts/s2-performance-report.py`、`scripts/tests/test_s2_performance_report.py`，以及本报告与[性能协议](COMPOSITE-CLIPBOARD-S2-07-PERFORMANCE-PROTOCOL-2026-10-07.md)。**835 个既有源码/测试/配置文件逐字节不变**，无删除、无产品行为改动、无依赖/schema/锁文件变化。已封存的主计划、S2-05/06 报告不改写；本报告作为追加状态记录，不回填旧证据。

工具是有界、只读、离线报告器，复用 S2-06 安全加载原语；只接受显式输入及可选基线。`REPORT_COMPLETE` 仅表示报告生成完成，允许包含 UNKNOWN，不是性能验收 PASS。四个 hash 是申报值，由外部封存核对，不是工具自己认证的产物。

## 2. 独立审查与二次修复

子代理原有 45 项单测通过，但父独立探针在交接源码上实际得到 **0 PASS / 6 FAIL**。没有接受其 PASS 报告作为最终判据。

第一次二修的 27 项门禁通过后，补充独立数值边界探针又实际得到 **0 PASS / 2 FAIL**。因此没有封存该版本为最终成功：r1 标 `SUPERSEDED_NUMERIC_BOUNDARY_DEFECTS`，保留历史结果与失败，追加修复后创建 r2，重新冻结并完整重跑，而不是继承 r1 的绿色矩阵。

| 属实缺陷 | 二修与回归 |
|---|---|
| 极大 JSON 整数使 `math.isfinite` 抛 OverflowError | 有界数值验证捕获溢出，输出固定 INVALID，而非 traceback |
| 正微小时延被六位舍入变成 0 | 样本统计保留原精度；`1e-10` 的 p95 不变成 0 |
| 百分比先舍入，略超过 10% 被判 OK | Decimal 基于原始 p95 作比较；超过阈值仍 REVIEW_REQUIRED |
| 合法极端比值输出 Infinity，破坏标准 JSON | 超范围 delta 为 null，标 DELTA_OUT_OF_RANGE，判定保持；序列化禁 NaN/Infinity |
| 两侧没有实测样本仍宣称 COMPARED | 无实际 measured pair 返回 NOT_COMPARABLE / NO_MEASURED_PAIRS |
| 底层 read syscall 的 OSError 未处理 | 固定 E_INPUT_READ，错误输出不回显私有路径或 traceback |
| 默认 Decimal 28 位上下文仍会丢失大整数略超阈值的增量 | 局部 1024 位精度，覆盖已准入有限 int/float 的数位/指数跨度；不改变全局上下文 |
| 输入小于 64 KiB，但巨型计数器求和后超过 Python JSON 整数转换限制 | 序列化前 fail-closed，固定 INVALID / E_OUTPUT_NUMBER；不放宽全局限制，不输出部分报告 |

独立探针最终 **8/8 GREEN**；上述八项进入持久测试，报告工具最终 **53/53**。Python 全套最终 **172/172**。RED 与 GREEN 命令、源码前后、原始日志及 hash 均保留，失败不覆盖。

RED 在 r1 的 `commands/parent-real-red-performance.json` 与 `commands/parent-real-red-numeric-edges.json`；最终 GREEN 在 r2 的 `commands/parent-green-v2-independent.json`、`commands/parent-green-v2-performance.json` 与 `review/parent-combined-regressions.py.txt`。子代理合成 smoke 只是统计逻辑测试，不是实际性能数据。

## 3. 最终同源码门禁

以下 **27 项门禁全部通过**，执行前后均绑定最终指纹。每项完整 argv、固定环境、退出码、耗时、原始日志 hash 与实际测试计数分别保存在 `commands/parent-final-*.json`、`logs/parent-final-*.log` 与 `sources/`；汇总为 `review/final-gates-summary.json`。

| 门禁 | 本轮实际结果 |
|---|---|
| Core default，test-support | 顶层 737 PASS / 6 既有 ignored（lib 735，integration 2） |
| Core all-features | 顶层 765 PASS / 7 既有 ignored（lib 748，integration 17） |
| Runtime all-features | 96 PASS |
| Runtime all-features + Core composite | 96 PASS |
| Protocol | 63 PASS |
| clipboard helper crate | 3 PASS |
| 独立 Rust 探针 | 16 PASS；重跑旧 S2-05 探针，不计为本轮新产品测试 |
| macOS Rust lib composite | 119 PASS / 3 既有 ignored |
| Swift | 307 PASS / 4 既有 skipped |
| React/Vitest，隔离 runner | 366 PASS |
| Windows Rust macOS-host，default + composite | 分别 128/129 PASS，各 2 ignored；合计 257/4，不代表 Windows 原生 |
| Python 全套 | 172 PASS，包含新性能工具 53 项 |
| native saved-bytes | 5 个实际语义检查通过，不以框架零测试充当通过 |
| generated model/decoder 探针 | 6 个实际语义断言通过 |
| TypeScript、workspace Clippy、macOS Clippy、三份 fmt | 全部通过，Clippy 保持 `-D warnings` |
| contracts、canonical、production features、shared resolution、native policy、capture visibility | 全部通过 |
| 输入与七份旧 seal 审计、30 次 helper 微基准、git diff --check | 全部通过 |

Core 计数按本轮 Cargo 分段，取各段最后的顶层完整摘要，并逐段校验 running 数、passed/ignored/filtered；不继承其他阶段数值。旧通用 raw parser 会把嵌套 process_worker 摘要混入，且并发输出可打断摘要，因此 r2 记录中的 738/773 是该 parser 的原始聚合，不是唯一/顶层用例数。本轮不修改既有 parser，也不改写记录；新增独立分段验真得到上述 737/765 顶层结果。配置之间存在重叠，**不把这些数量相加称作唯一用例总数**。测试计数为零的静态门禁仅声明其静态检查结果；native/generated/performance 另验非空语义结果。

Windows-host 原有 dead-code warning 原样保留，包括 deadline/retry 与 Win32/InvalidData 分支；未加 ignore/allow、未删平台分支、未隐藏告警。本轮 835 个既有文件不变，也没有新增 skip 或削弱旧断言。

父最终独立复核另外重新调用同一冻结 helper，验证五份公开 saved-bytes 样本；30 次计时输出的长度/hash 与复验的 RTFD 输出一致。统计由 raw trial 和原始 `/usr/bin/time -l` 日志独立复算，不仅信任 CLI exit 0。

## 4. 实测性能与限制

只测 `helper_analyze_rtfd`：公开合成红/蓝双附件 RTFD 保存字节，**30 次真实调用**，每次校验正文、附件字符位置、红蓝 PNG hash 与完整 RTA1 facts。

- 环境：macOS 27.0.1（26A434），arm64，Mac16,12。
- helper 由本轮 `xcrun swiftc` 默认未优化编译，登记为 debug；不是 release 包或完整 App。
- wall 使用 `perf_counter_ns`，包含 `/usr/bin/time`、helper 进程启动、管道和调用包装成本，不是纯转换耗时。
- CPU/RSS 来自每次 `/usr/bin/time -l` 的子进程记录；RSS 为 Darwin 字节值，不用累计 `RUSAGE_CHILDREN` 伪造单次峰值。
- 无 warmup，OS cache 未控制，不声称冷启动或热缓存场景。

| 实测指标 | p50 | p95 | 范围 |
|---|---:|---:|---:|
| helper 调用 wall | 12.828917 ms | 13.940916 ms | 12.401250–22.272750 ms |
| helper peak RSS | 19,562,496 bytes（18.65625 MiB） | 19,628,032 bytes（18.71875 MiB） | 19,513,344–19,628,032 bytes |
| user/system CPU | 原始输出均为 0.00 s | 原始输出均为 0.00 s | 输出精度有限，**不能解释为实际 CPU 为零** |

解码次数、转换次数、cache 命中/未命中、最终 lease 释放全部为 null/UNKNOWN，不能从循环次数推导。没有可比性能基线，比较结果为 `NO_BASELINE`，**≤10% p95 回退门未验收**。

helper hash：`7264298f6663861fac1722eab602b9464e281f238d69f7529bb772ed3ad5c33f`；fixture JSON hash：`96ab8ce3727eb5038d9236e57e1f82644acefdb4aadddf8d20df4b860a844b18`。`workload_sha256` 是规范化负载定义 JSON 摘要，不是采集脚本字节摘要；脚本另由 `review/auxiliary-final-source.json` 绑定。

原始证据：`review/headless-performance-raw.json`、`review/headless-trial-00..29-time.log`、`review/headless-performance-input.json`、`review/headless-performance-report.json`、`review/headless-performance-verification.json`、`review/native-performance-replay.json`。

## 5. Q01–Q24 与整体现状

逐项台账见 `review/acceptance-boundaries.json`。`automated_scope` 只是测试覆盖路由，不是各 Q 的完整 PASS；**24 项完整现场要求均仍 UNKNOWN**。本轮证明的是选定本机自动矩阵确实执行，不是每项产品要求均已现场满足。

| 专项 | 已有本机自动范围 | 仍缺的完整证据 |
|---|---|---|
| Q01–Q04 分类/正文/native-only/历史 | 内容分析、公开保存字节、隔离 DB 与重建回归 | 真实来源/目标、原生新旧历史及分类筛选观察 |
| Q05–Q08 菜单/诊断/失败/可达性 | Swift/React 策略、命令/本地化/键盘/IME 回归 | 原生菜单、主题、长内容、AX/VoiceOver 与实际提示归因 |
| Q09–Q14 文稿/首图/缓存/owner | 受控模型、样式/表格、首图顺序、版本/库/owner 隔离 | 原生渲染、实际资产更新/缓存生命周期、窗口/重开库观察 |
| Q15–Q20 安全/副作用/取消/预算/旧兼容 | 恶意模型/URL 拒绝、mock 副作用、worker 实际退出与准入回归 | 设备侧无网络/剪贴板变化、完整 App lease、codec stall 与交互响应 |
| Q21–Q23 QQ/持久对照/提示来源 | S2-06 离线证据 intake、失败状态与可见性代码门 | 获授权的 QQ 直接/历史粘贴成对证据，关闭来源/断网/重启，提示原文/时间/发出者 |
| Q24 长序列性能 | worker/cancel/admission 单测；另有 helper 微基准 | 实际 App 长序列 RSS、重复解码、cache、lease 与可比基线 |

当前 S2-01–05 的本机实现已有阶段记录，S2-06 只完成离线准备/校验工具，真实 QQ 未完成。本轮 S2-07 完成本机自动回归与有限微基准，但前置现场门未闭合，不宣布整个 S2 完成。

明确未验收：八个 GUI 性能场景（空闲、连续复制、列表滚动、首图、快速换项、重复文稿、取消、关闭）、真实 QQ/A02/A07、目标应用保真、AX/VoiceOver、Windows-native、双设备、hosted CI、完整 App CPU/RSS/cache/lease 与可比性能基线。它们全部 UNKNOWN；F03 完整性 UI 保留意见仍待用户决定，未擅自删除反馈。

本轮没有 GUI 演示用途，因此不生成重复的大 App 包：`candidate_package=NOT_BUILT_THIS_RUN`。旧 App/测试包不被重绑成本轮候选，测试源码不等于包已验收。

## 6. 封存、保护与停止点

七份旧产品 seal 及其绑定文档逐字节复验，包括 O05、S2-01–06；S2-06 的 2631 文件旧封存聚合保持 `c59261f37c93264e6319c0ab5f30045f884ceb70ced2997b1f8dfed420b6d15d`。另外只读复验 r1 的 2725 文件历史封存，聚合 `7ed430802b87d3c0b594a1ab7954692417b3a01a2e2ee032dc29deb8800f35d6`；其状态是被替代，不是最终通过。r2 的 `prior-run.json` 绑定该来源，避免覆盖失败。

本轮输入、子代理交接、独立 RED/GREEN、最终 raw gates、19 个冻结辅助输入、helper、fixture、性能原始日志、文档副本、三份阶段精确 patch 和状态统一封存。

- 本轮索引：`evidence-sha256.json`；状态：`status-final.json`。精确 delta 为 `review/before-to-agent.patch`、`review/agent-to-final.patch`、`review/before-to-final.patch` 与 `review/deltas.json`。
- 可只读复验：在唯一工作树执行 `PYTHONDONTWRITEBYTECODE=1 python3 artifacts/clipboard-improvement/2026-10-07-s2-07-acceptance-r2/review/verify-final-v4.py.txt --sealed`。
- 父首次附加证据复验失败于历史探针目录映射：把旧 `probe-v4` 当成旧 `probe`。保留 v1 脚本和失败记录，新增 v2 使用确切归档路径重新核验，不改产品源码、旧证据或放宽 hash 检查。
- r2 首次附加复验 v3 又因继承 r1 的混排摘要计数而失败；保留失败，v4 绑定本轮实际 raw 聚合，并增加逐 Cargo 段顶层摘要/running 精确验证。没有降低执行范围或把缺失测试直接标绿，27 项正式门禁与产品源码均不变。
- 源码/文档/证据变动后不得覆盖或重封这个已消费的 run；使用新 ID 重冻并运行受影响门禁。封存 hash 保证内部字节一致性，不声称独立签名或远程认证。
- Rust 串行复用唯一 `target/o04-shared`，jobs=2，关闭 incremental/dev-debug/test-debug。本轮检查时 target 约 5.2 GiB、run 约 44 MiB、磁盘可用约 68 GiB；未复制 target 到证据。
- 未读用户 DB、QQ 内容/缓存或 general clipboard，未启动/替换/停止日常 App，未删除缓存/旧证据/App/测试包，未暂存/提交/建分支。

回退仅涉及这两个新增 Python 工具及本轮新增文档/证据；不得整体 reset 既有 dirty 工作。工具不修改数据库或产品原件，无数据迁移回退步骤。

**本轮停止：LOCAL_AUTOMATED_COMPLETE_WITH_HEADLESS_MICROBENCHMARK；S2-07 PARTIAL，完整 S2 未完成，Release NO-GO。** 后续需要用户明确测试会话与来源内容范围，优先补 S2-06 QQ/A02/A07 和隔离候选的 GUI/实际 App 性能；本轮不自动进入 S4 或发布。
