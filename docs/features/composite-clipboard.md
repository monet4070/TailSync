# 图文复合剪贴板

TailSync 把一次复制产生的文字、HTML、RTF 与图片作为一个**逻辑历史条目**和一个**不可变同步事件**
采集、同步并恢复，使对端一次粘贴即可得到完整图文，而不是先到文字、再到图片互相覆盖。

本规格是长期规格。实现状态见 `docs/COMPOSITE-CLIPBOARD-REPAIR-EXECUTION-PLAN-2026-10-04.md`
（执行方案与阶段门槛）；R1–R5 补修与当前本地证据见 [补修记录](../COMPOSITE-CLIPBOARD-R1-R5-FOLLOWUP.md)；契约冻结见 [ADR-004](../adr/ADR-004-composite-clipboard-contracts.md)。
本规格描述**目标行为**，不把未验收项写成已完成。

2026-10-29 产品优先级：网页选区的正文、图片数量及图片在文字间的位置是核心；字体、颜色和复杂排版为次要内容。实时接收可发布已验证的可用图文副本，缺图原位占位，原件及不完整结果保留。产品范围和验收边界见 [ADR-007](../adr/ADR-007-web-article-sync.md)。

同日第二批已实现首次发送前的有界公开图片获取：开启“自动保存网络图片”后，在初次复制时尝试取得图片，保存后生成一条固定事件；超时保留可用部分，稍后补图只更新历史。设置默认值不变，真实网页／双设备验收仍待完成，见 [ADR-008](../adr/ADR-008-first-publish-network-images.md)。

## 用户语义

- 一次复制 = 一个历史卡片 = 一个同步事件。文字与图片不拆成先后覆盖的两个事件。
- 恢复有两种粒度，语义必须可区分：
  - **整组恢复**：按原样恢复该条目的全部表示与图片资源，图片数量、顺序与节点绑定都保留；
  - **显式单格式恢复**：只恢复用户明确选择的那一种表示。
- 结果必须如实：
  - 只有 `Published` 显示"已复制"；
  - `Partial` 说明"已保存历史、发布未完整"；
  - `Unknown` 说明"结果未知"并提供历史恢复；
  - `Superseded` 说明"已保存历史、保留了更新的复制"。
- 部分与未知**不得**显示为成功。一次手动恢复创建新的本地操作，不重新开放旧的远端 claim。

## 支持范围

| 内容形态 | 行为 | 成功条件 |
|---|---|---|
| Text / PNG / HTML / RTF 单表示 | 保留原内容，走相应兼容路径 | 目标格式可用，结果明确 |
| 一语义 item、多表示 | 一个 native item 同时发布多格式 | 全部选定格式发布；纯文本、图片、富文本消费者分别验证 |
| HTML + 多图资源 | 保留全部资源与正确节点绑定 | 图片数量、顺序、位置与文字均正确；整组恢复不能只取首图 |
| 多文件 | 保留既有文件批次语义 | 17 个文件的采集回归不被 composite 的 16-item 上限截断；遵守文件批次自己的 20 文件上限 |
| 原生多 item / RTFD | 同平台原始回放；跨平台仅用已验证的转换 | 无法无损转换时标 native-only / incomplete，不声称跨平台完整 |
| URL-only 图片引用 | 优先使用源剪贴板已有的字节 | 缺字节时显式 incomplete；不自动联网抓取（见下） |
| 正文与部分已保存图片，其他图片或网页样式依赖不可用 | 实时接收与历史恢复共用可用副本选择，保留正文、图片绑定及缺图位置 | 已验证的副本只发布一次；原件不变，最终仍为 StoredIncomplete；自动接收不放开 MissingFormats，见 ADR-007 |
| 网页单图同时有独立 PNG 与未保存 HTML 引用 | 预览、回溯与实时接收可选已保存 PNG；原件与引用保留 | 无正文、无 RTF、只有一图且格式未缺失；仅发布 PNG，最终仍为 StoredIncomplete，不宣称完整 HTML 恢复；见 ADR-006 |
| 旧端不支持 composite | 每个事件只选一个旧表示 | 单图明确则优先图片，否则文本；多图/RTF-only 不可靠降级时告知限制 |

RTFD 附件转换与 URL-only 图片补全**不是**通过容器自动获得的能力。若将来实现 URL 补全，必须有独立的
有界 `ResourceResolver` 并单独审阅联网策略：不携带用户 cookies/凭据、不读取 file/local 地址、
不允许重定向绕过目标限制、下载与解码计入同一事件预算，失败保持 incomplete。默认不开自动联网补全。

## 启用条件

