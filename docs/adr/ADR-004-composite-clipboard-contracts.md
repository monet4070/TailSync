# ADR-004：复合剪贴板的十条契约与三个深边界

2026-10-23 补充：[ADR-006](ADR-006-browser-image-alternative.md) 为 K6 的“不完整则不执行 native effect”增加一个受限例外：网页单图可以只发布已验证的独立 PNG，最终仍保持 StoredIncomplete；其余持久化、授权、claim 和不重复发布约束继续有效。

2026-10-29 补充：[ADR-007](ADR-007-web-article-sync.md) 将上述例外扩展至已验证可独立发布的网页图文副本，保留正文、已保存图片及其位置，缺图原位占位；最终仍保持 StoredIncomplete。产品优先级调整为正文、图片和图文顺序。

- 状态：已接受（2026-10-04）
- 范围：`shared/rust-core/src/{composite,peer,sync,db}/`、`shared/tailsync-protocol/`、
  `shared/platform-clipboard-composite.rs`、两端 `clipboard*`/`sync_adapter.rs`、两端 UI 恢复入口
- 依据：`docs/COMPOSITE-CLIPBOARD-REPAIR-EXECUTION-PLAN-2026-10-04.md`（执行方案）、
  `artifacts/composite-evidence/acceptance-verification-2026-10-04/VERIFICATION.md`（52 项裁定）、
  `docs/COMPOSITE-CLIPBOARD-ACCEPTANCE-REVIEW-2026-10-04.md`（原审查）
- 上游：ADR-001（core 深模块与平台适配器边界）。本 ADR 是它在"带不可逆系统副作用"场景下的补充。

## 背景（修复前核验）

图文复合剪贴板候选把文字、HTML、RTF 与图片作为一个逻辑条目采集、同步并整组恢复。独立核验确认：
现有抽象在这件事上出现了三个**没有接缝**的边界，缺陷集中堆在这三处，而不是分散在富文本转换逻辑里。

1. **能力开关不能表达持久化变更。** `protocol-composite-clipboard` 是空 feature，全仓唯一 `#[cfg]`
   是一个测试；协商与自动采集受 `cfg!` 与运行时开关控制，但 schema v12 迁移无条件执行。
   "关闭即不受影响"的用户预期与"打开一次即不可原地回退"的事实不一致。
2. **同步平台接口不能表达外部进程的不可逆副作用。** `SyncPlatform::write_text/write_image`
   是同步 `Result<(), String>`，没有超时预算、没有"结果未知"、没有部分成功、没有降级通道。
   复合发布把它变成"spawn helper + 5 秒截止 + clear→write→receipt 可中断"，于是阻塞落在
   async worker 与 engine guard 内，且 Err/Partial/Unknown 无法映射到来源登记、重试与 UI。
3. **失败出口与内容身份覆盖不足。** 采集失败后 `acknowledge` 吞掉变更；临时装配压力被归类为
   永久 `Rejected` 且不产生用户可见告警；恢复的 `Partial` 被泛化成 `Err`；
   来源指纹等于线协议摘要，因而依赖表示枚举顺序，与 OS 回读规范化不一致。

此外核验量化了一个独立的复杂度问题：`PortableBundle::validate()` 在 bindings 循环内对整块 HTML
重跑 `from_utf8`，bindings 上限约 5383，单次校验约 184 ms（2 MiB HTML），而一次接收会重复校验多次。

## 决策

### 一、冻结 K1—K10（规范性）

以下十条为本次修复的**契约冻结**。执行方案的 §2 是完整措辞；本 ADR 记录其规范要点，
任何实现偏离都视为设计变更，需新的 ADR。

**K1 区分三种摘要。** `WireDigest`（保持现有 envelope/bundle 编码与 ACK 语义，覆盖原始线协议字节，
**不得**为防回音而排序表示）、`OriginFingerprintV2`（仅供本机来源判定，规范化系统可观测的发布集合；
表示顺序无关，PNG 无关编码元数据无关，但文字空白、RTF 内容与 native item 边界必须保留）、
`EventReceiptDigest`（legacy 与 composite 各自保持现有算法，导入不得统一重算）。
原生发布格式的优先顺序与指纹计算顺序是两件事。

