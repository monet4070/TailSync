# DeepSeek 审查核实与整改验收

日期：2026-10-06。范围：今天的 S0、O11-D1/D2、O01、O02、O06、上轮审查清理和 O10。**本轮不启动 O03，不提交代码，不启动/安装 App，不访问通用剪贴板。**

工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，分支 `codex/composite-clipboard`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。

审查前源码：`e9d431aff31a9ae2181cc2e8313ebf05d385c89516c79b2e2de5856daaaeb739`，770 文件。整改源码：`364fbe7819331b2e28f3eb2477e219f1a32a691dba3e4469fec58493bb984aec`，771 文件。本轮增量 22 个源码/测试文件，+376 / −144；文档单独封存。

## 1. 审查来源与采信边界

用户指定 DeepSeek、max。桌面 managed 子代理及消息重投没有识别任务，未产生有效审查；已改由独立 `codex exec` 进程执行，显式指定 `cline-pass/deepseek-v4.1-flash`、`model_reasoning_effort="max"`、正确工作树与 read-only sandbox。实际 argv、完整事件流、退出码和原始报告保存在新证据目录的 `reviewer/`。

[DeepSeek 原始报告](COMPOSITE-CLIPBOARD-DEEPSEEK-REVIEW-2026-10-06.md) 保留原文。它审查整改前的阶段实现；主代理在审查期间另行复现和修正部分问题。因此该报告不能被当作整改后代码已获第二轮独立认可。报告的发现、行号、优先级及“逐字节比对”声明也不能取代下面的实际核实与测试结果。

## 2. DeepSeek 发现的处理

| 原始项 | 核实与处理 | 用户效果/边界 |
|---|---|---|
| P1-1 / P2-1：完成时过期快照不存历史 | **不采纳为确定缺陷**。ADR-005 §12 第一行明确要求 worker 完成时核对当前 OS revision 与 wake generation；“历史保存后可以保留旧事件”限定已经开始保存的事件，没有承诺每次 OS 复制都入历史。没有删除完成时的过期 gate，也没有让迟到观察重新 supersede 新 intent。 | 保留“调度 revision 匹配＋完成时仍当前”的准入规则。采样/转换结束前已经被替换的观察可能不入历史；已经保存的旧事件可留历史，但不得重新自动同步。 |
| P2-2：接收失败复用本机复制提示 | **成立，已修复**。真实可靠 text/image 接收的两处存储失败改用 `record_receive_not_saved(source)`；Swift/React 与中英文增加 `receive_not_saved`。 | 提示明确是接收内容未保存，动作指向发送设备。保留本机采集失败的原提示；来源设备参与 warning 去重与 ACK。 |
| P2-3：三个无消费者发布接口 | **成立，已删除**。全仓实际代码无调用的 `ClipboardPort::publish_composite`、`publish_native`（含实现）和 `SyncEngine::publish_composite` 删除；测试专用 import 移入测试模块。 | 减少绕过 RestorePlan 的失效入口，没有为未来阶段留占位代码。实际计划发布、文本/图片 admitted 路径保留。 |
| P2-4：准入策略重复 | **维护问题成立；策略差异本身有意保留**。Windows 格式名称资格抽为 `native_policy::windows_publishable`，用于 RestorePlan、平台准入与 legacy storage。旧 CNS1 存储集合由 `legacy_storable` 显式表达。 | macOS 不透明字节仍通过 CNA1 保存。没有把 macOS opaque publication 的扩展权限赋给旧 CNS1 存储，也没有扩大 Windows 类型集合。helper 的名称→Win32 ID 映射仍是必要平台映射。 |
| P3-1：部分读取诊断与 UI 同类去重 | **不构成缺陷，保留**。durable capture fault 与用户提示是不同事实；warning 按 kind/peer 去重。 | 不新增同义气泡或技术状态标签。 |
| 测试可进一步参数化 | 未提供具体重复断言/测试位置；没有为形式统一改写测试。 | 新通知复用已有参数表与双语表；已有关键断言不放宽。 |

## 3. 主代理补充发现与修复

### R1：大载荷被自己的采集预算拒绝

macOS 成功保存后仍持有 240 MiB 采集 scratch reservation，`SharedEvent::encode_shared` 又为正文申请发送预算；固定 256 MiB 管线因此只剩约 16 MiB。合法、超过 16 MiB 的 PNG 复合内容在该接缝被拒绝，原广播函数只写日志。

新增共用 `composite::capture::prepare_broadcast`：消费稳定 snapshot 和 portable bundle；确认 history worker 已释放共享 reservation；在既有预算下完成编码与 Arc 转换；释放原件/转换临时数据后把预算缩到正文与 envelope-copy workspace。不可变 `CapturedBroadcast` 持有剩余 lease，直到广播准备结束；`SharedEvent` 继续独立持有自己的正文预算。两平台使用同一交接函数，没有增大 256 MiB 上限。SharedEvent 编码/准入失败现在也发已保存但未同步提示。

大 PNG 回归使用真实 PNG、结构/解码 gate 和实际 SharedEvent；未缩减的交接先复现 `temporary-pressure: composite pipeline memory budget exhausted`（exit 101），修复后检验正文 digest 保持、广播准入成功和预算完整释放。此测试不是原生系统剪贴板或双设备吞吐验收。真实并发压力仍可按既有总预算拒绝，不宣称所有负载均无压力。

### R2：发送前检查与实际入队之间仍有等待