复合剪贴板只在三者同时成立时宣告能力并启用：

```
capture = compiled_feature && runtime_enabled && helper_healthy
automatic_publish = capture && origin_healthy && journal_healthy
```

- `compiled_feature`：构建时启用 `protocol-composite-clipboard`。
- `runtime_enabled`：未设置 `TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD`。
- `helper_healthy`：启动时通过无正文的 helper 版本/自检。

启动顺序固定为：无正文 helper 自检 → 冻结打开 policy → 打开 history/journal 并恢复 pending →
对外监听与协商。运行中 helper 失效时关闭新采集与新协商能力；已协商会话在无副作用阶段给出明确失败。
Mac headless 的 legacy 读取同样依赖该 helper，因此"退回 legacy"不是缺 helper 时必定可用的恢复。

来源记录或 effect journal 不健康时，不宣告新的 composite 会话，也不执行自动发布；健康的读取器
仍可保存本地历史。握手使用实际健康谓词，而非仅使用构建开关。startup 恢复不读取旧 clipboard
revision：未完成操作保留 token/digest/受监督进程 PID；首次稳定观测只建立新的 epoch 边界。
helper PID 在提交 stdin 之前持久登记；恢复中的存活或无法确认状态保持保守，不杀死未知进程。

## 结果与状态

原生发布结果是五个有副作用语义的值，而不是 `Ok/Err`：

| 结果 | 证据 | 自动重试 | 用户结果 |
|---|---|---|---|
| `NoEffect` | 可证明尚未进入 clear/set/write | 可以，在固定 deadline 内 | 临时失败 / 重试中 |
| `Published` | 回执完整、操作 token 正确、所选格式发布完成 | 不重做，只补持久化/ACK | 已复制 / 已同步 |
| `Partial` | 已执行副作用，格式组不完整或 clear 后未完整填入 | 不自动重做 | 部分发布，提供历史恢复 |
| `Unknown` | 可能已执行副作用，回执丢失/损坏或进程异常 | 不自动重做 | 结果未知，提供历史恢复 |
| `Superseded` | 已观测 generation/revision 变化，未执行写入 | 不重做这次旧事件 | 已保存历史，保留较新复制 |

Windows `GetClipboardSequenceNumber() == 0` 是 `RevisionUnavailable`，**不得**伪装为 `Superseded`。
`Partial`/`Unknown` 之后不自动清空、不自动恢复先前内容、不自动再次写入，以免覆盖用户的新复制。

## 来源与去重

```
稳定快照 → 匹配本地操作与 native revision → 来源分类 → 历史与广播策略
```

- `SelfWrite` 必须对应同一 clipboard epoch 下 revision 与**实际规范化发布指纹**都匹配的已知结果；
  token 只是关联证据，不能单独授权抑制。
- 未消费的确认回执不因 TTL 淘汰；容量不足时拒绝新副作用。未知关闭边界归 `UnknownMismatch`，不伪装为 `SelfWrite`。
- 新 revision 在操作关闭后的观测中出现，即使内容相同或带旧 token，也归 `External`。
- 已知 revision/token 对应的指纹不一致归 `UnknownMismatch`，保留诊断，禁止直接当 External 自动回播。
- 不稳定或未验证的快照不进入成功广播路径。
- 用于来源判定的指纹与线协议摘要是**两个**摘要：前者表示顺序无关、忽略无关编码元数据，
  但保留文字空白、RTF 内容与 native item 边界；后者保持现有编码语义不变。

## 持久化与回执

系统副作用之前的顺序是：

```
Authenticated → Admitted → Validated → HistoryPreparedDurable → EffectClaimDurable
              → NativeEffect → OutcomeDurable → Completed → FinalAck
```

- 先登记 intent，后启动 helper；intent 不是成功回执。
- 收到旧 claim 而无结果时，恢复历史与结果元数据并返回 `Unknown`，**不重新执行系统写入**。
- 回执权威是独立的 effect journal（SQLite WAL + `synchronous=FULL`，并检查提交结果）；
  在允许回退旧二进制的期间，兼容 JSON 与 journal 都提交后才写系统、才发最终确认。
- 回执丢失后只有 helper 被终止、回收、管道线程结束并完成稳定探测，才能建立 `resolution_revision`；
  该边界对应内容仍归 `Unknown`。kill/reap 不能独立证明 OS 服务没有待完成请求。
- 未知期间"每次用户复制都自动广播"与"所有自写都无回音"不可兼得。本产品选择：**保全可恢复内容、
  阻止不确定的自动回播、提供显式恢复。**

## 迁移与回退

