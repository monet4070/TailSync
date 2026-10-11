# R1–R5 补修与本地验收（2026-10-05）

本轮发现且可在当前工作区修复的 R1–R5 问题已经修复。结论为 **LOCAL_COMPLETE**；它表示下述源码与本地回归通过，不表示 Windows 原生、双设备、目标应用或发布验收完成。R6 可以以这份补修后的源码为输入继续集成验收。

## 工作区与证据身份

- 实际修改目录：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，分支 `codex/composite-clipboard`。
- 基线提交：`f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。R5 原有未提交源码指纹：`4e5125812219497e23fa272c275b5d1211280452e18a1078bad0ee66ab67788d`，736 个文件。
- 补修源码指纹：`05d9faa65cf1d5cc0897a0a30130ab01baf016d61be0dcbebfdb0cf1cd31267d`，740 个文件。逐文件清单和排除规则见 `artifacts/composite-repair/r1-r5-followup/source-files.json`、`source-fingerprint.json`。
- 未暂存、提交、推送、打包发布。原目录只保留原有未跟踪方案文档；未改原目录产品代码、用户数据、运行中的 App/daemon 或已有测试包。
- 补修前的未提交文件备份：`/var/folders/82/067680s94y703lgrz356cz5r0000gn/T/tailsync-r1-r5-before-0vi0uqes`。历史 R0–R5 证据保留；本报告是新证据，不追写旧 PASS。

## 修复清单

| 阶段 / 接缝 | 修复后的行为 | 本地验证 |
|---|---|---|
| R1 能力协商 | 真实 Noise 握手检查 helper、origin、journal 健康，不能仅凭 feature/env 宣告支持；健康读取器在自动发布受限时仍保存历史 | 真实 duplex Noise 握手四种健康组合；默认/启用 macOS 编译 |
| R1 启动恢复 | history 与 journal 打开、完整性检查、旧镜像导入和 prepared history 恢复在对外监听前完成；损坏原件保留并停止自动副作用 | journal 损坏回归、composite/legacy interrupted claim 启动恢复 |
| R2 验证与对象传递 | 结构验证产物携带共享 immutable wire/model 与预算指标；装配传给 receiver，无 engine 锁内大块 bundle 复制 | Protocol 全量、RR14、RR15、接收回归 |
| R2 内存与并发 | 装配、PNG/native 准备、capture 工作、发送缓存、恢复及预览工作显式预留；取消不提前释放仍运行 worker 的 lease；解码有身份级与全局上限 | RR16、取消 worker 配额回归、压力后恢复投递 |
| R2 操作表 | 满 1024 个活动 key 时拒绝新 key，保留已有 holder/waiter；不会通过淘汰等待锁突破上限 | 满载与 waiter 回归、RR19 |
| R3 原生异步端口 | 四类恢复入口和 legacy 收件使用 blocking port，native I/O 不占 Tokio worker、不持 engine 锁 | 计时器回归、慢恢复中取得 engine 锁 |
| R3 发布顺序与取消 | 全局 FIFO permit 随实际 worker 保持到退出；取消/撤销传入 supervisor，完成 kill/reap/join 后报告 Unknown，不重做旧事件 | 取消 FIFO/内存、运行中撤销、重复事件回归 |
| R3 来源恢复 | 成功回执关闭 intent；未消费回执不被 TTL 淘汰；持久化 token/digest/受监督 PID，stdin 提交前登记 PID；重启只在稳定观测建立新 epoch 边界 | durable origin、容量、成功后同内容新复制、原生 stall probe |
| R3 未知结果 | 丢回执后的稳定边界为 UnknownMismatch，不凭 token/hash 补造 SelfWrite；存活/无法确认的恢复操作保持保守，坏元数据不覆盖 | clear 后/写后 stall，未知关闭与新 revision 回归 |
| R4 历史与 claim 顺序 | history 正文先持久化；claim 与 history ID 原子绑定；失败不开始 native I/O。prepared history 可在启动时无 sender/无 native 恢复 | 磁盘失败注入、native 调用前可见历史、两类启动恢复 |
| R4 legacy 回执 | legacy 与 composite 共用独立 effect journal；两个兼容 JSON 均派生重建；严格有界导入，不吞损坏/读失败。legacy ACK 只确认历史接收 | receipt 全量、损坏镜像/journal、legacy crash recovery |
| R4 身份与 GC | 包括 terminal lookup 在内，每个操作核对 digest；重放 floor 单调，容量受限，删除历史不复活正文 | completed collision、clock floor、删除 tombstone |
| R4 编码与压力 | fan-out 共用缓存；缓存初始化串行，只缓存确定结果。暂时配额不足可重试，不能成为永久缓存失败 | 压力释放后同事件投递成功、共享缓存回归 |
| R4 截止 | 保留事件 ID、timestamp、选定表示、wire digest；每个 chunk write/ACK 受同一剩余截止限制；后入队 peer 不重新计时 | delivery 全量、fan-out 原始 deadline 回归 |
| R4 降级与终态 | RTF-only 无合法 fallback 明确拒绝；永久拒绝、部分、未知、超期均有可见提示，不误报“为保护新内容而过期” | RR06、legacy fallback 回归、双端本地化 |
| R5 采集确认 | 有界 revision 重试；只有保存正文或持久终态故障后才 acknowledge。保存失败不广播，不拿 RAM 成功冒充 durable history | capture retry、失败可见性 gate、双平台编译 |
| R5 pending history | 去重/裁剪/配额不删 native worker 仍依赖的条目；同时保留最新用户复制；有保护时禁止重映射存储。正常结算或取消后的最后一个 worker 退出释放保护 | 去重/配额、新复制、取消后保护释放 |
| R5 native-only 恢复 | 所有显式格式请求先检查 native-only；平台支持与菜单能力一致。来源 token/转换标记不进入 helper 发布集合，发布重新登记 token | Runtime RR08/09/21/22/24/30，RTFD native round-trip |
| R5 RTFD / file shape | helper 在截止内把可表示的内嵌附件转为有界 HTML/text/PNG，并保留原始 RTFD；不表示的属性保留 native-only。纯文件与混合 items 区分 | 命名板 RTFD Capture + Published + readback + SelfWrite；17-file probe |
| R5 Windows bitmap | 格式读取后立即关闭 clipboard，再做 DIB→PNG 转换；保留 PNG + CF_DIBV5 策略 | Windows helper MSVC 目标 portable-check、DIB policy gate；原生消费仍 UNKNOWN |
| R5 错误与告警 UI | 稳定错误码进入实际 Mac API/Tauri UI；只有 Published 报成功，Partial/Unknown 的重新复制是新用户操作。warning 队列有界、按 ID peek/ack，溢出可见；Mac 警告独立于 history version，隐藏/取消窗口不消费 | Swift/React 全量、warning queue/ack/overflow 回归 |
| 验证门禁 | RR20 两个分支均执行；RR14–RR16 和 Swift RR25 补入脚本；零测试、漏跑登记项、unexpected 均使门禁退出非零。native probe 每次检查当前源码构建，禁止仅复用旧二进制 | RR01–RR30；门禁解析器 7 项 |

256 MiB 是显式受管管线的预留上限，不能据此声称整个 App/OS 的 RSS 实测达标。预览 worker 中的解密、PNG workspace 和二进制编码受配额；返回后的 UI/IPC/OS 缓冲属于后续进程和设备测量边界。

## 最终本地验证

命令、退出码和耗时逐项保存在 `artifacts/composite-repair/r1-r5-followup/checks-{core,platform,gate,ui}.json`；对应 `*-final.log` 保存原始结果。最终源码一致性和日志摘要见同目录 `verification-summary.json`。

| 检查 | 结果 |
|---|---|
| Core，全部三个协议 feature + repair-regressions，串行，不排除 Iroh | 615 passed / 0 failed / 4 原有 ignored |
| 本轮新增 followup 集成回归 | 10 passed |
| Runtime 全量 | 24 单元 + 10 集成通过 |
| Protocol 全量 | 36 passed |
| Swift 全量 | 258 executed / 4 原有 skipped / 0 failures |
| Windows 前端全量 | 51 files / 268 passed |
| TypeScript、跨平台 contract、生成 DTO/dispatch 检查 | PASS |
| Core/Runtime all-targets Clippy，`-D warnings` | PASS |
| macOS 默认与 protocol-composite-clipboard 编译 | PASS |
| Windows helper，MSVC target + portable-check | PASS；编译检查，不是 Windows 原生执行 |
| RR01–RR30 修复门禁 | 30 checks / 0 unexpected |
| 门禁解析器 | 7 passed |
| Swift helper typecheck | PASS |
| 独立命名粘贴板 RTFD capture/native restore | PASS；保留原始附件，完整 text/HTML/1 PNG，Published 回执，稳定回读和匹配 SelfWrite |
| diff 空白检查、最终源码指纹一致性 | PASS |

所有有状态测试通过 `run-isolated-tests.mjs` 或独立随机测试目录运行。原生 probe 只使用 `tailsync-probe.*` 命名板。

初始并行 Core 再次出现已有 Iroh 多接口/短窗口波动。本轮把协议 round-trip 测试限定为明确回环地址，仍执行真实 QUIC + Noise，配对窗口与建连预算分开；完整 Core 随后通过。没有据此声称公网、relay、多接口选路或产品冷连接预算已通过实机验收。前次诊断与初次失败日志均保留。

## R6 必须继续保留的边界

- Windows 原生 clipboard sequence=0、delayed rendering、锁竞争、消费者接受的 bitmap/HTML/RTF：**UNKNOWN**。
- 实际 macOS App 菜单、键盘、VoiceOver、窗口告警呈现及真实 Word/RTFD 来源：**UNKNOWN**；Swift build/test 和命名板 probe 不能替代这些。
- Mac↔Windows 双设备、真实新复制/撤销/睡眠重连、包内 helper 和持久存储失败：**UNKNOWN**。
- 微信、Word、浏览器等目标应用的双图/位置/格式等价性，以及 URL-only 来源补全：**UNKNOWN / 完整来源门槛保持 BLOCKED**。
- 公网 direct/relay、真实多接口/代理/Tailscale 以及冷连接耗时：**UNKNOWN**。
- 当前仍是未提交候选，CI/发布证据不属于这份本地结论。整体公开发布继续 **NO-GO**，不得把 R1–R5 本地补修通过改写为 R6/R7 已完成。
