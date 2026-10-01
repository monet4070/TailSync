# 独立复核整改执行记录

> 本文件的测试数字和包证据对应上一轮源码，现为历史记录。2026-10-01 后续复核、修复及当前证据见 [FOLLOW-UP-2026-10-01.md](FOLLOW-UP-2026-10-01.md)。

依据 [独立复核报告](INDEPENDENT-REVIEW-2026-09-30.md) 实施；用户已在本聊天授权。起点 `914638d125e651c9d08052d20a8cc9ad77deda9a`，工作树 `/Users/monet/TailSync/TailSync-remediation`。执行从 2026-09-30 延续至 2026-10-01。代码和报告目前在本地，未提交、推送或合并 PR；已有审查文件及其他工作树保留。

本轮本机代码整改、九轮生产 worker 基线和最终全量复验已完成。原生 UI/设备验收仍有明确待验与阻塞，不能宣称整个发布验收已完成。31 项台账收窄为 **26 fixed_gated / 3 partial / 1 unfixed / 1 accepted**；S2-F5 与 S3-P1-2 保留 partial。这里的 gate 状态是限定断言的实现/测试状态，**不代表当前未提交代码已跑过 GitHub CI、Windows 原生或真实双设备验收**。上一轮 CI 是历史证据；本轮源码指纹、日志与验收分别记录。

## 逐项实施

| 问题 | 当前结果 | 修复与证据 |
|---|---|---|
| R1 共享载荷误删 | 本机已修/门禁通过 | delete、clear、quota eviction、重复替换统一按 live 文件身份保护，preserve_path 亦按身份；引用/身份不确定时保留，不新增迁移/计数表。大小写别名与 hardlink 反例红→绿 |
| R2 遗忘后迟到 ACK | 本机已修/门禁通过 | Core revoke_peer 在 state→settings→pending 同一锁序中使旧 session/generation 无效；macOS Unix、macOS Tauri、Windows Tauri 三入口调用同一用例；随后 disconnect pool，即使 note 清理报错也执行 |
| R3 提交点与 UI 状态 | 本机已修/门禁通过 | trust 写成功及 Paired/清窗口、控制/期限在同一 state 临界区完成；transport shutdown 仅收尾。commit 后 cancel/expire 不改成失败，撤销必须走 revoke；shutdown 卡住反例红→绿 |
| R4 先报 persisted 后 pin 冲突 | 本机已修/门禁通过 | 发送 completion 前校验已有 pin，promote 保留重检；共用 Settings key 规则。已知冲突不会先向对端报告完成 |
| R5 真实性能与 worker 隔离 | 隔离实现及门禁通过；九轮本机基线通过 | 认证 worker 的旧行为测得 500 ms 文件 ACK 将文本拖住约 503 ms；同 peer 事件/文件各有 worker/认证连接，候选、pin、重连、撤销复用，批次仍 per peer 串行。文件故障仅重建文件 lane，禁用/遗忘关闭全部。9 轮基线全部通过；原生 Windows/设备仍待验 |
| R6 警告展示/确认 | 本机已修/门禁通过 | showNotice 明确接受/拒绝；React layout commit 的可见 key 才算展示回执；cooldown/隐藏时重试，RPC 失败只重试 ack，不延长 notice TTL。实际 History cooldown 组件反例红→绿 |
| R7 通知窗口竞态 | 本机已修/门禁通过 | entries/earliest/dropped 在一个 mutex 中取快照；确定性 overflow 交错反例红→绿。Swift gap 按现有通知设置发一次提示；restart policy 保留 |
| R8 五秒 checkpoint 与异步阻塞 | 本机已修/门禁通过 | checkpoint 临时 busy_timeout=0，并恢复原 5000ms 策略；忙/失败登记 deferred，maintenance 补偿；磁盘重开登记一次重试，持有 reader 的重启反例红→绿。receipt/配额扫盘与淘汰、GC 在 spawn_blocking 中执行，准入锁仍覆盖整个流程；默认配置的约 5.33s 红阶段已转绿 |
| R9 pending 原子性/恢复出口 | 本机已修/门禁通过；双设备待验 | clone→私有原子保存→替换内存；未知版本/坏文件只读保留。Core 状态提供生成契约摘要；Windows/SwiftUI 显示未完成状态、指纹、重配/遗忘，不把 pending 当 active；恢复必须匹配之前确认的 key |
| R10 LAN 广告边界 | 本机已修/门禁通过 | 只选当前 IPv4 listener 支持的私网 IPv4/APIPA；排除 Tailscale-only、ULA-only、fe80-only，无地址显示不可用。APIPA 是有意支持，原台账断言已纠正；未做全 IPv6 改造 |
| R11 门禁脱离实际入口 | 本机入口门禁通过；Windows DACL 待原生 CI | 实际认证 Unix long poll/断线/semaphore；实际 incoming 创建检查 Unix mode，Windows native branch 检查 protected DACL/允许的 trustee；实际 fanout 发往1/2/8 peers、每 peer FileResume/restart，统计真实 hash 调用并检查完成后源变化。对手 ACK 是 seam，不是设备传输 |
| R12 atomic temp 永久豁免 | 已知 writer 格式已修/门禁通过 | writer 创建前登记 lease，GC 同锁判断并 unlink，且保护存活 foreign PID。旧进程 exit 不执行 Drop 后的残留可回收；过 grace 的 active writer、未知名均保留。**旧版无 PID 的临时名保守保留**，需要排他升级清理/人工核对，不能宣称所有历史 temp 已清零 |