**K2 原生结果升级为有副作用语义的结果。** `NoEffect`（可证明未进入 clear/set/write，可在固定
deadline 内重试）、`Published`（回执完整、token 正确、所选格式发布完成，不重做）、
`Partial`（副作用已发生但不完整，不自动重做）、`Unknown`（可能已发生，回执丢失或损坏，不自动重做）、
`Superseded`（已观测 generation/revision 变化且未写入）。Windows `sequence=0` 是
`RevisionUnavailable`，不得伪装为 `Superseded`。

**K3 来源分类先于内容去重。** `稳定快照 → 匹配本地操作与 native revision → 来源分类 → 历史与广播策略`。
`SelfWrite` 必须对应同一 clipboard epoch 下 revision 与实际规范化发布指纹都匹配的已知结果，
token 只是关联证据；新 revision 即使内容相同或带旧 token 也归 `External`；已知 revision/token
而指纹不一致归 `UnknownMismatch`，禁止直接当 External 自动回播；不稳定快照不得进入成功广播路径。
结果必须传到既有 `shadow_check`/`image_shadow_check` 与 Core 门禁。

**K4 未知副作用必须有闭合边界。** 操作状态 `Prepared → Claimed → InFlight →
KnownComplete/KnownPartial/Unknown/NoEffect → Closed`。**先登记 intent，后启动 helper**；intent 不是成功回执。
回执丢失后只有在 helper 被终止、回收、管道线程结束并完成稳定探测后才能建立 `resolution_revision`；
该边界对应内容仍归 `Unknown`，不凭 token/hash 补造成功。kill/reap 不能独立证明 OS 服务没有待完成请求。
未知期间"每次用户复制都自动广播"与"所有自写都无回音"不可兼得；本决策选择保全可恢复内容、
阻止不确定自动回播、提供显式恢复，并把该取舍写入产品文档。

**K5 系统副作用之前，内容与 claim 已持久化。** `Authenticated → Admitted → Validated →
HistoryPreparedDurable → EffectClaimDurable → NativeEffect → OutcomeDurable → Completed → FinalAck`。
收到旧 claim 无结果时恢复历史/结果元数据并返回 `Unknown`，不重新执行系统写入。durable 不等于
"调用了 commit/rename"：effect journal 使用 SQLite WAL + `synchronous=FULL` 并检查提交结果；
history/parts 在 claim 前满足相同恢复承诺；外置文件与兼容 JSON 使用既有 `private_fs` 原子替换并核对
文件与父目录的持久化步骤。进程崩溃与断电/文件系统持久化分别记证据。

**K6 最终 ACK 只陈述已持久化的事实。** 分片 ACK 只表示分片接收；最终结果等待历史、claim、终态与
兼容回执提交。Composite 沿用 `Applied/Superseded/ApplyUnknown/FailedPartial/StoredIncomplete`。
协议与资源验证合法但目标准备超限或不支持时，保存完整内容并以 `StoredIncomplete` 加明确原因终止、
不执行 native effect。Legacy 不修改现有 EventAck 帧格式；忙/准入压力尚未产生副作用时保留为可重试
传输失败；格式不支持与永久拒绝必须可见。

**K7 锁只保护短状态读写。** 不得持有 `SyncEngine`、settings、全局 finish 锁等待 helper、DB、
PNG 转换、管道 join 或 async permit。原生发布 permit 全局为 1（系统只有一个目标剪贴板）；
校验、历史准备与不相同事件的去重不因此全局串行。