- v12 的**读取能力**与是否允许 **v11→v12 迁移**是两个独立的值，由显式 `MigrationPolicy`
  （`LegacyOnly` / `CompositeAllowed`）贯穿打开、迁移与 degraded/reopen 路径。
- 关闭态打开 v11 库：保持 v11，不建 composite 表，不写升级前快照。
- 关闭态打开已有 v12 库：保持 v12 并读历史，禁止自动降到 v11，同时关闭 composite 的新采集与新投递。
- 新目的库承接既有 v12 历史：依据源库创建 v12 并完整迁移（保全数据），不解释为开启新采集。
- 升级前备份是**完整回退包**：SQLite 一致快照 + 所有存活外置图片/文件载荷 + 权限与摘要清单。
  备份失败或空间不足时**停止升级**。
- 回退步骤：停止写入并验证 quiescence → 另存升级后库与载荷 → 校验升级前快照 → 恢复到新目录 →
  用旧二进制逐项读取正文/图片/文件/收藏 → 再切换路径。
- **不得随历史快照回退 identity、trust 或 effect receipts**，否则旧回执丢失会导致重复执行系统写入。

## 工作与配额的生命周期

全局 FIFO permit 由实际 native worker 持有至退出；取消 async 等待者不提前释放它。撤销在运行中
传入 helper supervisor，kill/reap/join 后仍报告 Unknown。历史先准备，claim 与 history ID 同时提交，
保护该条目的去重、裁剪和存储迁移；正常完成或最后一个 worker 退出时释放保护。重启直接结算
prepared history；legacy ACK 只确认历史接收，不补造原生成功。两种兼容 JSON 从同一 journal 派生。

验证后的 wire/model 使用共享不可变对象；解码、装配、capture、原生准备、发送重试缓存和预览工作
显式申请配额。取消时 worker 的 lease 仍有效。256 MiB 是受管管线配额，不是整个 App/OS 的 RSS 保证。
缓存只保留确定结果，临时准入压力不进入不可变失败缓存；fan-out 使用同一初始截止。

RTFD 转换在有截止的 helper 中进行，只接受有界内嵌普通文件图片和可表示的格式属性，不解析外部
URL。成功投影保留原始 RTFD；不可表示的属性或附件保留 native-only，不以缺图的文本假称完整。
合成命名粘贴板验证不替代 Word、微信或 Windows 的真实来源/消费验收。

## 失败与可见性

- 采集失败只有在该次变更已进入历史、或已证明可用的 fallback 完成、或已持久化终态故障并显式告知后，
  才 `acknowledge` 观测到的 revision。无法读取的正文不能宣称已经保存。
- 永久拒绝（含装配预算压力、能力降级、不可降级的 RTF-only）必须产生用户可见告警，不能只写日志。
- 忙/准入压力尚未产生副作用时保留为**可重试**传输失败，不归类为永久拒绝。
- 告警是有界结构化队列，按 warning ID 确认；队列溢出有计数或聚合提示，永久终态仍可从关联记录查询。
- 错误以稳定错误码映射中英文本地化文案，不透传 Rust 英文串。

## 验证要求

- 证据分级：源码确认、单元测试、平台 adapter、包内 helper、完整 App、Windows 原生、双设备、
  目标应用**分别记录**；来源不完整时不计完整成功。本地全绿只能记为 `LOCAL_COMPLETE`。
- 必须覆盖：默认与启用两种构建、四种打开状态、完整迁移与回退、HTML bindings 边界与长度预估、
  图片解码预算、表示排列与 native 回读顺序、PNG 元数据/色彩/alpha、helper 在 clear 前/后/回执前的
  失败注入、未知关闭探测、128 条与 epoch 回绕、helper 退出/取消/管道拒写、逐边界崩溃、
  两个 JSON 的导入与损坏组合、同事件跨 session、非法交错与身份级配额、配额释放与时钟回退、
  多次重试的编码次数、旧端降级、慢接收中的新复制与撤销、17 文件与混合 items、
  ENOSPC 与共享资源删除、收藏/配额/degraded 恢复、双图与 native-only 各 API 入口、
  五种 UI 结果、多 warning 与多窗口、preview 大小语义与生成契约、Windows sequence=0/delayed rendering、
  原生键盘与 VoiceOver、以及 Iroh/完整 App/双设备/目标应用。
- 目标应用拒绝某格式时保留失败与剪贴板检查证据，判定是发布缺陷、转换缺陷还是消费者能力限制，
  不能都归为"对方应用问题"。

## 独立版本与封存 R6 证据

