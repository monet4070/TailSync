# TailSync 复合剪贴板修复执行方案

日期：2026-10-04。本文保留制定时的执行方案与基线。2026-10-05 当前验收入口见
[单 Mac 验收报告](COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md)：R1–R6 本地验证与 R7 部分原生场景已执行，完整候选仍为 **NO-GO**。

本方案以独立核验后的真实缺陷为依据，覆盖构建、采集、转换、持久化、投递、系统发布、来源判定、恢复交互和验收。本文定义改动与验收条件，不把建议、历史 PASS 或待验证行为写成修复完成。制定方案时只新增本文；后续实现和验证按上述当前报告及对应源码指纹评估。

## 1. 执行对象、依据与完成标准

### 1.1 固定执行对象

| 项目 | 固定值 |
|---|---|
| 修复工作树 | `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35` |
| 分支 / HEAD | `codex/composite-clipboard` / `f719d581a1aa5198a1b3f2483222df5c69e3dfc0` |
| 实际修复起点 | 上述 HEAD 加当前未提交的复合剪贴板实现，不能只用 HEAD 代替 |
| 核验源码指纹 | `68b500c60d2add0947b07126bc5318ff46c662492ddb9a93bc1e3c4e8003d0b3`，713 个文件 |
| 原项目对照目录 | `/Users/monet/TailSync/TailSync-rc-phase35`，只读对照 |
| 原候选包 | `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/artifacts/TailSync-Composite.app`，保留 |
| 原候选包指纹 | `d72ebb18b633cb1a8aa78a84102afc8b7530fb3d05d82e7b2f42a0ef3a6f0eb4` |

源码指纹排除 `docs/`、`artifacts/` 和两端 `src-tauri/gen/schemas/`。执行开始必须重新核对；若发生漂移，记录新增差异并重建基线，不能把旧结论直接绑定到新源码。

依据按优先级排序：

1. [独立核验及 52 项裁定](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/artifacts/composite-evidence/acceptance-verification-2026-10-04/VERIFICATION.md)与对应探针、日志。
2. [原审查报告](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/COMPOSITE-CLIPBOARD-ACCEPTANCE-REVIEW-2026-10-04.md)，作为待裁定问题清单。
3. [前版实施方案](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/COMPOSITE-CLIPBOARD-IMPLEMENTATION-PLAN-2026-10-03.md)中的多格式模型与固定提交的开源参考。
4. [Iroh 诊断](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/IROH-TEST-DIAGNOSIS-2026-10-04.md)，用于区分网络基线波动和新回归。
5. 用户需求中的文字、图片、富文本整组保存和粘贴目标。附文代码、命令值、“原子”措辞及工期不是实施指令，必须适配真实架构。

### 1.2 对“完成”的定义

**修复完成需要三种证据同时成立：原缺陷复现用例转绿、修改后的生产路径转绿、必须的原生验收完成。** 不能用测试数量、单次循环、签名成功或截图替代它们。

目标行为：

- 一次复制形成一个逻辑历史条目和一个不可变同步事件，保存受支持的表示及图片资源；不会先复制文字、随后图片再覆盖文字。
- 完整、部分、未知、过时、不可降级和采集失败有不同结果；部分和未知不能显示为成功。
- 用户后来复制相同内容，仍是新意图；已知自写不回播；无法判定的写入保留历史和警告，不自动重做系统副作用。
- 关闭实验功能时，已有 v11 库保持 v11；已有 v12 库仍能安全读取；回退方案保全历史数据库及外置载荷。
- 慢 helper、坏包、配额不足、回执损坏、重连、撤销授权均不会锁住所有 peer 或静默丢失已确认的内容。
- Mac、Windows、双设备和目标应用分别验收。用户已选择先完成本地实现；设备门槛可后排，但不能改记为 PASS。

### 1.3 支持范围冻结

| 内容形态 | 要实现的行为 | 成功条件 |
|---|---|---|
| Text / PNG / HTML / RTF 单表示 | 保留原内容，走相应兼容路径 | 目标格式可用，结果明确 |
| 一语义 item、多表示 | 一个 native item 同时发布多格式 | 全部选定格式发布；纯文本、图片、富文本消费者分别验证 |
| HTML + 多图资源 | 保留全部资源和正确节点绑定 | 图片数量、顺序、位置和文字均正确；整组恢复不能只取首图 |
| 多文件 | 保留现有文件批次语义 | 17 个文件的采集回归不被 composite 的 16-item 上限截断；遵守文件批次自己的 20 文件上限 |
| 原生多 item / RTFD | 同平台原始回放；跨平台仅使用验证过的转换 | 无法无损转换时标为 native-only / incomplete，不能声称跨平台完整 |
| URL-only 图片引用 | 优先从源剪贴板已有字节补全 | 缺图片字节时显式 incomplete；目标用例若要求此来源完整，须完成资源补全后才能验收 |
| 旧端不支持 composite | 每个事件只选一个旧表示 | 单图有明确图片则优先图片；否则文本；多图/RTF-only 不可可靠降级时告知限制 |

RTFD 附件转换、URL-only 图片补全不是通过容器自动获得的能力。Chrome/Word/Notes 的承诺用例逐个登记，不能把未实现的来源默默删掉以通过验收。微信是否接受图文混合，由实际版本和原生输入框决定；没有证据时保留 UNKNOWN。

## 2. 必须先冻结的十条契约

### K1：区分三种摘要

1. `WireDigest`：保持现有 envelope / bundle 编码与 ACK 摘要语义，覆盖原始线协议字节。不得为防回音排序 representations 后改变它。
2. `OriginFingerprintV2`：仅用于本机来源判定，规范化系统可观测的发布集合。表示枚举顺序不影响它；PNG 的无关编码元数据不影响像素身份；文字空白、RTF 内容及 native item 边界必须保留。
3. `EventReceiptDigest`：保持现有 legacy/composite 回执各自的摘要算法。legacy 的 command、时间字节序和 composite 的时间字节序不同，导入不得统一重算。

原生发布格式的优先顺序与指纹计算顺序分开。不能为了顺序无关指纹改变 Windows 消费者优先选择的格式。

### K2：将原生结果从字符串 Err 升级为有副作用语义的结果

| 结果 | 证据 | 自动重试 | 用户结果 |
|---|---|---|---|
| `NoEffect` | 可证明尚未进入 clear/set/write 阶段 | 可以，在固定 deadline 内 | 临时失败 / 重试中 |
| `Published` | helper 回执完整、操作 token 正确、所选格式发布完成 | 不重做，只补持久化/ACK | 已复制 / 已同步 |
| `Partial` | 已执行副作用，格式组不完整或 clear 后未完整填入 | 不自动重做 | 部分发布，提供历史恢复 |
| `Unknown` | 已可能执行副作用，回执丢失/损坏或进程异常 | 不自动重做 | 结果未知，提供历史恢复 |
| `Superseded` | 已观测到 generation/revision 变化，未执行写入 | 不重做这次旧事件 | 已保存历史，保留较新复制 |

Windows sequence=0 是 `RevisionUnavailable`，不能伪装为 `Superseded`。没有 expected revision 时，接收端不能擅自覆盖未知当前内容。

### K3：来源分类先于内容去重

`稳定快照 → 匹配本地操作和 native revision → 来源分类 → 历史与广播策略`。

- `SelfWrite` 必须对应已知结果，匹配同一 clipboard epoch 下的 revision 和实际规范化发布指纹；token 为关联证据，不能单独授权抑制。
- 新 revision 在操作关闭后的观测中出现，即使内容相同或带旧 token，也归类 `External`，除非匹配另一条已知操作。
- 已知 revision/token 对应的指纹不一致：`UnknownMismatch`，保留诊断，禁止直接当 External 自动回播。
- 不稳定或未通过验证的快照不能进入成功广播路径。