## 轻量性及兼容边界

- 数据保全复用现有文件身份和 GC；未引入新数据库表、持久字节账本或第二套 GC 服务。checkpoint 重试复用已有维护 tick。
- 配对复用已有 wire 命令和 sidecar，不增加 Settings 顶层字段；旧版 deny_unknown_fields 配置夹具继续保留。旧 UI 可忽略新增 JSON 字段；新客户端因 `pending` 字段非可选而无法解码旧 daemon 状态（实际影响有限，因 app 与 daemon 同包发布）。
- 恢复摘要来自同一 Core 状态，通过现有 schema 导出生成 TS/Swift/fixtures；未推进受 PR #68 前置条件约束的 ConnectionsView 大规模状态所有权重构。
- worker 隔离是经真实阻塞反例证明后才加入：最多按需每 peer 两条连接，peer 级批次串行/模式/pin/限流/显式撤销策略保留。旧版接收端可接两条已认证会话，但真正旧版双设备互通仍需验收。
- active 与 pending 的分布式不确定性不可由有限消息消除，可能持续至恢复；Paired 只表示本端 durable trust 已提交。pending 不自动提升，改变 key 必须先显式遗忘。

## 验证与证据

最终复验（日志与 JSON 见 [证据目录](implementation-evidence-2026-10-01/README.md)）：

| 范围 | 最终结果 |
|---|---|
| Core | 单元486 passed / 3 ignored；集成2 passed / 1 ignored；无失败 |
| tailsync-runtime | 23 passed |
| macOS Rust all-targets | 112 passed / 2 ignored；callback诊断已另行执行，live Tailscale发现仍ignored |
| Windows 前端 | 49套件、246 passed；build通过；lint退出码0，历史warnings保留 |
| Swift | 211 executed，3 skipped，208 passed，无失败 |
| Node脚本 | 116 passed |
| 严格 Clippy | workspace与macOS all-targets `-D warnings` 均通过 |
| 契约/台账 | 跨平台、31个DTO生成核对通过；台账31项、rust-macos适用28个具名gate均在真实运行结果中通过 |
| Windows源码 | macOS host编译通过（有原有cfg dead-code warning）；实际private_fs模块含新增Win32 DACL/进程检查在Windows目标类型检查通过，未执行原生代码 |
| macOS release包 | 当前源码`.app`构建/严格签名/包结构/版本元数据通过；Community DMG构建/checksum通过，只读挂载后8个包内文件与原app哈希一致 |

四个台账套件的记录绑定同一个当前tracked源码指纹；独立source-snapshot.json还收录新增未跟踪源码，补足既有工具只哈希tracked文件的范围。未提交代码未跑新的GitHub CI。Windows typecheck不计为原生运行/DACL验收，Swift跳过项不计为通过。


前端 lint 的历史 warnings 与构建结果分开记录；新 hook 的 render-time ref 写入已改成 layout commit 后写入。不会为了清零与本轮无关的旧 warnings 扩展改动。

变异验证在临时副本进行，主工作树未被改坏。已实测八项 red→green：R1 unlink identity、R5 lane 选择、R9 恢复 key、R10 IPv6、R11 incoming 调用点、R12 temp 豁免、R11 实际 API whitelist、R11 实际 fanout 哈希次数。JSON 包含被测源码 SHA256、变异 SHA256、测试名与退出码；另保留此前 R2/R3/R4/R6/R7/R8 的红/绿执行日志。

首次 DB 验证使用 `/tmp` 符号链接路径触发既有 private_fs 拒绝策略；修正为 `/private/tmp/tailsync-implementation.BzV6PH` 的隔离 data/v1 后通过，未修改 HOME/真实配置/权限策略。第一版性能驱动使用单线程 + Skip，漏掉应尝试的文本（第一轮 692/1200），已中断并保存为无效驱动记录，不能用作验收。

## 性能测量