两端原有检查后仍等待 Settings、设备发现、pool/trust 锁及满队列；每个 peer 的 task 也可能延迟运行，因此旧观察仍可在新复制或显式恢复之后进入发送队列。

共享 pool 先 reserve 实际有界 channel 的 capacity，**等待结束后**才执行当前采集 gate；gate 在短 engine lock 内核对 generation、OS revision 与 wake generation，并持有该 guard 穿过同步 `permit.send`。过期返回正常跳过，队列等待不持有 engine，取消时 permit 正常释放。平台路由/信任与原有错误提示保留。

确定性回归填满真实 Tokio channel，精确 poll 到 pending 后改变当前状态，再释放 capacity；旧实现仍入队（exit 101），修复后旧观察不入队、当前观察入队，且等待中可获取模拟 engine mutex。它验证入队等待接缝，不代替完整 live monitor、OS 快速复制或跨设备顺序验收。

### R3：HTML 已知依赖漏判

原 inventory 只检查 literal `url(`，漏掉 CSS `@import` 字符串形式、CSS escape/HTML entity 隐藏的 URL，以及 SVG `image/use` 的 `href/xlink:href`。这些内容可被误标 Complete 并通过默认原件恢复或 HTML 兼容恢复。

现在 extraction、历史菜单与 RestorePlan 使用同一保守谓词：CSS import、转义和实体缺少闭合证明时 withheld；非导航 `href/xlink:href` 按资源依赖处理。普通 literal 字体/颜色、无外部依赖的 style block 和导航链接保留。未引入 CSS renderer 或网络/文件 dereference；遇到无法证明闭合的 CSS 可能保守拒绝 HTML 恢复，原始归档不改写。

表驱动回归同时检查采集 completeness、菜单 inventory、portable 和 original RestorePlan，另有正常格式正例。先在 `@import` 被误标 Complete 处失败（exit 101），修复后通过。零引用的旧 `has_external_images` string-search helper 删除，避免与共用 inventory 形成另一套规则。

## 4. 回归维护、验证及证据

新增三个行为回归：大载荷预算交接、真实满队列 gate、CSS/SVG 依赖资格。接收提示回归扩展已有真实 SQLite 拒绝/可靠接收测试；React 与 Swift 扩展已有表。没有新增 ignore/skip，没有新增只复述实现的测试。

R1/R2 的 RED 在提取新共用接缝、保留旧预算持有/检查顺序时执行；之后修复同一接缝并接入两平台。它们不是完整旧 monitor 的端到端复现，旧路径的问题另由真实生产调用链确认。

四处问题均保留语义失败记录：`budget-red`、`queue-red`、`html-red`、`receive-warning-red-corrected`。中间编译错误（测试 ACK 名称、接收 worker 的 source ownership）及后续修正尝试也保留，不能算作预期 RED 或 PASS。`review-green` 过滤还匹配了已有 preview 测试，实际执行 15 项，不把它描述为仅执行三个新增测试。

最终矩阵和源码绑定结果见本节后续自动生成表及 `validation-results.json`。历史 S0/O11-D1/O11-D2/O01/O02/O06/cleanup/O10 八份 seal 已逐项复算。S0 的 Cargo manifest 必须按已有 `manifest-archive-location.json` 检查 `.txt` 原文字节；不把已记录的路径归档当作损坏，也未改写旧 seal。

证据目录：`artifacts/clipboard-improvement/2026-10-06-deepseek-review-remediation/`，包含完整源码基线、最终变更副本、incremental.patch、命令 argv/原始日志、每次尝试的退出码/源码指纹、原始审查报告、处理记录、历史 seal 复算及本轮封存。

**验收范围：本机自动部分。** 本轮没有新 App 包、真实 GUI、QQ/目标应用、Windows 原生、Mac↔Windows/双设备、hosted CI、物理满盘/断电或 O04 资源下载验收。它们继续是 UNKNOWN/后续门禁，Release **NO-GO**。整改完成后暂停，原计划下一项 O03。

### 最终验证结果

19 条最终命令（`*-final-v3`）全部 exit 0，运行前后源码指纹一致。全部实际尝试 32 条（含收尾文档 diff-check）；其中四条预期语义 RED、五条中间编译失败均保留，不能被最终 PASS 覆盖或当作门禁成功。

| 最终检查 | 结果 |
|---|---|
| Core enabled＋repair＋两项传输扩展，serial | 660 PASS，4 既有 ignored |
| Core integration | 11＋3 PASS |
| Runtime all-features＋Core composite | 27＋7＋11＝45 PASS |
| macOS Rust enabled | 118 PASS，3 既有 ignored；编译实际 monitor/adapter |
| Windows Rust on macOS host | 127 PASS，2 既有 ignored；不认证原生 Windows |
| Protocol all-features | 42 PASS |
| React History | 50 PASS；可见提示/按 ID ACK、隐藏不 ACK |
| Swift localization＋retry identity | 13 PASS；不是 GUI 验收 |
| Workspace all-targets/all-features Clippy | PASS，`-D warnings`，0 warning |
| production-features / canonical / contracts / native policy | PASS |
| capture visibility / TypeScript | PASS |
| workspace＋两平台 fmt / diff-check | PASS |

显式 Rust ignore 注解 11→11。Windows host 保留既有 `clipboard_file.rs` 中 Win32/InvalidData 未构造告警，文件本轮未修改；没有隐藏该告警。没有持久 schema、网络/本地契约或 helper 版本变更。判定：**LOCAL_COMPLETE（上述自动部分），Release NO-GO**。