来源结果必须传到现有 `shadow_check` / `image_shadow_check` 与 Core 门禁。已确认 External 不能再次被 sticky hash 拦住；SelfWrite 静默消费；Unknown 走本方案的保守处理。新 native revision 一旦被观测就推进/保留本地意图 generation，不等待慢正文读取结束；否则新的复制读取失败时，旧远端内容仍可能覆盖它。

### K4：未知副作用必须有关闭边界

操作状态：`Prepared → Claimed → InFlight → KnownComplete / KnownPartial / Unknown / NoEffect → Closed`。

先登记 intent，后启动 helper；intent 不是成功回执。写入活动期间的快照按 revision 有界暂存，不抢先判为 External。

回执丢失后，只有在 helper 被终止、回收、管道线程结束并完成稳定探测后，才能建立 `resolution_revision`。该边界对应的内容仍归 `Unknown`，不凭 token/hash 补造成功；边界建立后出现的新 revision 按新用户意图处理。探测失败则保持未解决状态，限制自动发布并显示故障，不能靠 60 秒 TTL 把它放回 External。

kill/reap证明helper不再发新请求，不能独立证明OS服务没有待完成请求。原生验收必须检查延迟provider/系统服务竞争；无法形成可靠关闭边界的路径保持Unknown，不用固定sleep补造“写入已停止”。

未知期间无法同时保证“每次用户复制都自动广播”和“所有自写都无回音”。本方案选择保全可恢复内容、阻止不确定自动回播，并提供显式恢复。这一取舍必须进入产品文档与故障验收。

### K5：系统副作用之前，内容与 claim 已持久化

远端事件顺序：

`Authenticated → Admitted → Validated → HistoryPreparedDurable → EffectClaimDurable → NativeEffect → OutcomeDurable → Completed → FinalAck`。

收到旧 claim 无结果时，恢复历史/结果元数据，返回 Unknown，不重新执行系统写入。原生完成而终态持久化失败时，也不重新发布。已有文件接收“落盘成功后 ACK、剪贴板激活失败不使文件转移回退”的契约继续保留。

这里的durable不是“调用了commit/rename”即成立：effect journal显式使用SQLite WAL + `synchronous=FULL`，检查提交结果；history/parts在claim前满足相同恢复承诺；外置文件及兼容JSON使用现有private_fs的原子替换并核对文件/父目录的持久化步骤。Windows相应落盘原语要独立验证。进程崩溃与断电/文件系统持久化分别标证据，不能用前者证明后者。

### K6：最终 ACK 只陈述已经持久化的事实

- 分片 ACK 只表示分片接收；最后一片的最终结果必须等待历史、claim、终态和兼容回执提交。
- Composite 沿用 `Applied / Superseded / ApplyUnknown / FailedPartial / StoredIncomplete`。
- 协议结构/资源验证合法但目标准备超限或不支持时，保存原完整内容，以StoredIncomplete及明确原因终止，不执行native effect；该结果表示目标未完整应用，不能把合法可恢复内容误判成坏包。真正非法包拒绝，不进入成功历史/成功ACK。
- Legacy 不修改现有 EventAck 帧格式。明确 Unknown/Partial 的接收使用终态错误及本地警告，不能冒充 Applied；重复事件返回同一持久化终态。
- 忙/准入压力尚未产生副作用时保留为可重试传输失败；格式不支持和永久拒绝必须可见。

### K7：锁只保护短状态读写

不得持有 `SyncEngine`、settings、全局 finish 锁等待 helper、DB、PNG 转换、管道 join 或 async permit。

原生发布 permit 全局为 1，因为系统只有一个目标剪贴板。校验、历史准备和不相同事件的去重不需要因此全局串行。

### K8：功能、健康状态和数据库演进分开

`compiled_feature && runtime_enabled && helper_healthy` 决定是否宣告并启用 composite。v12 读取能力与是否允许 v11→v12 分开。

关闭态承接已有v12的迁移/恢复时，目的库必须至少是源库schema，属于保全已有数据；不能为保持新目录v11而丢掉composite。没有承接v12数据的新库仍保持v11。

运行中 helper 失效时关闭新采集/新协商能力，现有已协商会话的 composite 输入在无副作用阶段给出明确失败；重新握手后反映能力变化。Mac headless legacy 读取也依赖同一 helper，不能把“退回 legacy”当作缺 helper 时必定可用的恢复。

### K9：一个不可变事件，一个固定截止

事件 ID、原始 timestamp、peer 选定表示和 wire digest 在重试中固定。总截止是 enqueue TTL 与原 timestamp 有效期的较早者；重试不能重新计时。

同一 peer/event ID 不允许在可能发生副作用后改成不同 fallback 内容。撤销授权、队列过期、generation 更新要在发布前再次检查。

### K10：证据不能跨层替代

源码确认、单元测试、平台 adapter、包内 helper、完整 App、Windows 原生、双设备、目标应用分别记录。来源不完整时不计完整成功。例外和豁免必须明确说明影响、证据及替代验证，不允许用“基线已有”自动放行。

## 3. 目标架构与改动边界

保留 helper 进程隔离，保留现有认证/文件/网络栈，增加三个深边界，避免在每条错误分支打补丁。

```text
平台监听 / 历史恢复 / 网络接收
           │
           ▼
ClipboardCoordinator（来源、generation、发布串行、typed outcome）
           │                    │
           ▼                    ▼
异步 ClipboardPort         EffectJournal（持久 claim / 终态）
           │                    │
           ▼                    ▼
HelperSupervisor           HistoryDB（完整内容 / parts / 墓碑）
           │
           ▼
进程隔离的 macOS / Windows helper

网络字节 → 有界准入 → ValidatedComposite → 原生准备 / 历史准备
                           │
                           └→ WireDigest 与 OriginFingerprint 分离
```

| 责任 | 当前主要位置 | 修复方向 |
|---|---|---|
| helper 调用、路径、来源 | [共享平台 composite](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/platform-clipboard-composite.rs)、[Mac legacy helper](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/src-tauri/src/clipboard_file.rs) | 共用 supervisor/resolver；保留平台路径与权限规则 |
| 平台端口与引擎 | [SyncPlatform](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/sync.rs)、[剪贴板用例](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/sync/clipboard.rs) | 增加异步端口，准备与完成阶段分离 |
| 原生操作 | [Swift helper](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/src-tauri/clipboard-helper.swift)、[Windows helper](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/tools/clipboard-helper/src/main.rs) | 预分配、预校验、结果回执、文件先分类、缩短 Windows 打开会话 |
| 协议与转换 | [协议 composite](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/tailsync-protocol/src/composite.rs)、[Core composite](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/composite/mod.rs) | 验证一次、有限工作量、独立本地指纹 |
| 装配与投递 | [装配](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/composite/chunks.rs)、[接收](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/peer/composite_receiver.rs)、[投递执行](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/peer/delivery/executor.rs) | 身份配额、固定 deadline、终态一致、编码缓存 |
| 数据生命周期 | [DB open](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/db/open.rs)、[DB composite](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/db/composite.rs) | 迁移政策、完整备份、parts 事务与配额、恢复保护 |
| 恢复/API | [共享 history runtime](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/tailsync-runtime/src/history.rs)、双端 history routes/commands | 服务端 guard、typed outcome、全部入口同契约 |
| UI | [Mac HistoryRow](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/swift-ui/Sources/TailSync/Views/HistoryRow.swift)、[Windows HistoryItem](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/windows/src/pages/history/HistoryItem.tsx) | 真实可恢复结果、格式选择、键盘/无障碍、两种语言 |