**K8 功能、健康状态与数据库演进分开。** `compiled_feature && runtime_enabled && helper_healthy`
决定新采集和迁移准入；自动发布与新会话宣告还要求 `origin_healthy && journal_healthy`，实际握手必须
调用完整健康谓词。来源或 journal 异常不关闭健康读取器的历史保存；v12 **读取能力**与是否允许 v11→v12 **迁移**是两个值。关闭态承接既有 v12
历史时目的库必须至少是源库 schema（保全数据），不得为保持新目录 v11 而丢掉 composite；
没有承接 v12 数据的新库保持 v11。运行中 helper 失效时关闭新采集与新协商能力，已协商会话在
无副作用阶段给出明确失败。Mac headless legacy 读取同样依赖该 helper，不得把"退回 legacy"
当作缺 helper 时必定可用的恢复。

**K9 一个不可变事件，一个固定截止。** 事件 ID、原 timestamp、peer 选定表示与 wire digest 在重试中
固定；总截止是 enqueue TTL 与原 timestamp 有效期的较早者，重试不重新计时。同一 peer/event ID
不得在可能发生副作用后改成不同 fallback 内容。撤销授权、队列过期、generation 更新在发布前再次检查。

**K10 证据不能跨层替代。** 源码确认、单元测试、平台 adapter、包内 helper、完整 App、Windows 原生、
双设备、目标应用分别记录。来源不完整时不计完整成功；例外与豁免必须说明影响、证据与替代验证，
不允许用"基线已有"自动放行。

### 二、确立三个深边界

按 ADR-001 的"core 深模块 + 平台适配器"原则，新增三个边界，而不是在每条错误分支打补丁：

1. **ClipboardCoordinator + 异步 ClipboardPort。** 来源、generation、发布串行与 typed outcome
   集中在 Core；`SyncEngine` 只准备授权/generation/revision 上下文并接收完成结果，**不在 engine guard 内
   执行系统 I/O**。端口是 object-safe 的异步 trait（返回 boxed Send future），以 `Arc` 持有。
   仅给现有同步函数包 `spawn_blocking` 不满足本决策。
2. **EffectJournal（独立 SQLite 元数据库）。** 独立 schema1，位于 identity/settings 私有根，
   不依赖 history 迁移与用户删除；只保存来源、事件身份、immutable digest、状态、终态、原 timestamp、
   operation token/epoch 与可恢复历史引用，不保存正文或绝对路径。两个既有 JSON 回执**必须先做兼容桥**
   再谈弃用；回退期间"SQLite claim + JSON claim 都持久化后才写系统"。
3. **MigrationPolicy。** `LegacyOnly / CompositeAllowed` 显式贯穿 `db/open.rs`、`db/schema.rs`、
   `db/migrations.rs` 与 degraded/reopen 路径；policy 同时携带恢复源要求的最小 schema。
   不能只 gate 迁移而仍让 `initialize` 建 composite 表。

同时确立 **ValidatedComposite**：不可变、只能由校验工厂构造、私有字段持有原始合法 wire 与转换缓存，
使"校验一次"成为可复用对象，消除 bindings×HTML 的重复扫描。

### 三、冻结支持范围

Text/PNG/HTML/RTF 单表示、一语义 item 多表示、HTML+多图资源（节点绑定正确、整组不丢图）、
多文件（遵守文件批次自己的 20 文件上限，17 文件采集回归不得被 composite 的 16-item 上限截断）、
原生多 item/RTFD（同平台原始回放；无法无损转换时标 native-only/incomplete）、
URL-only（缺字节显式 incomplete；首次本地修复阶段**默认不开自动联网补全**）、
旧端不支持 composite（每事件只选一个旧表示；多图/RTF-only 不可靠降级时告知限制）。

RTFD 附件转换与 URL-only 补全**不是**通过容器自动获得的能力；Chrome/Word/Notes 的承诺用例逐个登记，
不得把未实现的来源默默删掉以通过验收。

## 影响

- 复合剪贴板从"默认关闭的候选"变成**有条件启用**：`compiled_feature && runtime_enabled &&
  helper_healthy` 三者同时成立才宣告能力；关闭态不再改写持久化 schema。
- `SyncPlatform` 的同步写入方法不再承载复合发布；legacy text/image、native-only 恢复、单格式恢复、
  文件历史激活与直接 API 都必须经过同一协调器，不允许留下绕过来源登记的第二条写入路径。