九轮均通过：每轮 attempted/enqueued/received/ACKed = 1200，256 MiB 完整处理，文本 p99 45.236–51.896ms / max 55.616–67.092ms，事件队列占用峰值均2/64（含 reserve 槽位），并非签署的共享 priority 水位；旧 rejected/expired 字面零不是观测。延迟与收发数字为当时观测，不是界限。完整数表及方法见 [性能预算](../performance-budgets.md#生产-worker-本机观测2026-10-01)。

新基线 `s2_f5_authenticated_worker_baseline` 使用与应用一致的多线程执行器（4 worker）、生产 pool/worker/executor、Noise 认证 duplex、实际 Core 文件接收/.part/摘要验证/提交与文本处理；合法 16 文件 ×16 MiB，共 256MiB。每 50ms 计划一个文本、固定尝试1200次，Burst 补迟到事件且端到端从计划时间起计，核对实际 enqueued/received/ACKed。文件 ACK 人为延迟250ms保持持续文件负载；FileMeta ACK 0/50/500ms，每场景三轮。按 enqueue reservation 记录队列峰值；分别报告 file-active 与全程 p99/max。

该基线不测原生系统剪贴板/历史落盘回调、Windows 或真实网路；BenchmarkPlatform 的 files_received 是 stub，不能把它写成完整应用双设备验收。即使本机全部通过，S2-F5 仍保留 partial，原生与设备证据另补。旧 scheduler_model_latency_baseline 仅是调度模型，不再负责关闭性能问题。

macOS 实际平台进度回调的200次锁竞争注入：人为持 FILE_PROGRESS 50ms，callback p50 60024µs / p99 60117µs / max 61882µs。该实验说明同步回调确实可被平台锁拖住；没有证明实际设备负载中20ms预算成立或失败。S4-P1-4 仍 unfixed，暂定20ms仍需真实双平台采样。

## 必须保留的待验/限制

1. 当前改动的 GitHub 原生 Windows CI、Windows打包与原生UI；macOS最终包已构建/核验，但运行smoke和托盘/VoiceOver因具体本机阻塞仍待验，历史全绿不能覆盖此工作树。
2. 两台设备/两平台的配对中断、重启、同 key 恢复、遗忘/迟到ACK以及 UI 一致性；有限消息的 active/pending 不确定性不宣称消除。
3. 真实大文件/剪贴板传输性能、S4-P1-4 回调与引擎锁分别 wait/hold 的设备测量；20ms仍为暂定。
4. 原生 VPN/虚拟接口/断网，Windows文件ID、DACL与进程检查分支；IPv6 LAN/scoped地址广告仍未实现。
5. 未知/旧版无PID temp 的安全回收不能靠年龄猜测，排他升级清理另议。
6. PR #68 的合并与后续 ConnectionsView 所有权重构未在本轮执行。

完整 [归档证据](implementation-evidence-2026-10-01/README.md) 包含实际门禁JSON/原始日志、九轮性能JSON、无效性能驱动、变异红/绿与对应源快照、最终源码哈希清单及验证摘要。调试临时目录仍保留；审查证据源码/日志保留；独立探针Cargo清单按文本快照归档并更新复现步骤，避免触发生产test-support守卫。

## 本机原生运行的具体阻塞

已经构建 [release .app](../../macos/TailSync.app) 与 [Community DMG](../../macos/release/TailSync-2.3.1-macOS-arm64.dmg)，并通过严格签名、包结构、DMG校验与只读内容核对。运行时smoke在执行产品脚本的端口预检处停止：`/Applications/TailSync.app/Contents/MacOS/tailsyncd`占用19890，其UI拥有每用户的SingleInstanceLock。没有停止/覆盖已安装实例，也没有运行会覆盖系统剪贴板的后续helper round-trip。Computer Use读取已安装TailSync返回`timeoutReached`；新包托盘、pending恢复界面和VoiceOver实际朗读均未认证。**这是本机运行阻塞/人工验收待办，不能列入“缺第二台设备所以不能做”。**

## 后续验收顺序

1. 本轮改动提交后在当前SHA跑原生Windows/macOS矩阵；检查新增DACL、进程状态分支及每个具名gate实际执行。归档的本地结果不得搬作新提交的CI认证。
2. 在已安装TailSync无传输且退出、19890与单实例锁释放后，对当前release包用隔离DATA/V1/API_SOCKET路径执行daemon smoke；clipboard-helper round-trip前保存/恢复系统剪贴板。启动最终包检查托盘、连接窗口、pending摘要/重配/遗忘与daemon重启。此操作不能覆盖用户真实配置。
3. 按VoiceOver清单做中英朗读、焦点和值/状态；该清单原范围含PR #68控件，需要相应版本与配对设备。无障碍树/编译只能补证，不能代替实际朗读。
4. 两设备执行同key重配、双方重启、在注记/commit/ack各边界断线、遗忘/迟到ACK；记录本端active与对端pending持续到恢复的行为，不能把有限消息说成原子分布式提交。
5. 实际文件/文本并行传输采集端到端ACK与零丢；对S4-P1-4分别测引擎lock wait/hold与平台callback（现有200样本只量callback总耗时）。若设备预算确实不达标，再移动回调并补序号/完成不可覆盖门禁。当前不以注入50ms本身决定设备20ms预算。
6. Windows原生VPN/虚拟接口/断网；S1-P1-5明确仅Windows，其partial理由统一为当前原生证据待验，macOS等价工厂gate单独待补。旧无PID temp须有排他升级清理/人工核对条件。PR #68的合并和大型状态所有权重构仍另行授权执行。

当前不更新“可发布/所有验收完成”结论。