拟新增模块均为计划：Core `clipboard/` 下的 coordinator/port/effect-journal，shared 的 helper supervisor，以及 origin fingerprint 实现；具体目录可随现有模块边界调整，责任不能重复分散。已有文件继续采用 canonical include，注册到跨端检查器。

## 4. 执行顺序与阶段门槛

关键路径：`R0 → R1 → R2 → R3 → R4 → R5 → R6 → R7`。每阶段形成独立可审阅改动，不一次性提交一个大改造。R5 的 UI 准备与文档可以提前，但不能用临时假结果替代 R3/R4 的真实契约。

### R0：建立可追踪基线与失败回归

**目的：先证明原问题，再约束修复。**

1. 保存 dirty 状态、全部受审源码（含 untracked）、包指纹、原探针和环境版本。复制到独立修复验证 snapshot，不覆盖原项目或旧候选包。
2. 将核验探针拆成生产 seam 回归：默认脚本、v11禁用打开、写后回执丢失、clear后超时、顺序变换、Rejected warning、17-file shape、两图 PNG选择、native-only API、JSON损坏、异步计时。
3. 引入仅 test/probe 可用的确定性故障点；生产构建不能凭环境变量打开写后 sleep、general board 替换或绕过授权。
4. 为每个用例记录修复前的失败断言。证实缺陷的 probe exit0 不算产品 PASS。
5. 冻结 K1—K10 与支持范围，创建长期规格和新的 ADR，核对当前仅有 ADR001—003 后选用未占用编号；不声称已有 ADR004。

**Gate R0：** 关键阻断均有可重复失败或可执行源码边界断言；源码与证据一一对应；后续所有阶段有恢复点。

### R1：修复默认构建、能力开关和迁移/回退

**R1-01 / A：构建参数。** 修复 `macos/build-mac.sh` 在 Bash3.2 + `set -u` 下的空 feature 数组；使用 Bash3.2 支持的 positional arguments 或安全展开。验证 0/1 × native/universal 分支；stub 回归检查传参，至少默认和开启各完成一次真实构建。输出新路径，例如 `artifacts/composite-repair/<run-id>/TailSync-Default.app` 和 `TailSync-Composite-Repaired.app`。

**R1-02 / B：数据库政策。** 增加显式 `MigrationPolicy::LegacyOnly / CompositeAllowed`，在 `db/open.rs`、`db/schema.rs`、`db/migrations.rs` 和 degraded/reopen 路径统一使用。不能只 gate migration，仍让 initialize 创建 composite 表。

| 已有库 | 开关/健康状态 | 打开后要求 |
|---|---|---|
| 新库 / v11 | feature off | 最高到 v11，无 composite DDL |
| 新库 / v11 | feature on，runtime off | 同上 |
| 新库 / v11 | feature on，helper unhealthy | 保持 v11；功能不可用可见 |
| 新库 / v11 | feature on，runtime on，helper healthy | 完整备份成功后升 v12 |
| v12 | 任意关闭状态 | 保持 v12，读历史；禁止自动降到 v11，关闭 composite 新采集/新投递 |
| 新目的库承接既有v12历史 | 任意关闭状态 | 依据源库创建v12并完整迁移，不解释为开启新采集；回退承诺按v12规则处理 |
| 高于支持读上限 | 任意 | 明确拒绝，不能尝试重建 |

最大可读版本与允许迁移的目标版本是两个值。policy同时携带恢复源要求的最小schema，避免把已有数据保全与新功能启用混为一谈。v11的degraded/reopen/迁移查询不得无条件访问不存在的`history_parts/history_composite_events`。测试 fixture 显式申请 composite policy，不能让默认单元测试因隐式升级继续掩盖生产政策。

**R1-03：完整回退包。** 升级前备份由 SQLite 一致快照、所有存活外置图片/文件载荷、权限与摘要清单组成。冻结写入/删除并校验引用期间完成快照；实际实现采用 SQLite backup 或受控 checkpoint 后快照，不能运行中裸拷 DB/WAL 三个文件。备份失败/空间不足时停止升级。

外置文件采用独立复制或证明独立生命周期的 clone；不能只保留会被 GC 删除的路径，也不能用可能被原地修改的共享 hardlink 冒充安全备份。快照加入 GC/配额保护与独立容量预算，只有明确退休快照后释放。

回退步骤：停止候选写入并验证 quiescence → 另存升级后库和载荷 → 校验完整升级前快照 → 恢复到新的目录 → 用旧二进制打开并逐项读取正文/图片/文件/收藏 → 再切换路径。升级后的 composite 历史另行归档，旧版本无法读取的部分不能承诺无损直接导入。**不得随历史快照回退 identity、trust 或 effect receipts，避免旧回执丢失导致再执行。**

**R1-04 / D-1,H-7,H-8：健康与路径。** 启动先做无正文 helper version/self-check，再决定 composite capability 和迁移 policy。release 只用包内 helper；debug 使用构建脚本实际产物位置，不搜索 PATH。两端 resolver 共用规范。缺失、无执行权限、版本不符分别报告。

启动顺序固定为：无正文helper自检 → 冻结打开policy → 打开history/journal并恢复pending → 对外监听/协商。不能先启动接收线程，再异步补回执导入。只有journal恢复健康且history准备可持久化时才开放自动系统发布。

**Gate R1：** 默认脚本真实构建成功；四种 policy fixture 与 v12关闭态通过；完整回退后旧版本可读所有测试载荷；helper 不可用有明确状态，不能虚假宣告能力。

### R2：有界校验、转换和来源指纹

**R2-01 / C,G-9：校验一次。** 引入只能由校验工厂构造的不可变 `ValidatedComposite`，私有字段持有原始合法 wire、解析模型、预算信息和转换缓存。来自网络/DB/IPC 的未可信字节都要校验；共享已验证对象才可复用，不提供任意 unchecked 构造。

HTML UTF-8 在每表示校验一次；绑定排序、范围与边界使用已验证结果。不能在每 binding 对完整 HTML 重跑 `from_utf8`。PNG 在有配额的 worker 中每个独立资源完整解码一次，复用像素摘要及转换结果。接收终态 ACK 从已验证 wire 计算 digest，不在 Tokio worker 上重新 encode/compress。

保留合法表示顺序及现有线协议编码。不得为了“canonical”而使旧合法包被 decoder 拒绝。bindings 数量由 manifest 及实际预算限制，不能随意降为 512 而不声明兼容性变化。

**R2-02：预估物化长度。** 在复制/分配 data URI HTML 前，checked arithmetic 计算替换后的 UTF-8 字节总数，应用同一 native HTML 上限。当前 2MiB HTML/5383 bindings 的包可通过 wire 却无法物化，须在发布前返回明确不可发布结果并保存历史，不能等 clear 后才发现。

结构/资源完整性验证与`NativePrepared`是两道独立门槛：前者成功后才允许安全入历史，后者失败不应让接收器直接跳过可恢复历史。重构现有`validate_for_publish`调用顺序时必须覆盖这条分支。

**R2-03 / H-20：独立来源指纹。** 按 MIME 与 native item 边界排序计算本地指纹；固定像素、色彩和 alpha 规则；忽略自身来源格式与明确识别的系统合成冗余表示。HTML 绑定按资源内容身份还原比较，不以临时 resource ID 比较；未使用资源不能使系统回读凭空不匹配。