- 回执权威从 JSON 文件迁到 SQLite journal，但旧 JSON 在允许回退期间继续写入，
  因此**回退验收必须注明旧版本没有新 journal 的时钟防护**（`replay_floor_ms`）。
- 用户可见结果从"成功/失败"细化为 `Published/Partial/Unknown/Superseded/Unsupported`；
  只有 `Published` 可以显示"已复制"。
- 性能上，校验复杂度不再随 bindings×HTML 乘积增长；首版目标为同机同 opt-level 下
  1 MiB/5383 bindings 纯结构校验平均 ≤10 ms（设计目标，未达须分析，不删样本降低输入）。
- 证据分级（K10）意味着本地全绿只能记为 `LOCAL_COMPLETE`；Windows 原生、目标应用与双设备仍为
  `UNKNOWN` 时整体保持 NO-GO。

## 未决

- URL-only 资源补全是否实现，取决于真实来源证据；未实现前完整来源门槛保持 BLOCKED。
- RTFD 附件跨平台转换的预算与等价性判据需真实样本确定。
- 微信等目标应用接受哪些格式组合，必须按版本实测，不从系统包含 HTML 推导。
- 旧 JSON 兼容桥的退休条件（何时可以停止写兼容 JSON）需在允许回退窗口结束后另行决策。
- 事件跨 peer 先完成准备后进入全局 FIFO；在取得 permit 后重新核对 generation/授权，过期 generation
  保存历史但不发布。permit 随 native worker 而非 async 等待者释放。设备时序验收仍属 R6。

## 2026-10-05 R1–R5 补修

- 同一 effect journal 同时负责 composite 和 legacy claim；兼容镜像均从它派生，严格导入旧 JSON。
  每个操作核验事件 digest；历史引用与 claim 原子写入。startup 结算已有正文，未知副作用不重做。
- intent/未消费回执可持久恢复；PID 在 helper stdin 前登记。不跨 epoch 比较 changeCount/sequence，
  只在稳定观测与进程已结束后建立 Unknown 边界。未消费记录不按 TTL 删除，不健康元数据保留原件。
- 管线配额随 blocking worker、准备数据和重试缓存释放；临时压力不得写成永久缓存失败。
  prepared history 的保护随最后一个 worker 释放，取消不能长期阻塞 trim 或 migration。
- UI 按 ID peek/ack，隐藏窗口不消费；Mac 告警轮询独立于 history version，避免第二条警告滞留。
- RR01–RR30 全部纳入门禁，检查实际执行数量与漏跑登记项；原生探针每次检查当前源码构建状态。

详细修改、命令与设备边界见 [R1–R5 补修记录](../COMPOSITE-CLIPBOARD-R1-R5-FOLLOWUP.md)。

## R6 版本决策与自动门禁（2026-10-05）

复合剪贴板的版本分别管理：identity/base **4**；data frame **5**（认证连接可协商回退到 4）；main history **条件 v11/v12**；effect journal **1**；local contract **1**；helper protocol **2**；origin fingerprint **2**。v11→v12 受构建开关、运行时开关和 helper 健康控制；已有 v12 在关闭态可读取，承接已有 v12 数据的目的库保持至少 v12。版本变更须有独立决策，不能把这些编号统称为“协议版本”。

R6 没有修改 helper wire、数据帧或摘要算法。本轮新增的是只读 resolver 诊断入口、执行过滤器、共享接线检查和证据采集；保持既有版本。此前引入 OriginFingerprintV2 与 helper protocol 2 的决定已在本 ADR/修复方案登记。未来实际协议变更必须单独决策并保留旧端回归。

K10 的自动执行入口为 `scripts/run-composite-r6.py`。K8 关闭态 3 项固定命中；T27 canonical 包含 clipboard_composite 双端 wrapper/模块注册，移除任一端必须失败；真实 Iroh 网络组使用 N0 和实际接口地址，协议回环组另有 120 秒测试总截止（不改变产品预算）。JSON diagnostics 按 code/message/primary spans 去重，Cargo 汇总行不计 warning；cache 命中不推断 lint 未执行，独立 scratch eq_op 负向门禁核验执行。