以下是封存 R6 时的版本：identity/base **4**；data frame **5**（认证连接可协商回退到 4）；main history **条件 v11/v12**；effect journal **1**；local contract **1**；helper protocol **2**；origin fingerprint **2**。v11→v12 受构建开关、运行时开关和 helper 健康控制；已有 v12 在关闭态可读取，承接已有 v12 数据的目的库保持至少 v12。版本变更须有独立决策，不能把这些编号统称为“协议版本”。

O11/O01/O02/O06 当前实现使用条件 history **v11/v12/v13**、Manifest **1/2**、CNA1 **1**、local contract **2**、error schema **1**、macOS helper **4**、Windows helper **2**。RestorePlan **1** 是内存操作模型；original origin fingerprint 使用独立域 **1**。已有版本的存储读取和 schema 12 真实 reader 回退继续单独验证。新记录的版本与证据入口见 [ADR-005](../adr/ADR-005-composite-content-identity.md) 和 [逐项执行记录](../COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md)；下述 R6 证据只认证其封存源码。

本地矩阵包括默认/开启/运行时关闭（固定 3 项）、完整 Iroh 与真实网络组、平台 host suites、Swift/React、前端 build/lint、双端 canonical includes 正负向、实际 diagnostics 与独立 scratch lint probe。`results.json` 不由命令名称推断测试数或 warning 数；raw logs 与退出码是输入。

故障矩阵区分生产接收器计数 adapter 和实际私有 board supervisor；不适用的 child/history 项写 null 并标明边界，不能推算为通过。包 manifest 将 debug、native/universal 两态 App、Windows portable/installer 分别登记。历史证据不能继承到新源码或新包。当前状态与完整证据入口见 [R6 核验](../COMPOSITE-CLIPBOARD-R6-VERIFICATION.md)。

## 显式保存公开图片（O04）

含尚未闭合 HTML 图片引用的历史行可手动选择“保存图片（联网下载公开图片）”。仅该动作联网，不携凭据，不访问内网、file 或 blob。当前只接受公开 HTTP(S) 静态 PNG：8 引用、8 MiB／单图、16 MiB 总读取额度，逐跳校验目标与实际连接；JPEG／GIF／WebP／SVG、CSS／srcset、entity URL 和登录态未支持。失败保留原件与引用，提示图片索引；不提供假完整恢复。

资源进入加密 Manifest **3** 派生层，原 HTML／CNA1、data_hash 和内容身份保持。再保存只补缺项，无新资源不增版本；不复制、不自动同步。明确恢复走 O06 RestorePlan，已提交资源关闭来源／离线后不依赖原 URL。旧 reader 对 v3 fail-closed，回滚须备份完整库／载荷，不静默丢层。

本机自动／public HTTPS／文件 DB 重开／私有命名粘贴板与完整 GUI／Word／QQ、Windows 原生／双设备分别验收；源码不代表已发布 App 包已更新。范围与证据见 [O04 报告](../COMPOSITE-CLIPBOARD-O04-RESOURCE-ARCHIVE-2026-10-06.md)。

## 可选浏览器来源文件桥（O05）

登录态／blob 的同源静态 PNG 可使用手动增强：历史行“授权浏览器来源”→选择新 ticket 文件→可选 Chromium 扩展加载文件并再次点击收集→下载 packet→同一历史行“导入来源资源包”。保存 ticket 不覆盖现有文件；packet 不是任意 URL 下载指令。拒绝授权、无扩展、跨 origin、srcset 改变／重复匹配、超时／超额或不支持的编码保留原件，仍显示未闭合限制。当前不处理 QQ 私有资源、任意登录网页或 CSS/srcset 依赖。

授权为 5 分钟／一次性／可撤销能力，不含长期 App token／Cookie／会话；请妥善保管并及时删除导出的授权和资源文件。资源 PNG 本身可能包含敏感内容，不会被正文脱敏。扩展只有 activeTab／scripting，不自动安装，不直接连接 daemon，不操作剪贴板。已有原 HTML 仍可能含来源 URL；本功能不承诺对既有原件进行脱敏。

资源存入加密 Manifest **4** 派生层；partial 可重新授权补缺，后续公开补图保留已存来源证明，原 HTML／CNA1／hash／内容身份／收藏保持。恢复继续走 O06，已完整归档可离线准备；来源 scope 证明不等同网页签名认证。真实浏览器／页面／文件对话框与设备验收仍 UNKNOWN；本机测试不能代表当前运行 App 已更新。见 [O05 报告](../COMPOSITE-CLIPBOARD-O05-SOURCE-CONTEXT-2026-10-06.md)。