发布前指纹使用“目标平台实际会发布的集合”，回读使用对应同一投影；不得对源包和目标 OS 各算不同模型。遇到无法证明等价的转换，保持 UnknownMismatch，不能用宽松字符串清洗掩盖丢失。RTF 不任意重排或剥离控制字；不做近似语义匹配来吞用户复制。

**R2-04 / D-4,G-9：多层配额。** 按认证公钥指纹统一 admission，不按连接、显示名或可更改 device name。

| 预算 | 首版规则 | 说明 |
|---|---|---|
| assembly | 同一认证 peer 1，全局4 | 跨重连/多连接共享；结束/拒绝/断开释放 |
| wire | 保留现有 wire rate budget | 每到达片计费；不能代替 raw CPU 预算 |
| raw CPU budget | 首版64MiB burst、64MiB/60s refill | 设计起始值，按一次新装配的声明 raw 预留；需依据测量调优 |
| decode | 全局2，同一 peer 1 | 禁止 unbounded spawn_blocking |
| pipeline memory | 首版256MiB显式预留预算 | 设计上限，非现有 RSS 测量；包含 wire/raw、像素、native准备和重试缓存 |
| 格式硬上限 | 沿用现有 body、manifest、native、RGBA、分片约束 | 32MiB级body、64KiB manifest、40MiB native、32MiB RGBA、512KiB×64 分片；使用真实常量和 framing 扣减 |

相同 assembly 的重复片不重复预留 raw；已关闭后重新装配仍计 decode 成本，不能靠换连接绕过。预算必须覆盖 PNG 工作区和转换临时对象；实际 decoder 使用分配限额。进入下一阶段采用非阻塞预留/转移，不能持有一批预算等待另一批预算造成死锁。无法安全预估时在副作用前拒绝。

**Gate R2：** 原合法样本仍可 decode；顺序变换不回音；2MiB物化超限可见；1MiB/5383 bindings 成功；一 peer 四连接不能占四个槽；配额释放与实际分配受限有断言。

### R3：异步发布、helper 生命周期与未知结果

**R3-01 / F,G-13,H-18：异步端口。** 新增 object-safe 异步 `ClipboardPort`（返回 boxed Send future），以 `Arc` 持有端口。`SyncEngine` 只准备授权/generation/revision上下文和接收完成结果，不在 engine guard 内执行系统 I/O。平台 supervisor 在受控 worker 管理子进程。

先接通“单事件 composite 接收→真实 helper→typed outcome”纵向切片，再迁入 legacy text/image、native-only历史恢复、单格式恢复、文件历史激活和直接 API。不得同时留下会绕过来源登记的新旧写入路径。不能只给原同步函数包 `spawn_blocking`，仍持 engine 锁 5 秒。

**R3-02 / H-5,H-6,H-7：Supervisor。** 统一路径、启动、stdin/stdout/stderr、deadline、取消、kill/reap 和线程 join。读超时返回 CaptureTimeout；写超时依据是否可能启动副作用返回 NoEffect/Unknown。无法证明未写时一律 Unknown。

stdin writer 必须回收，stdout 有 native 长度限制，stderr 捕获最多4KiB并屏蔽正文/路径；记录 command、operation ID、退出码、耗时、错误类别即可。提前失败、管道拒写、解码失败也要进入同一 cleanup guard。只做 async timeout 丢掉 JoinHandle 不足以终止 helper。

取消必须把操作带到 quiescent 状态，不允许旧 helper 在 UI 显示取消后继续覆盖新内容。读取和发布有各自的队列/预算，发布 permit 全局1；helper健康自检不读写 general clipboard。

**R3-03 / E,H-19：操作登记与来源闭合。** intent 在 helper 前登记，绑定 operation token、clipboard epoch、expected revision、目标指纹及 deadline。KnownComplete/KnownPartial 由进程内产生的回执确定；Partial 的实际发布集合必须在匹配 revision 的稳定回读中确认，不能读不到便回退预期整组指纹后宣称自写。

活动/未知条目不能被 128条/60秒缓存驱逐。旧缓存仅管理已确认、monitor 已消费的条目。活动条目有单独硬限额，满时阻止新发布并告知原因；重启由 effect metadata 保守恢复，不能因 tracker 为空重播仍残留的自写。epoch 包含系统启动/session 边界，处理 sequence wrap 和回绕；不跨 epoch 比较裸数字。