证据边界保持：本地测试、私有 board、包静态可 PASS；完整 GUI、Windows、目标 App 与双设备各自 UNKNOWN 时不能形成发布许可。

## O04：显式公开图片归档（2026-10-06）

仅 authenticated 用户动作增加加密 `archive_layer`（派生 CBP1），原 capture/content/dedupe／HTML／CNA1 保持。Manifest 明确升为 **3**，使用 `portable-cbp1+resource-archive`，尾部可选 `archived_info` 提供派生列表状态；v1／v2 禁止该语义，冻结 v2 序列化／digest 不变，旧 reader 对 v3 fail-closed。schema 13、HistoryIdentity 1、CNA1 1、helper／网络版本不变；local contract 2 追加命令和生成 report DTO。

读取／提交证明只替换原 img/src 合法 URL 的精确跨度，其他表示／原资源不变、bindings 精确移位，MissingFormats 不升级。恢复仍走 O06 RestorePlan／资源门，不新增发布捷径、clipboard 副作用、同步或文件 lease。

首批只取公开 HTTP(S) 静态 PNG，不携凭据／代理／自动解压，每跳全 DNS 校验＋单 endpoint pin＋实际连接相等。8 引用／8 MiB 单图／16 MiB 总读取额度／3 redirect／10 秒单请求／30 秒资源阶段／45 秒整体操作硬夹紧；网络与 Runtime 各 admission 2。entity URL、CSS/srcset/SVG/object、blob/file/登录态未支持，原件保留且 withheld。

Mac socket request_id／断连取消与 session task，Windows Tauri 截止／迟到 session 提示隔离分别验证。真实 GUI／目标 App、Windows 原生、双设备和 hosted CI 不由本机探针推出，Release NO-GO。详细证据见 [O04 报告](../COMPOSITE-CLIPBOARD-O04-RESOURCE-ARCHIVE-2026-10-06.md)。

## O05：显式来源授权文件桥（2026-10-06）

可选 Chromium MV3 扩展只请求 activeTab／scripting。用户分别选择授权文件保存、扩展加载与收集、资源 packet 导入；不自动安装／复制／同步。既有父 PID＋私有 token 认证和 1 MiB API line cap 不变，不新增网页 CORS／localhost HTTP／native messaging；API 只传 id＋path。

短期 grant 绑定完整 HistoryIdentity、当前 DB authority、canonical origin、原 img/src 的 index＋SHA256，最多 8 个 live grant／300 秒／一次消费；撤销取消 in-flight，提交前复查 expiry／取消／authority／CAS。Ticket 最多 64 KiB；packet 最多 24 MiB JSON、8 节点、8 MiB／PNG、16 MiB 合计。扩展自身在读取期间夹紧合计额度、10 秒单请求／30 秒整体截止，navigation、srcset drift、重复匹配、非静态 PNG 拒绝。

采用 Manifest **4**／`portable-cbp1+resource-archive+source-context`。可选 `source_info` 仅记录 origin 与实际补入节点，不记录 grant、Cookie、完整来源 URL 或 token。每个节点的 index、原 src hash、原 reference origin 与精确 span 必须重新对应；原件与身份域不改写。partial→source 重试合并旧证明；source→public 只追加合法公开缺项且保留旧证明／资源／bindings。旧 reader 对 v4 fail-closed，v2/v3 冻结合同不变。

文件桥是用户授权导入，不是浏览器字节的签名证明：用户能修改 ticket／packet，TailSync 的存储证明只验证授权范围与派生关系，不能密码学证明 PNG 来自网页。真实浏览器加载、登录态／blob 页面、文件对话框、目标 App、Windows 原生、双设备与 CI 仍须独立验收。完整界限见 [O05 报告](../COMPOSITE-CLIPBOARD-O05-SOURCE-CONTEXT-2026-10-06.md)。