**R3-04：原生写入安全。** 所有解码、编码、CF_HTML、内存分配在 clear 前完成；expected revision 在副作用前再校验。Mac `clearContents()` 是先清空的独立步骤，返回 change count；`writeObjects` 仍可能失败，不能称原子事务。[Apple clearContents](https://developer.apple.com/documentation/appkit/nspasteboard/clearcontents()), [Apple writeObjects](https://developer.apple.com/documentation/appkit/nspasteboard/writeobjects(_:))。

Mac revision 检查不是跨进程 CAS；必须检测并发修改并保守报告，不能承诺消除 API 不提供的所有 TOCTOU。Windows 用非空 owner window 打开并预分配 HGLOBAL，成功 SetClipboardData 后所有权交系统，不重复释放。[OpenClipboard](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard), [SetClipboardData](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setclipboarddata)。

clear 后0格式成功不能伪装成 FailedNoEffect。Partial/Unknown 后不自动清空、不自动恢复之前内容、不自动再次写入，以免覆盖用户新复制。

**Gate R3：** 两个原超时探针通过新断言；顺序反转仍 SelfWrite；helper被杀/回收后无晚写；相同内容的新复制判External；原250ms同步阻塞探针改为异步后10ms timer不再随250ms调用一起结束；所有生产发布入口通过同一协调器。

### R4：持久回执、接收幂等与投递失败

**R4-01 / G-10,G-14：独立 EffectJournal。** 采用 identity/settings 私有根中的独立 SQLite 元数据库，独立 schema1，不依赖 history迁移/用户删除。主历史库保持条件 v12；不为修复回执随意引入 history v13。

该库只保存来源、事件身份、immutable digest、状态、终态、原timestamp、operation token/epoch及可恢复历史引用，不保存正文或绝对文件路径。按现有 private_fs 管理库/WAL/SHM权限。event key 绑定认证来源与消息ID；同ID不同digest拒绝。

接收先幂等保存完整加密历史和 parts，取得可恢复引用，再提交 claim，再执行 native effect。history与独立journal不声称跨库原子：prepared但未claimed可安全继续；claimed无结果视Unknown。GC不得在pending期间移除payload；用户显式删除可断开history引用，但保留防重放墓碑。

**兼容桥必须先完成，不能直接弃用两个 JSON 回执：**

- 启动一次性严格导入 `remote-event-receipts-v1.json` 和 `remote-composite-receipts-v1.json`，保留原hash和状态；导入标记在新journal提交后才完成。
- 导入旧claim无结果→Unknown；legacy `received=true`仅证明旧历史接收完成，不能推断NativeApplied。
- 在允许旧二进制回退的期间，继续写兼容JSON：**SQLite claim + compatible JSON claim均持久化后才写系统；SQLite终态 + compatible JSON终态均提交后才发最终确认。** 合并可合并的终态更新，Lookup使用受锁保护的内存索引，避免每次扫描文件。
- 旧 JSON 不增加 `deny_unknown_fields` 不认识的新字段。新数据库保留精确状态，旧字段做保守映射；文档说明旧协议不能表达所有新状态。
- 单实例 lease、journal与镜像写入锁统一，防止两个daemon交替写。history位置切换不移动/回退effect root。
- 活跃claim、终态和墓碑保留到原timestamp已不能通过接收校验；未来偏移合法事件可能占10分钟。journal持久化`replay_floor_ms`，回收前推进floor，接收用不低于floor的时间基准校验，防止GC后时钟回退重新放行原事件。发现时钟异常显式报告，不默默放宽窗口；兼容镜像本身不赋予旧二进制新的时钟防护，回退验收需注明这项旧版本边界。

**损坏恢复：** 新journal完整且JSON坏→从权威ledger重建兼容镜像；journal坏且完整镜像覆盖所有已开始副作用→恢复为保守状态，模糊结果归Unknown；两者都坏/覆盖无法证明→保留损坏文件、禁止自动native写入、可继续展示已有历史并给出明确故障。禁止删空重建后恢复自动发布，也禁止从旧 LKG 任意恢复并忽略新claim。

启动对journal做完整性及状态检查；修复只从可证明覆盖所有claim的健康证据恢复。恢复期间不能把“历史有内容”误当“没有发生原生副作用”。此方案不承诺在所有持久元数据同时物理损坏后自动还原真实系统结果。

**R4-02 / G-13：短去重锁。** 用有界 `(authenticated_source,event_id)` 操作锁代替全局 FINISH_LOCK。读取终态直接返回；同事件跨session串行，不同事件可以并行校验/存储准备；写系统仍使用全局publish permit1。锁表回收不得移除被持有/有waiter的项。

同时修改legacy `peer/event_receiver.rs` 的错误出口：现有任何write Err都执行`Operation::Remove`并允许重试。新实现只有证明NoEffect才解除可重试claim；Unknown/Partial保留claim和历史，固定终态。不能只修composite接收而让同一慢helper在legacy路径重复写入。

**R4-03 / D-3,D-4：装配准入。** 保留当前同连接顺序分片契约；A0→B0不是合法预抢占，不能看到新message ID就丢旧assembly并重置TTL。重复首片/重连按固定事件身份处理；断开释放内存和预算。临时global/per-peer压力关闭或明确失败传输、不给永久Rejected，依赖现有Transport重试；非法内容仍永久拒绝并警告。

**R4-04 / D-5,G-7,G-8,G-12：发送端。** 把 Rejected 接入永久失败提示。每个事件/选定编码只encode/compress一次，共享不可变chunks；缓存计入R2预算，过期/终态/撤销后释放。所有分片发出前检查统一deadline，不刷新timestamp。RTF-only旧端无安全plain-text转换时明确UnsupportedFallback；不使用正则剥RTF造出错误文字。

**R4-05：授权与新复制。** 收到首片记generation；进入permit后、启动helper前重查授权与native revision。新用户复制/历史恢复使旧事件只入历史并Superseded。撤销已运行操作时取消helper并等待quiescence；若副作用已可能发生，记Unknown/Partial，不能承诺撤销能撤回已有系统写入。

首版沿用现有共享generation优先级，不增加按跨机器wall timestamp比较“最新”的机制。一个远端发布已改变目标剪贴板时，仍基于旧generation装配的其他事件只入历史；同peer正常串行的下一事件在新generation下开始。跨peer同时到达的选择以本地协调器的确定性顺序为准，必须测试并写入规格，不能因改锁而改变为随机覆盖。

**Gate R4：** 每个 crash point 的重复事件最多一次native attempt、一条逻辑历史、同一终态；坏journal不会再执行；新连接/多连接不绕过配额；临时压力可恢复；Rejected/过期/不可降级可见；重试encode调用次数=1。

### R5：采集、存储、恢复 API 与 UI

**R5-01 / D-1,D-2,H-9：采集状态机。** helper在应用composite数量上限前做安全metadata分类，真正file selection优先交文件流水线。fileURL+富文本混合形态不能盲目当纯文件或扁平化；单独返回形态及限制。

采集结果包括 Empty/Stable/Unstable/Busy/Timeout/TooLarge/UnsupportedShape/FormatFailures。按revision有限重试（首版200ms、800ms、2s，后续测量调优）；新revision取消旧重试。只有历史/可证明fallback完成，或已持久化终态故障并显式告知，才acknowledge observed。无法读取的正文不能宣称已经保存，保存的是故障元数据。

Mac缺同一个helper时保持ClipboardUnavailable，不虚构legacy可用。只有独立legacy读写健康检查通过才降级；降级导致图片缺失必须显示PartialCapture，不能当完整文本成功。

**R5-02 / H-2,H-1,H-4：parts生命周期。** 用RAII pending-part guard管理新写载荷：任何`?`、ENOSPC、commit失败都进入按活引用清理。共享已存资源不能删除；成功事务后解除guard。重启sweeper与回退快照pin都参与活引用判定。移除已被替代的死trim函数，保留实际物理配额逻辑；分页使用批量parts查询避免每条一次SQL。

加入收藏、并发删除、去重、降级存储重开与迁移回归：涉及composite的迁移必须带parts和事件映射；迁移失败保留原DB，不能丢掉已ACK的degraded历史。数据库不可用不发“已完整应用”的终态。

**R5-03 / G-2,G-3,G-4：恢复服务端守卫。**

- `native_only`在任何单格式选择之前验证平台和形态；不让`format=text`绕过。若要导出可用摘要，另设显式ExportSummary，不冒充原格式回放。
- 显式PNG：优先真实独立PNG；只有唯一图片资源时可使用它；多资源且无独立PNG返回NoSingleImage并隐藏该选项。若需要逐图复制，使用独立resource ID的显式动作，不能静默取first。
- 整组HTML恢复保留所有绑定资源；单格式HTML也保留它所需的绑定，未使用资源按模型处理。
- 返回typed `Published/Partial/Unknown/Superseded/Unsupported`。Partial与Unknown有操作ID和可恢复历史引用；不能只返回通用英文Err后显示成功。
- 双端HTTP routes、Tauri commands与快捷键共用runtime用例，不把关键限制只写在菜单隐藏逻辑里。

**R5-04 / G-1,G-5,G-6,H-15,H-16：可用UI。** Mac菜单显式名称、格式动作、键盘入口接入既有HistoryKeyboardNavigation；Windows使用现有按钮样式和焦点规范。菜单选项来自服务器实际可恢复能力。条目区分“表示数”与“图片数”，native-only不误标为多种可跨平台格式。

稳定错误码映射中文/英文Loc，不透传Rust英文。仅KnownPublished显示“已复制”；Partial/Unknown解释“已保存历史、发布未完整/结果未知”，允许用户明确重新复制。一次手动恢复创建新的本地operation，不重新开放旧remote claim。

警告使用有界结构化队列与历史/投递终态关联，保留旧latest字段兼容；按warning ID确认已展示，不能一个window清掉另一个未展示的失败。队列溢出有计数/聚合提示，永久结果仍可从关联记录查询，不只存一条易覆盖的字符串。

**R5-05 / H-3,H-17：契约与预览。** 明确logical size、external physical size、preview binary size各自语义；preview传输不复用伪原始payload DTO。纠正CompositeInfo注释，保留已有PreviewKind公共导出，生成Rust/Swift/TS/JS fixture并做缺字段/未知enum检查。

保持local contract schema1的兼容添加策略；若现有客户端严格拒绝扩展结果，新增版本化响应或能力字段并提供旧形状，不能直接塞字段。不要只因修了几条route把全局supports_stable_errors改true。

**R5-06：目标来源的资源闭合。** 对Chrome/Word/Notes实际快照记录已有HTML、PNG、RTF/RTFD附件的来源关系（证据不存私人正文）。有可信绑定才将字节资源附加到相应HTML节点；不能把独立截图当作任意img。RTFD转换在可终止helper内执行，有总长度/解码预算，输出受验证的portable模型及原native备份。

URL-only先登记Incomplete及缺失原因。若承诺来源只能提供外链，要实现独立有界ResourceResolver并单独审阅联网策略：不携带用户cookies/凭据，不读取file/local地址，不允许重定向绕过目标限制，下载/解码/deadline计入同一事件预算；资源失败保持Incomplete，保留原引用。首次本地修复阶段默认不开自动联网补全，完整来源门槛保持BLOCKED直到实现或获得明确支持范围调整。不能把这项写成已经支持Chrome所有网页图文。

**R5-07 / H-10,H-11,H-12：Windows兼容。** CF_DIBV5可由系统合成CF_DIB，因此“不手动补DIB”不能独立证明粘贴失败；先验证目标应用，仅有实际失败和明确必要性才补兼容DIB。[Microsoft Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats)。

Windows读取在OpenClipboard会话中只做受限提取并复制必要字节，关闭后做DIB→PNG/像素指纹。考虑delayed rendering，系统sequence=0和读取发生变化均有专用结果。[GetClipboardSequenceNumber](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber)。去掉helper不实际需要的blake3/pure隐式工具链耦合，保留合理portable-check边界；MSVC真实构建单独验证。

**Gate R5：** 原采集/恢复探针转绿；17-file样本走文件路径；多图整组完整、PNG选择不默丢第二图；native-only不能API绕过；ENOSPC无错误ACK/误删共享parts；失败结果双端可见；UI交互有行为测试，原生可访问性留R7。

### R6：全量本地与包门禁、证据重建

1. 运行第6节矩阵；默认/开启/运行时关闭三态；保留真实Iroh全量，不在最终门禁skip。
2. 若Iroh自然波动，保留首次失败并执行同环境基线对照。协议往返测试可分离为回环地址和明确deadline，但必须保留真实网络组；改变测试窗口不是产品预算修复。
3. 构建新候选，包内helper核对架构、最低系统、resolver和协议版本；debug、App内、Windows portable/installer分别验证。
4. 扩展canonical include检查器，覆盖clipboard_composite双端wrapper/shared include；保留负向测试，移除一端接线应失败。
5. 由实际命令日志/JSON diagnostics生成结果索引；警告按唯一诊断统计，Cargo汇总行不重复计数。缓存命中不能自动解释为clippy未运行，必要时独立scratch负向lint probe验证门禁。
6. 更新CONTEXT、根README、相关平台README、长期规格、ADR和执行台账。版本分开写：identity/base4、data frame5、main history条件v11/v12、effect journal1、local contract1、helper现行协议2、origin fingerprint2。任何实际协议变化必须另有版本决策。

**Gate R6：** 本地必需测试、构建、包静态检查和私有board故障探针通过；所有新证据绑定新源码/包hash。可标LOCAL_COMPLETE；Windows/目标App/双设备仍UNKNOWN时，发布仍NO-GO。

### R7：实机与双设备验收

沿用户已确认的节奏，在本地实现完成后安排，不重复询问环境。本阶段执行前确认TCP19890无冲突、启动包路径/hash正确、使用隔离数据；不自动终止现有daemon，不把旧App画面当新候选证据。

| 验收组 | 场景 | 通过标准 |
|---|---|---|
| Mac完整App | 新包启动、采集、历史恢复、通知与故障 | 生产daemon/GUI/helper全部接通；不能只用包内helper替代 |
| Windows原生 | MSVC、tauri dev、portable、NSIS安装 | helper定位正确；真实剪贴板读写与错误提示通过 |
| 来源应用 | Chrome、Word、Notes/可用富文本编辑器 | 实际源版本、format集合和resource完整性有记录 |
| 目标应用 | 微信、Word、富文本编辑器、终端/记事本、图片消费者 | 依据目标接受格式核验文字/所有图片；不把纯文本目标不显示图判为失败 |
| 双向同步 | Mac→Win、Win→Mac，单图/两图/RTF/中文emoji | 一逻辑条目；无拆分覆盖、无重复回音；内容和终态一致 |
| 意图竞争 | A复制后B同内容；慢传输中接收端再复制；双端交替100轮 | 新revision不吞；旧事件不覆盖新内容；双方不自激循环 |
| 网络 | LAN、Tailscale、Iroh-only direct/可用relay、重连/睡眠 | 无旧事件重放、ACK丢失不再写；路由与耗时明确 |
| 故障 | helper缺失/挂起/写后断回执、storage满/不可用、撤销授权 | 结果诚实、历史可恢复、不阻塞其他操作、不晚写 |
| 回退 | v11完整快照恢复、旧版本读取；升级后资料归档 | 外置payload真实可读，identity/trust/receipts不倒退 |
| 可访问性 | VoiceOver、仅键盘、IME、深浅色、高对比、缩放 | 菜单有名称，格式可选，焦点/反馈明确，两端无裁切 |

操作次数只用于持续稳定性采样；100轮不取代故障点和shape覆盖。目标应用拒绝某格式时保留失败及clipboard检查证据，决定是发布缺陷、转换缺陷或消费者能力限制，不能都归“对方应用问题”。

**Gate R7：** 承诺范围全部通过；每项UNKNOWN已解决或有用户明确接受的范围调整；复核没有数据丢失、回音、新意图被吞、未知结果假成功。只有此时才能评为可验收候选。

## 5. 必须执行的故障与回归用例

下面编号是计划中的新增回归，不表示已有测试。测试使用确定性时钟、计数器和barrier；不使用脆弱的sleep竞速证明业务正确。

| 测试ID | 覆盖 | 核心断言 |
|---|---|---|
| T01 | feature off Bash3.2/native/universal | 默认脚本到达真实build，传参正确；关闭态不是只cargo check |
| T02 | v11×feature/env/helper四种打开 | 禁用/不健康不建composite表、不升12；已有12可读；关闭态承接12仍保全全部数据 |
| T03 | 完整迁移/回退/空间不足/中断 | 旧版本可读真实图片、文件、收藏；不足不升级；备份独立且不被GC |
| T04 | HTML bindings 0/1/1000/5383与manifest边界 | 同一合法输入语义不变；UTF8次数按表示数，非binding数；长度预估无overflow |
| T05 | 图片decode、raw/内存预算 | 一资源每流水线decode一次；工作区有上限；失败释放预算 |
| T06 | 4 MIME所有排列、native回读顺序 | WireDigest兼容；本地OriginFingerprint一致；Png,Text实测不回播 |
| T07 | PNG metadata/色彩/alpha、HTML资源ID、OS合成格式 | 仅已定义等价变换可归SelfWrite；非等价必须Unknown/External按revision区分 |
| T08 | helper启动前、clear后、write后、receipt前注入失败 | NoEffect/Partial/Unknown准确；不自动重做、不自动清空、不吞后续新复制 |
| T09 | 未知关闭探测成功/失败、写中复制同内容 | resolution边界Unknown；边界之后新revisionExternal；无法探测不靠TTL解除 |
| T10 | 128条压力、pending未消费、restart/epoch/wrap | 活动和未知登记不被驱逐；容量满有可见拒绝；不跨epoch误判 |
| T11 | helper退出/取消/管道拒写/输出超限 | 所有child和线程回收；无晚写；读写错误不同；日志不含内容 |
| T12 | 250ms I/O并发timer/engine操作 | event loop仍推进；engine/settings guard不跨原生等待 |
| T13 | history commit/claim/JSON镜像/native/terminal/ACK逐边界崩溃 | 每次只许最多1 native attempt；历史与终态幂等；丢ACK不会再次写 |
| T14 | 两JSON导入、SQLite坏/JSON坏/两者坏 | 保守状态、不删空重播；健康副本恢复覆盖claim；旧binary不再执行已claim事件 |
| T15 | 同event跨session、digest碰撞、删除后重试 | 同终态、一历史；碰撞拒绝；用户已删行不复活 |
| T16 | A0→B0→A1、重复首片、4连接一身份 | 非法交错拒绝；合法串行不回归；身份quota不可绕过 |
| T17 | 暂时quota满与释放、分片deadline/时钟回退 | temporary transport可重试；过期warning；不刷新原timestamp；GC后旧事件不复活 |
| T18 | 多次重试/多peer相同表示/缓存预算 | encode一次；immutable chunk复用；缓存释放，不无限积压 |
| T19 | 旧端text、single PNG、RTF-only、多图 | 一旧事件；合法降级；不支持可见，不误称完整 |
| T20 | 新复制、history恢复、撤销授权穿插慢接收 | publish前再次检查；旧事件Superseded；已开始取消结果诚实 |
| T21 | 17-fileURL、混合items、Busy/Unstable/Timeout | 文件走20-file契约；失败ack只在终态可见后；不稳定不广播 |
| T22 | ENOSPC、parts写后SQL失败、并发删/GC/共享资源 | 无漏cleanup/误删共享资源；pending/snapshot引用受保护 |
| T23 | 收藏/配额/degraded恢复/存储迁移 | composite parts及映射同行迁移；失败不换库；已ACK内容不丢 |
| T24 | 双图整组/PNG选择/native-only各API入口 | 全部图保留；NoSingleImage；server guard一致 |
| T25 | Published/Partial/Unknown/Superseded UI | 仅Published显示复制成功；明确恢复动作；中英稳定错误 |
| T26 | 多warning、多window、队列溢出 | 不丢永久终态；按ID确认；聚合计数可见 |
| T27 | Preview DTO/生成fixture/include负向 | 大小语义正确；兼容字段/enum；删wrapper接线则门禁失败 |
| T28 | Windowsseq0/delayed rendering/owner/alpha | RevisionUnavailable正确；不双free；转换在关闭guard后；目标App验证 |
| T29 | 原生键盘/VoiceOver/Windows布局 | 格式菜单有名有动作；焦点反馈、缩放和IME可用 |
| T30 | Iroh、完整App、双设备与目标应用 | 按R7证据分类，真实失败不被单元PASS覆盖 |

性能采用两类断言：复杂度/次数作为稳定自动门禁；墙钟/RSS作为固定环境测量。首版目标为同机同opt-level下，1MiB/5383 bindings纯结构校验平均≤10ms且不随HTML×bindings乘积增长；该值是修复目标，未达须分析，不删样本降低输入。

同时测小包p50/p95、最大合法包encode/decode/native准备、后台timer延迟、helper队列、RSS峰值和缓存占用。若性能优化改变输入接受范围或格式语义，停止阶段，补兼容性决策和回归再继续。

## 6. 命令门禁与运行约束

以下命令是执行时的验收命令，本次编写方案未运行。工作目录均为第1节修复树；状态相关命令使用现有隔离runner。缺离线依赖时登记环境阻断，受控补依赖后重跑，不能改掉`--locked`或伪记PASS。

```bash
# 默认Core全量，包含Iroh
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  -p tailsync-core --lib -- --test-threads=1

# 协议扩展全量，包含Iroh
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  -p tailsync-core \
  --features 'protocol-composite-clipboard protocol-file-sliding-window protocol-image-compressed-chunks' \
  --lib -- --test-threads=1

# protocol/runtime
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  -p tailsync-protocol -p tailsync-runtime \
  --features tailsync-core/protocol-composite-clipboard

# runtime关闭专门用例；R6 固定命中 3 项
node scripts/run-isolated-tests.mjs -- env TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD=1 \
  cargo test --locked --offline -p tailsync-core \
  --features protocol-composite-clipboard --lib runtime_disabled -- --test-threads=1

# Mac默认 / 开启
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  --manifest-path macos/src-tauri/Cargo.toml --lib
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  --manifest-path macos/src-tauri/Cargo.toml \
  --features protocol-composite-clipboard --lib
node scripts/run-isolated-tests.mjs -- swift test --package-path macos/swift-ui

# Windows源码host测试不等于Windows原生
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  --manifest-path windows/src-tauri/Cargo.toml --lib
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  --manifest-path windows/src-tauri/Cargo.toml \
  --features protocol-composite-clipboard --lib
node scripts/run-isolated-tests.mjs -- npm --prefix windows test
npm --prefix windows run build
npm --prefix windows run lint

# 编译 / lint，两种feature态分别记录
cargo check --locked --offline --workspace --all-targets
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo clippy --locked --offline --workspace --all-targets \
  --features tailsync-core/protocol-composite-clipboard -- -D warnings
cargo clippy --locked --offline --manifest-path macos/src-tauri/Cargo.toml \
  --all-targets -- -D warnings
cargo clippy --locked --offline --manifest-path macos/src-tauri/Cargo.toml \
  --features protocol-composite-clipboard --all-targets -- -D warnings

# 契约与共享接线
node scripts/check-local-contracts.mjs
node windows/scripts/check_cross_platform_sync.mjs \
  --win-root windows --mac-root macos --core-root shared/rust-core
```

运行时关闭测试过滤器必须命中预定测试数，0 tests不能通过。上述只列基础套件，T01—T30新增故障回归须加入正式测试入口/CI，不能长期只留临时examples。

Mac包构建在新的run目录设置`TAILSYNC_APP_BUNDLE`，使用`/bin/bash macos/build-mac.sh`验证系统Bash；关闭态设置`TAILSYNC_COMPOSITE_CLIPBOARD=0`，开启态设1。native真实构建后再做universal目标；交叉target未安装应明确列环境门槛。

新包必须运行`codesign --verify --deep --strict`、现有`macos/scripts/check_macos_deployment.py`及包内helper探针。私有board探针使用隔离snapshot，不把故障helper覆盖产品包。完整App验收单列，提前确认19890与候选身份。

Windows上分别运行默认/开启的Rust test、clippy、helper MSVC build、tauri dev和打包脚本；示例打包开关为`-CompositeClipboard`，使用新的`-BuildDirectory`及`-OutputDirectory`。**Mac上的MSVC cargo check不替代链接、安装、原生运行。** 不使用`-SkipChecks/-SkipSmokeTest`通过正式门槛。

不改系统HOME/Windows USERPROFILE，不复用日常数据，网络组串行执行。GUI测试修改general clipboard时进入独立人工验收窗口，记录是否恢复，不能混同私有board无扰动测试。

## 7. 审查报告52项的闭环映射

| 原编号 | 本方案处理 | 闭环证据 |
|---|---|---|
| A | R1-01，真实默认构建 | T01 + 两态真实包 |
| B | R1-02/03，政策与完整回退 | T02/T03；不是声称完全不可逆 |
| C | R2-01/02/04，复杂度和raw预算 | T04/T05 + 同机微基准 |
| D-1 | R1-04/R5-01，健康与故障出口 | T21；Mac缺helper不假fallback |
| D-2 | R5-01，失败可见才确认revision | T08/T21/T26 |
| D-3 | R4-03，保留合法串行契约 | T16；不无条件message-ID抢占 |
| D-4 | R2-04/R4-03，身份级配额 | T16/T17 |
| D-5 | R4-04/R5-04，Rejected warning | T19/T26；采集失败另修 |
| E | R3-02/03/04，副作用结果及关闭 | T08/T09/T11 |
| F | R3-01，异步端口与短锁 | T12 + 实际adapter |
| G-1 | R5-04，沿现有样式修格式动作 | T29；Windows视觉验收 |
| G-2 | R5-03，单图选择守卫 | T24；整组两图原本保留 |
| G-3 | R3/R5-03，Partial typed outcome | T08/T25 |
| G-4 | R5-03，native-only先做服务端guard | T24，各API入口 |
| G-5 | R5-04/R7，显式无障碍与键盘 | T29；不声称已实测完全不可用 |
| G-6 | R5-04，稳定错误码与Loc | T25，两种语言 |
| G-7 | R4-04，immutable编码缓存 | T18，计入预算 |
| G-8 | R4-04，统一绝对deadline | T17，时钟与assembler期限 |
| G-9 | R2-04，同C的raw与内存预算 | T05/T17 |
| G-10 | R4-01，可证明覆盖的恢复 | T14；不删空重建 |
| G-11 | K2/R3/R5-07，RevisionUnavailable | T28，Windows原生 |
| G-12 | R4-04，RTF-only明确不可降级 | T19，不静默Rejected |
| G-13 | R3-01/R4-02，短事件锁 | T12/T15，全局发布仍1 |
| G-14 | K5/R4-01，保留至多一次尝试 | T13；补恢复UX，不自动redo |
| H-1 | R5-02，清死代码并保留实际quota | T22/T23 |
| H-2 | R5-02，RAII与活引用cleanup | T22，ENOSPC/commit失败 |
| H-3 | R5-05，preview大小语义 | T27，不宣称生产已显示0字节 |
| H-4 | R5-02，批量parts查询 | 查询计数与大历史测量 |
| H-5 | R3-02，统一cleanup/join | T11；僵尸原报告未证实 |
| H-6 | R3-02，有限stderr与读写错误 | T11，无内容泄漏 |
| H-7 | R1-04/R3-02，共用resolver | dev/bundle路径负向测试 |
| H-8 | R1-04/R7，实际Windows产物接线 | tauri dev与安装包实测 |
| H-9 | R5-01，文件先于composite cap分类 | T21 + Finder实际17文件 |
| H-10 | R5-07/R7，按消费者证据决定DIB | 自动合成已核对；目标App未知 |
| H-11 | R5-07，缩短Windowsclipboard guard | T28，锁持有计时 |
| H-12 | R5-07，整理helper工具链依赖 | MSVC真实build；非已证实发布失败 |
| H-13 | R5-04，warning结构化队列 | T26；承认单槽为基线已有 |
| H-14 | 保留现有validate拒绝路径，不修虚假广播缺陷 | T21/负向snapshot测试 |
| H-15 | R5-04，表示与图片数分开 | 合法portable/native DTO fixtures |
| H-16 | R5-04，显式格式标签/本地化 | T25，images0等边界 |
| H-17 | R5-05/R6，契约导出与版本决策 | T27，生成物drift |
| H-18 | R3-01，文件恢复释放engine guard | T12，保留文件commit/ACK语义 |
| H-19 | R3-03，pin活动和未知来源记录 | T10；原报告是理论边界 |
| H-20 | R2-03/R3-03，实际投影与顺序无关指纹 | T06/T07，原真实adapter复现 |
| I-1 | R6，共享include注册及负向门禁 | T27 |
| I-2 | R6，准确描述条件v11/v12 | 文档与实际policy测试一致 |
| I-3 | R0/R6，长期spec/ADR | K1—K10与支持矩阵可追踪 |
| I-4 | R6，证据清单与可分享材料 | source/package/log SHA256；正文脱敏 |
| I-5 | R6，程序解析诊断数量 | 不把summary重复计warning |
| I-6 | R6/R7，状态口径一致 | 每门槛独立PASS/FAIL/UNKNOWN |
| I-7 | R0—R7，新增行为/故障门禁 | T01—T30；保留已有contract测试 |
| I-8 | R6，缓存lint的证据化核对 | 缺证据不记缺陷；必要负向probe |

52项均有去向，但不等于52个独立严重缺陷。重复根因只实现一个权威边界，各入口补回归。

## 8. 整体把关、交付和停止条件

### 8.1 每一阶段的完成包

每阶段输出：改动清单、受影响生产调用链、修复前失败证据、修复后同一用例证据、回归范围、源码指纹、尚未通过项与下一阶段条件。逻辑批次由review检查，不自动commit/推送。

最终证据目录建议为`artifacts/composite-repair/<run-id>/`，包含：

- `source-fingerprint.json`：HEAD、dirty diff、含untracked文件清单与聚合hash。
- `results.json`：每条命令、退出码、预期断言、真实测试数、失败/跳过、平台和环境。
- `fault-matrix.json`：故障点、native attempt计数、历史计数、终态、预算释放、child回收。
- `package-manifest.json`：新包路径、架构、最低系统、helper版本、签名与文件hash。
- `native-acceptance.md`：完整App、目标应用、Windows、双设备记录，版本/route/source/target明确。
- `rollback-proof.md`：完整快照和外置payload验证、旧版本打开、升级后数据归档、receipts保留。
- 适当脱敏的日志和probe说明；大包通过制品机制分享，不把私密正文/数据目录提交Git。

历史证据只归属于旧指纹；修复后重新跑当前必需门槛。已有21份日志、固定顺序10000次probe不能继承为新包的通过证明。

### 8.2 评审分工与硬停止条件

按职责评审：数据/迁移负责人检查备份、parts、claim与ACK；并发负责人检查lock/permit、取消与预算；平台负责人检查OS I/O和helper；UI负责人检查结果、恢复与无障碍；验收负责人检查证据覆盖。一个人可承担多项，但必须留下对应检查记录，不能只有总分。

出现以下任一项，停止当前阶段进入诊断，不扩大修复范围掩盖：

1. 已ACK历史/收藏或可恢复parts消失；rollback缺载荷；恢复倒退trust/receipts。
2. 同事件native attempt超过1；Unknown被当NoEffect重试；Partial/Unknown显示成功。
3. 新revision的相同用户复制被自写过滤；自写因顺序/编码变化自动回播。
4. helper取消后仍晚写；engine/settings锁跨I/O；预算泄漏/过界。
5. 未授权peer、撤销后新操作或未协商的新命令可达系统写入。
6. 为变绿关闭测试、缩小承诺范围、丢失败日志或用host结果代替Windows原生。

性能小幅偏差、继承的lint warning及尚未证明的Windows兼容假设可分类继续处理；不能与上面的数据和因果门槛混记为普通待办。

### 8.3 最终决策表

| 状态 | 要求 | 允许结论 |
|---|---|---|
| IMPLEMENTED | 代码完成，故障/本地门槛未全部通过 | 不能验收 |
| LOCAL_COMPLETE | R0—R6通过，新源码/包证据完整 | 本地修复完成；实机待安排 |
| NATIVE_PARTIAL | 部分原生通过，其他UNKNOWN/FAIL | 仅说明已验收子集，整体NO-GO |
| ACCEPTED_CANDIDATE | R7承诺范围全通过，必要未知已解决 | 可进入独立发布评审 |

发布、默认启用、安装替换、tag与push另属授权范围，本方案不自动执行。最重要的把关标准是：**内容可恢复、结果真实、用户新意图优先、重试不重复副作用、证据对应实际产品路径。**

### R6 实际执行入口

`python3 scripts/run-composite-r6.py <new-run-id>` 已补齐本节命令矩阵、三态、真实 Iroh 组、canonical includes 正负向、日志解析、scratch lint 负向及包检查。当前逐门槛状态及 H-17/I 系映射以 `docs/COMPOSITE-CLIPBOARD-R6-VERIFICATION.md` 为准；新 evidence 不继承 R0–R5。
