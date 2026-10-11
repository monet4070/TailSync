# ADR-005：剪贴板原件身份、去重和版本兼容

- 日期：2026-10-06，Asia/Shanghai。
- 状态：O11-D1 设计完成；O11-D2 已接入生产存储、重试和条件 v13 升级。本机验证及未完成边界见第 8 节；O01 已接入 macOS CNA1 原件/派生采集接口，见第 9 节。
- 依据：[逐项方案](../COMPOSITE-CLIPBOARD-IMPROVEMENT-PLAN-2026-10-06.md)、[S0 复现与执行记录](../COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md)。
- 上游：[ADR-004](ADR-004-composite-clipboard-contracts.md) 的 K1—K10 全部保留。本决策新增历史身份，不修改 WireDigest、OriginFingerprintV2 或 EventReceiptDigest 的算法。

## 1. 问题与本项范围

S0 用真实包 helper 和当前 Core 复现：两份 RTFD 原件的附件文件名不同，生成的便携内容相同；当前 v12 库按便携摘要去重，三次均只留下后一条。扩大原始格式保留之前必须解决这个身份混用问题。

当前 data_hash 还有两个消费者：get_composite 用它校验便携内容；Swift/React 的显式恢复重试用它匹配被替换的记录。因此不能直接把该列改成“完整原件 hash”，也不能继续让便携相同代表原件相同。

O11-D1 交付是确定字段、摘要规则、版本升级和回退契约，并用 S0 样本验证规则模型。第 2—7 节保留该设计检查点；第 8 节记录 O11-D2 的实际 DB、存储与重试接线、S0 B02 修复证据及尚未完成的范围。

## 2. 身份与摘要各自负责什么

| 名称 | 范围与用途 | 何时变化 |
|---|---|---|
| LocalCaptureId | 一次允许入库的稳定采集的随机 128-bit 身份；External 或需保留内容的 Unknown 均有，SelfWrite 不新建 | 用户产生新的复制事件；同内容的新 revision 仍是新 capture |
| RemoteEventKey | 已认证来源身份＋MessageId；接收去重、journal 和回执 | 来源发起新事件；重传不变化 |
| WireDigest / EventReceiptDigest | 当前网络包/回执的既有校验契约 | 按既有包编码规则变化；不由本决策重新计算旧值 |
| OriginFingerprintV2 | 当前平台实际发布/回读的来源分类 | 按 ADR-004 规范化；只供来源分类，不供历史去重 |
| OriginalContentIdV1 | 来源平台、项目边界、原始类型名称、原始字节及读取事实 | 原件集合变化；派生、revision 和自写 token 不影响它 |
| DedupeKeyV1 | 完整原件身份＋采集当时已证实闭合的依赖内容 | 原件或依赖内容变化；未满足完整条件时没有此键 |
| ArchiveRevision / ArchiveDigest | 当前资源、兼容表示和派生绑定的归档版本 | 资源补齐或派生集合提交；不创建新 capture，不改变原件身份 |
| history.id | 当前数据库行的定位符 | 当前重复条目置顶/替换仍可能换行；不能代替原件身份 |

“内容相同”不能阻止 External 的新事件广播或更新复制时间；RemoteEventKey 的重传去重与历史内容去重始终是两条独立链路。

## 3. 原件身份的字节规则

### 3.1 原生内容

使用 BLAKE3 的域隔离模式，context 固定为 `TailSync.history.original.v1`。规范输入按顺序包含：

1. 来源平台 tag（Unknown=0、macOS=1、Windows=2）及 provenance tag（Exact=1、LegacyProjection=2）。
2. UInt16 BE 的项目数，按 item index 从 0 顺序处理；空项目边界不省略。
3. 每个项目的 index 和原始格式数；项目内保留来源格式优先顺序。不同项目记录在 IPC 中的交错排列不改变身份。
4. 每个格式的 UTF-8 类型名称、读取状态和原始字节。可变字段用 UInt64 BE 长度前缀；状态用一个 byte。

排除且只排除 TailSync 自身的 `org.tailsync.clipboard-source` 与 `org.tailsync.rtfd-converted` 控制格式，以及系统 before/after revision。派生 HTML/text/PNG 不进入原始集合，不需要依赖“删名字”判断一般派生内容。

原始名称、文字空白、编码元数据、附件文件名、项目顺序、项目内格式优先顺序都保留。宁可保留两条看起来相同的记录，也不为减少记录数规范化掉原件差异。来源分类的顺序/像素规范化规则仍只属于 OriginFingerprintV2。

新 helper 必须明确提供 advertised 顺序、读取状态和原件/派生角色；旧 CNS1 按白名单输出的顺序不能冒充完整的来源优先顺序。新采集 metadata 无法证明完整枚举时，不产生可用于删除旧记录的去重键。

Exact 表示来源角色和已取得的字节没有被派生替换，不等于全部格式读取成功；读取状态和 enumeration_complete 仍独立判定，部分读取不能生成安全去重键。Unknown 来源也不能因内容摘要匹配而升级成 External。

外部表示为带类型前缀的文本：`hcn1:<platform>:<hex>`；旧采集投影用独立 `hcl1:<platform>:<hex>`。数据库保存 kind/version/platform/digest 分列，比较时必须包含这些维度。

### 3.2 只有便携表示

收到旧复合协议的 PortableBundle 时，没有原始平台集合。以现有严格 CBP1 编码的字节作为输入，域为 `TailSync.history.portable.v1`，得到 `hcp1:<hex>`。

它不能与原生 ContentId 匹配。便携内容完整且依赖闭合时，以独立域 `TailSync.history.portable-dedupe.v1` 生成 `hpd1` 键；ExternalReferences/MissingFormats 或依赖未知均不可内容去重。

WireDigest 仍是现有 bundle.digest，不改为上述带域摘要；ACK、effect journal 和旧事件的摘要保持原值。

### 3.3 何时允许内容去重

原生 DedupeKey 使用域 `TailSync.history.dedupe.v1`，包含带类型的 OriginalContentId 及依赖闭合清单。仅当以下事实全部成立才生成：

- 来源平台已知，provenance 为新协议证实的 Exact。
- 原始格式枚举完整，各原始格式均 Captured，且每个项目具有原始内容。
- 资源依赖已在采集时证实闭合；对只含内嵌字节的已知格式，空依赖清单可以是闭合清单。
- 文件 URL、外链、对象/provider token、未证明可持久化的不透明类型，不能仅按路径或字符串判闭合。

依赖清单包含表示/节点定位、MIME 和实际资源字节身份；按定位排序做无歧义长度编码。同一定位重复出现应拒绝，正文多次使用相同资源以不同定位表达。同 URL 的内容改变会产生不同键。

原件含同一 URL，不意味着两次复制的图片相同。无法在采集时取得字节就保留两次事件；后续补图不回溯删除其中一条，不重写采集时的去重键。

已知完整的相同集合可继续采用现有重复记录置顶策略，但只删除相同 DedupeKey 的非收藏、非 protected、可安全替换记录。查询还要限定 history.type=composite 和键版本；不再调用不分内容域的 entry_ids_by_hash 来选择复合旧记录。

## 4. data_hash、重试与迟到任务

### 4.1 保留 data_hash 的旧含义

- 所有旧行原值保留，不做就地重算或给旧值加前缀。
- 新行仍按已定义的兼容校验视图计算：便携行用 CBP1 bundle.digest；native-only 用 CNS1 原生集合的既有 native_digest。Manifest v2 明确写出 integrity_method 和对应部分。
- 新原件完整性另外通过 ContentId、每个 part 的 digest/长度及 Manifest 校验，不能以便携 data_hash 校验替代。
- history_composite_events.digest、event_receipts、effect journal 的既有事件摘要不改写。

### 4.2 恢复重试

新契约提供隐藏的 HistoryIdentity：kind/version/content digest、capture ID、可选 DedupeKey、ArchiveRevision。它服务恢复，不增加日常状态标签。

1. 原行还在：检查原件身份/capture 一致并重新准备当前内容；payload 绑定实际 ArchiveRevision。
2. 原行已被内容去重替换：只有双方非空、同域同版本且相同的 DedupeKey，才允许重绑定。格式可恢复能力重新检查。
3. 旧记录或不完整内容没有安全键：禁止仅凭相同 data_hash 跨行重绑定。原行已删时返回 not_found；用户另选存在的条目创建新意图。
4. Unknown/Partial 不自动再次写入。用户点击“再次复制”是新 intent，继续经过准备、claim、授权、generation 和原生结果边界。

### 4.3 ArchiveRevision 的原子提交

资源/预览任务绑定 `(history.id, capture_id, OriginalContentId, expected ArchiveRevision)`。提交事务中核对行存在、未删除、身份一致和版本相同，成功后递增版本。

迟到、被删除、版本冲突、换原件或错 capture 的结果全部拒绝，不复活行，不更新新选中项，不写系统剪贴板。新版本准备失败保留旧可用版本；预览/RestorePlan 缓存以行＋原件＋归档版本为键，不能仅按 history.id 缓存。

归档版本、持久资源和原件必须相互验证；SQL 已提交再清理旧版本文件，活动 preview/restore/clipboard lease 释放之前不删除仍被依赖的资源。

## 5. 存储和版本演进

### 5.1 预留的版本

以下是 O11-D1 完成时的版本设计记录；O11-D2 实际接线后的版本和分阶段准入见第 8 节。

| 面 | 当前 | 后续实现目标 |
|---|---|---|
| 主历史 schema | 条件 v11/v12 | v13 增加原件身份/归档版本；v11/v12 仍可读 |
| 加密 Composite Manifest | 1 | 2；读取器分派 1/2，不把 2 塞给旧 reader |
| helper 健康协议 | CWH1 protocol=2 | protocol=3，证明新的 archive read 接口 |
| 新原件/派生采集 IPC | 无 | 独立 CNA1/version=1、`--read-archive`；有界且带角色/provenance |
| 现有原生发布 IPC | CNS1/version=1 | 保留；新归档选择发布集合后生成 CNS1，准入规则在 O02 扩展 |
| 本地契约 | 1 | 2，统一生成 Rust schema、Swift、TS/decoder；新重试身份不得由旧客户端忽略 |
| 回退包 | pre-v12 / Manifest 1 | 独立 pre-v13 / Manifest 2，原 pre-v12 包保留 |
| identity/base、data frame、CBP1、网络消息、journal、OriginFingerprint | 4、5、CBP1、既有消息、1、2 | 本阶段全部保持；O07 新原生网络载荷另行协商 |

选择新增 CNA1 接口，是为了不把复合归档的两套角色塞进原有 CNS1 发布格式。新 GUI/daemon/helper 必须同包配套；旧 helper 的 protocol=2 不证明 Exact archive 采集可用。

### 5.2 v13 的增量结构

新增 `history_composite_identity`，history_id 是 PK/FK ON DELETE CASCADE：

| 字段 | 约束 |
|---|---|
| capture_id | 128-bit，本行稳定且唯一；与用户新复制事件区分 |
| content_kind / identity_version / source_platform | 明确摘要域，不能只比较裸 hex |
| content_digest | 32 bytes；旧内容可在有界验证后补齐 Legacy 身份 |
| provenance / enumeration_complete | Exact、LegacyProjection 或 PortableReceived；完整性是事实，不是 UI 标签 |
| dedupe_kind / dedupe_version / dedupe_digest | 可空；仅采集时证明闭合后设置，不因迟到归档变更 |
| archive_revision / archive_digest | UInt64 范围内递增；Manifest 与关系表一致 |

建立非唯一 dedupe 查询索引；收藏/protected 条目允许相同内容并存，不能加全局 UNIQUE 强行合并。

沿用 history_parts 持久载荷，Manifest v2 明确 `original / portable / derived / resource` 角色、item/type/priority/read status、来源部分和转换器版本、资源绑定及摘要。旧 Manifest 1 不就地覆盖；新 reader 用版本分派决定原件和兼容校验视图。

不需要新增网络消息号码。LocalCaptureId 不作为信任凭据；远程幂等仍以已认证来源和 MessageId 为准，不能以 ContentId 替代认证或碰撞校验。

### 5.3 迁移与关闭态

1. v12→v13 前保存一致数据库快照和全部 live 外置 payload，并完成文件/目录持久化及独立验证；空间不足或快照失败则停止升级。
2. 新表、约束及 schema=13 marker 事务提交。旧行的 data_hash、Manifest 1、part 字节、收藏和事件映射保留；旧格式枚举/字节无法补回，标 LegacyProjection，不标 Exact。
3. 旧 ContentId 可有界补齐，未验证时不参与新内容去重；不能启动时无界解密整个历史库。
4. 把读能力、身份存储能力和 Exact archive 写入资格分开。O11-D2 的 v13/Manifest 2 可以保存 protocol=2 取得的 LegacyProjection，并明确不给原生去重键；新 reader、功能开启和健康 helper 仍是升级前提。只有 O01 的 protocol=3 完整采集事实才可授权 Exact 身份及原生去重，不能因新增 schema 自动晋升保真能力。
5. 关闭新采集时仍读取已有 v13，不自动降库；迁移存储根时目的 schema 至少达到来源版本，包含 degraded pending 历史及全部新引用。
6. 不能只把 SCHEMA_VERSION 从 12 改成 13：当前 open/new_unavailable 中的 `target >= SCHEMA_VERSION` 还承担创建 v12 表的职责，须拆为具体 schema 能力，避免关闭态/旧写入丢掉表。

### 5.4 回退

升级后的 v13 数据和所有必要引用先归档保全，再到新目录恢复 pre-v13 的 v12 快照及载荷。旧二进制不打开 v13；新 reader 验证归档保全，实际旧 reader 验证恢复目录。

pre-v12 与 pre-v13 分别保存，不能复用固定旧文件名覆盖原回退包。恢复脚本验证对应 manifest 版本、路径、摘要、权限和完整文件树。身份、信任、配对与 effect journal 不随历史回退而倒退；未知系统副作用不由恢复动作重放。

迁移、删除、收藏、trim、共享资源、pending 接收/归档、degraded 搬迁、当前 clipboard/preview/restore lease 必须覆盖新角色与引用。配额同时计原件和派生数据；关功能不清库。

## 6. 可执行规则验证与当前边界

模型位置：`artifacts/clipboard-improvement/2026-10-06-o11-design/`。它调用当前 Core 的 NativeSnapshot 解码与 from_native，使用 S0 保存的真实 helper 合成结果；新身份与提交规则是树外运行的参考模型。

重点验证：S0 不同 RTFD 原件仍生成不同安全键；revision/token 不改变原件；项目边界和格式优先顺序保留；Legacy/未知来源/缺失格式/未知依赖不可删除旧记录；同 URL 不同资源字节不合并；补图不改变 capture/原件/旧 hash；迟到、删除、错误 capture 与版本冲突被拒绝；旧 retry 不凭便携 hash 跨行匹配；WireDigest 保持原值。

模型通过不等于 DB/GUI 修复。O11-D1 当时未改 helper、数据库或 generated contracts，未执行真实迁移/断电/回退，也未访问系统剪贴板。O11-D2 的迁移/回退接缝结果见第 8 节；A09/A14/A21 的完整验收仍开放。

## 7. O11-D2 的完成门槛

- 把本模型规则移到有明确类型的 Core seam，接入 v13 DB、Manifest 1/2 读取和新契约重试；按阶段准入，不提前把 v12 投影标为 Exact。
- 先保留现有合法部分，再让 S0 B02 的三次生产 DB 重现由“1 条”转为“2 条”；原件不同、同便携内容、收藏/protected 等实测覆盖。
- 同原件重复仍按产品规则置顶；External 新 revision 不被内容去重吞掉广播，RemoteEventKey 重传不重复写系统。
- 实际 v12 reader、新 reader、满盘/中断和完整回退目录有独立证据；旧 data_hash 与网络/journal 摘要不被迁移改写。
- Swift/React 重试接缝红绿验证，格式选择保留，删除/不同原件/未闭合依赖不能误绑定；helper、契约、包和完整矩阵按变更范围重新绑定。

O11-D1 完成后按用户要求暂停，O11-D2 的后续实施见第 8 节；O01 仍作为下一项独立任务。

## 8. O11-D2：实际接线与分阶段边界

2026-10-06。本项在同一 composite-clipboard 工作树执行，保留所有既有未提交改动和测试包。

### 8.1 已实现的接缝

- Core `composite/identity.rs` 提供独立原件投影/便携内容域、采集身份和重试判断。WireDigest、EventReceiptDigest、OriginFingerprint 和旧 data_hash 的算法不变。
- 条件 schema v13 新增身份关系表；同一个事务写入 Manifest 2、parts、身份及事件映射。读取器分派 Manifest 1/2，核验归档摘要、身份关系、部分字节和内容身份。旧行不批量解密、不重写 Manifest 1，不补造 Exact 事实。
- 已知闭合的便携条目按 hpd1 键置顶；仅查询 composite 类型。收藏和 protected 条目保留。当前便携闭合证明保守限定于完整 text/PNG 及无属性的已知 HTML 格式标签；RTF、图片 HTML、任意属性/对象/未知依赖不参与删除式内容去重，等待 O04 的资源闭合证明。
- 当前 CNS1 仍是 LegacyProjection，保存的原件集合具有 hcl1 身份但**没有原生安全去重键**。同一次远程事件重传仍经 RemoteEventKey 幂等；新外部复制继续采集、保存与广播。当前重复原生采集可能保留多行，这是完整枚举尚未实现时的保全策略；原生安全置顶等待 O01。
- degraded carry-over 按 capture_id 搬运复合记录；Manifest 1 按已保存加密 manifest 精确匹配，不能按便携 hash 合并。目的库最低版本达到来源版本；搬迁不在维护锁内触发 schema 升级。
- 归档摘要更新提供实际数据库 CAS 接缝：capture、原件、原归档摘要和版本匹配才原子提交；迟到、删后、错 capture、错原件被拒绝。当前该接缝只更新有界摘要，资源载荷替换、lease 和派生绑定仍属 O03/O04/O08。
- local contract=2，由 Rust 一次生成 Swift/TS/decoder/fixtures；stable error contract 仍为 1。身份字段隐藏于 composite metadata。Swift/React 跨行“再次复制”不再使用 data_hash，持有的身份由守护进程在准备 payload 的同一 DB 锁内复核。原行仍在时允许重新准备当前 archive revision；格式选择和显式新 intent 保留。
- pre-v13 Manifest 2 回退包包括一致 v12 SQLite 快照和 live composite 外置载荷，显式声明 BLAKE3 文件摘要算法（sha256 是旧字段名）。验证版本、路径、尺寸、摘要、权限和完整文件清单。pre-v12 保持独立目录和 Manifest 1。`restore-pre-v13-rollback.sh` 只恢复到新目录，升级后历史单独保全。

### 8.2 验证与仍开放的边界

生产回归覆盖不同原件/相同便携内容、关闭重开、收藏/protected、未知依赖、CAS、删后迟到、degraded 幂等搬运、旧记录字节保留、真实 SQLite SQLITE_FULL、事务失败重试和回退包损坏。S0 的 B02 原始 helper 合成结果三次重放都保留两条。

独立 v12 生产源码读取器创建旧库，v13 writer 升级并保全新增记录，再让旧 reader 读取新目录；原件摘要/旧 data_hash/收藏一致。遗漏外置载荷和让旧 reader 打开 v13 的两条负向验证均拒绝。

本机详细结果和源指纹登记在逐项执行记录及 `artifacts/clipboard-improvement/2026-10-06-o11-storage/`。原始失败和修复过程日志保留。源变化后原 R6/R7 封存证据不继承；本项不替换封存 App。

最终 DB 135 PASS、标准 enabled Core 623 PASS、Runtime 36 PASS、macOS Rust 117 PASS、Windows Rust/macOS 宿主 126 PASS、Swift 265 PASS/4 skipped、Windows UI 275 PASS，Clippy/契约/独立回退通过。ignored、各次检查时间及源码绑定范围见执行记录；此前 all-features 串行的两项 Iroh 超时与既有两项 production-feature 声明违规仍作为失败保留，不能宣称全仓门禁通过。

O01 的 CNA1/protocol=3、原件/派生分离和 Exact 采集；真实应用/QQ、Windows 原生、设备同步、安装包、真实断电/物理磁盘耗尽及资源任务生命周期均未由本项验证。SQLite 页数上限产生的 SQLITE_FULL 是真实数据库失败证据，不等同物理磁盘耗尽验收。Release 继续 NO-GO。

O11-D2 本机实现与本项自动验证完成，按用户约定暂停；下一项 O01 尚未启动。


## 9. O01：CNA1 原件与派生角色

2026-10-06。macOS helper protocol 提升到 **3**；Windows 保持 **2**。CNA1/version 1 是本地采集归档，不是网络帧或 OS 发布请求。CNS1/version 1 发布入口继续保留；旧 helper 的健康探测按 expected protocol 拒绝，而非尝试解析新帧。

### 9.1 采集、存储与默认恢复

- `NativeArchive` 保留来源平台、advertised item/type 顺序、before/after revision、完整枚举标志及逐格式读取状态/原字节。当前 helper 仍只读取既有允许类型；其他 advertised 类型以 Absent 表示尚未尝试，O02 负责扩大准入。Exact 描述原件角色没有被转换覆盖，不证明完整读取或资源依赖闭合。
- RTFD→HTML/text 与 TIFF→PNG 是独立 `DerivedFormat`，包含 item、源类型和转换器版本。转换读取已经捕获的字节；RTFD 派生对按容量原子准入。不稳定采集丢弃派生结果，不能新入库。原 HTML/text/TIFF/RTFD 永远不被转换结果替换。
- Core 的便携投影只使用已知的转换器/源类型组合。来源自带的 TailSync 控制类型不是转换成功证明；投影只在派生对存在时加入内部标志。投影验证失败时保留合法 archive 为 native-only。
- Manifest 2 新增可选 `archive_version=1`；旧 Manifest 1/2 缺省字段时的字节序列化与摘要保持原规则，不批量升级旧行。一个加密 `history_parts` 的 archive 角色保存完整 CNA1，portable representation/resource roles 继续独立保存。读取核验 archive 角色/版本、密文载荷、内容 ID 与去重键；沿用配额、删除和外置载荷 GC。
- 新原件默认恢复只选择原始格式，移除两个 TailSync 控制类型后由 publisher 安装新 token。显式 text/html/rtf/png 使用已验证兼容投影，不能修改 archive 或下一次默认恢复。读取失败/超限/未尝试描述仍保留到准备阶段；枚举不完整或任何原件未捕获时，共用 runtime 在发布前返回 `originals_incomplete`，有效的显式兼容动作仍可使用。新 archive 在平台不支持原集合时不自动改为派生默认恢复。旧记录保留原兼容行为。
- `originals_incomplete` 是 additive stable error code；error schema 仍为 1、local contract 仍为 2。Rust policy 统一生成 Swift/TS/decoder，并连接 HTTP/Tauri/Swift 的本地化映射。

### 9.2 身份、来源分类与有界成本

新原件 ID 使用 hcn1 域；保留原字节、名称、顺序及读取事实，排除 revision、派生和两个控制类型。完整枚举与读取、已知平台及 text/PNG/无属性已知 HTML 的闭合证明才准入 hnd1 键。RTF/RTFD/TIFF、未知类型/依赖仍没有安全原生键。采集完整与依赖闭合是两个独立事实。

旧 OriginFingerprintV2 便携算法不变。原始 RTFD/TIFF/已知多项目集合以前没有可发布指纹，O01 增加独立域 `tailsync-origin-native-original-v1`：去控制类型/修订、按 item/type 排序后比较精确原始 bytes。仅对稳定、完整取得且准入的原件集合使用；Unknown/incomplete 不伪造匹配。生产读取在有派生输出时按原件分类，避免把转换输出与发布原集合混用。

CNA1 保持原件＋派生总共 ≤64 格式、16 项目、40 MiB；历史总预算仍为 72 MiB。macOS 捕获 reservation 从 204 提升到 240 MiB，**全局 256 MiB 上限没有提高**。转换器失败/预算不足不删除原件；这不等于所有原生格式或任意大内容都已支持。

### 9.3 证据范围

当前源码、命令退出码、失败日志、新包与私有 board 证明见逐项执行记录第 9 节及 `artifacts/clipboard-improvement/2026-10-06-o01-originals/`。历史第 8 节的检查点保持不变，不能用 O11/R6 的旧 package/hash 认证 O01。七组真实 helper 合成采集针对原 HTML/text/RTFD 及 TIFF 的逐字节保留、DB 重开和原集合回放；完整 GUI、来源/目标应用、Windows 原生和双设备验收仍开放。O02 未启动，Release NO-GO。


## 10. O02：原生字节准入与敏感采集阻止

### 10.1 共享规则与兼容性

`shared/schema/native-capture-policy.json` 是 policy 1 的源；生成器写入 Rust 常量和 Swift helper，`--check` 与负向测试进入 CI。macOS helper protocol 从 3 升至 4，Windows 保持 2；旧 helper 由启动前 resolver 拒绝。CNA1 仍为 version 1，新增 metadata-only Suppressed=6（空项目/格式且稳定 revision）；该帧不存历史，helper 4 版本检查隔离旧 codec。main history 13、Manifest 2、local contract 2、便携 origin fingerprint 2 和原件独立域 1 均保持。

未知合法 UTF-8 名称（非空、≤256 bytes、无 ASCII 控制字节）以 opaque Data 捕获，单格式 ≤4 MiB；text/HTML/RTF/RTFD/PNG/TIFF 既有额度保持。常用表示先读取，选中的描述按 OS 原始顺序保存。16 项目、64 原件与派生格式合计、40 MiB CNA1、5 秒监督期限、240 MiB reservation/256 MiB 管线总预算不提高。超限/失败/未读取描述与枚举不完整事实保留；默认原件恢复继续拒绝不完整集合。

四种精确标记 org.nspasteboard.TransientType、org.nspasteboard.ConcealedType、org.nspasteboard.AutoGeneratedType、com.agilebits.onepassword 在所有 items 元数据中预查，先于项目限制和任何正文请求。CNA1/CNS1/旧 text、image、file 入口均阻止正文；监听确认该 revision 后不创建历史、广播或进行 fallback 正文读取。DB archive/legacy 入口在摘要或转换前拒绝，内层存储门再拒绝。

纯文件选择继续走文件管线；混合 public.file-url、NSFilenamesPboardType 及 file promise 名称中的策略项只保留 Absent 描述，当前规则不读取或发布。准确名单以 policy 1 为准。不透明字节不被任意反序列化/执行，不抓网络或外部文件；未知格式不能因此获得依赖闭合、所有目标应用可用或安全去重证明。

### 10.2 原件来源分类

只要集合含已知投影范围之外的名称，就对整个原件集合使用独立 original origin 域，不能因为另有 text 便退回只比较便携投影。准入由共享 macOS byte policy 决定；未知类型变化会改变 SelfWrite 比较，未知/未闭合内容的 history dedupe key 仍为空。Windows 的原生发布规则与 O09 范围保持。

### 10.3 证据边界

执行记录第 10 节与 `artifacts/clipboard-improvement/2026-10-06-o02-admission/` 保存真实 RED、最终测试、全新 helper 4 包和私有 provider/DB/回放证据。O01 的封存源码、包与失败日志保持历史记录。合成原生验证不认证 QQ/TextEdit 等实际来源/目标、完整 GUI、Windows 原生、双设备或发布资格；Release NO-GO。本项完成后暂停，O06/O10/O03 未启动。

## 11. O06：不可变 RestorePlan 与单次发布

`RestorePlan` version 1 是一次操作持有的内存计划，不是新增存储或网络载荷。输入保留实际参与选择的来源平台与请求格式；字段与输出字节由计划独占，准备成功后只能只读检查，不能在 FIFO/worker 边界重选格式。2026-10-06 审查整改删除了没有消费者的 HistoryIdentity 副本、来源归档版本与 `TailSync.restore.input.v1` 摘要；历史身份、归档版本和部件完整性仍在 DB 加载接缝验证，计划本身没有额外的摘要比对门。输出诊断清单包含 item、类型名称、长度和 BLAKE3 摘要，检查时按需生成，正常发布不分配这份清单或计算这些诊断摘要。

选择规则：同平台完整、可发布且无已知外部 HTML 依赖的集合选择 originals；已有精确资源绑定的 HTML 选择资源物化后的兼容输出；异平台只选择完整便携表示，不携带来源私有类型；显式 text/HTML/RTF/PNG 选择独立投影，不修改原件或之后的默认计划。原件未完整取得仍返回 originals_incomplete。已知缺资源返回 resources_incomplete；不透明格式的字节回放保留 O02 的边界，不能据此宣称任意未知格式的语义依赖闭合。

已知 HTML 资源必须与 img/src 的精确字节跨度、resource ID 和真实 PNG 字节相符。相邻 srcset、CSS URL、poster、嵌入对象等不能借用一个图片绑定的证明；source-inline PNG 同时计入像素和数量预算并在准备阶段解码。不会访问网络、file 或 blob。列表用有界、校验摘要的 HTML inventory 在 owned read worker 上判定，损坏/不可读 HTML 不再提供恢复操作；存储 manifest、原始完整性事实和身份均不因此改写。

采集与恢复共用有界词法 inventory：注释不贡献标签，script/style 的 raw text 不产生图片绑定，每一个真实 img 节点独立检查 src，不能用另一张合法图片掩盖缺来源节点。实际 script/object/iframe/embed/base、CSS URL 与外部属性依赖继续拒绝；普通正文中的 URL 字样不会与无 URL 的 style 标签拼接成外部依赖。Manifest 菜单与完整载荷准备共用 `RestoreFormatInventory` 的 native-only、HTML 完整性和单图选择规则；列表的引用校验与发布的像素/平台准入校验仍是不同强度的门。Runtime `restore_capabilities` 是完整载荷的回归预言机，生产菜单来自 `CompositeInfo`，由跨层测试守住一致性。

macOS 输出保持原 item 边界；兼容 HTML 的图像以内嵌字节保持节点顺序。Windows 输出提前形成 UTF-16 text、CF_HTML 的 UTF-8 偏移及 helper 原有 PNG/CF_DIBV5 组合，也提前补齐 HTML/RTF 必要终止字节和验证 DIB。输出仍受 CNS1/policy 的既有额度限制，不增加 CF_DIB。操作 token 是 helper 安装的控制字段，不属于应用输出清单。

本项所有输出都是 owned inline bytes，因此准备后的 DB 正常删除/关闭不会破坏计划，无需临时文件 lease。O04/O08 如引入文件 materializer，必须在当前剪贴板依赖期持有单独 lease，不能把本项的无文件证明推广到未来文件输出。公开 URL 归档、RTFD 兼容物化、Windows 带图 RTF 与真实目标适配仍是后续工作。

历史异步恢复先在短 engine lock 下登记新 intent generation 和 OS revision；准备在 blocking worker 进行，成功后取得 publication permit，再检查 generation，并移动同一个计划到 ClipboardPort。Mac/Windows adapter 以捕获的 revision 和 cancellation 调用现有 supervisor/helper、origin journal 与 typed outcome。取消 waiter 后，阻塞 worker 继续持有输出、memory 和 FIFO，直到进程/worker 静止；Partial/Unknown 不自动重试，也不转换为 Published。

自动接收同样在持久 claim 之前准备计划，保留现有授权、generation、operation permit、删除 tombstone 和 ACK 规则。合法但不可完整恢复的包仍保存历史并返回 StoredIncomplete；发送方的 Complete 标签不能绕过实际依赖检查。准备结果是发布事实，不能证明未来粘贴目标的选择或显示。

本项不增加持久/IPC 版本：history 13、Manifest 1/2、CNA1 1、local contract 2、error schema 1、macOS helper 4、Windows helper 2、原件 origin domain 1 保持。resources_incomplete 是已生成客户端契约中的新增稳定原因。执行与验证记录见第 11 节及 `artifacts/clipboard-improvement/2026-10-06-o06-restore-plan/`；真实 GUI、Windows、来源/目标与双设备证据继续独立登记。

## 12. O10：采集因果与结果事实

本地采集 worker 的 before/after 必须对应调度时的 observed_revision，完成时还需核对当前 OS revision 与 wake generation；不把另一条稳定快照绑定到旧重试。来源分类先于 MissingFormats 重试及内容去重，SelfWrite 终结观察，外部/不明观察使旧 generation 失效。历史保存后可以保留已完成的旧事件，但自动发送前仍需当前 revision/generation；旧失败不能重读已经被替换的剪贴板，也不能重建新观察的 retry 预算。

读取、保存和转换是不同事实。稳定原始字节可在便携转换失败或部分 provider 失败时进入原生历史；Failed/TooLarge/MissingFormats 仍产生读取不完整提示。CNA1 精确归档继续保留已捕获字节与未读格式事实；旧 CNS1 的原生专用保存如未保留任何原件，或过滤掉任一已捕获非控制格式，则在提交前拒绝，不能只保存摘要就宣称成功。本项不扩大 Windows 原生格式白名单。

保存成功只因不支持便携转换时不报 capture_failed。共用结果函数记录保存、格式准入、便携转换和已归档资源数等无正文事实；本机整组恢复资格由现有 CompositeInfo/RestorePlan 决定，格式准入及资源数不能取代完整性检查。来源不明与同步准备失败分别使用 capture_origin_unknown/capture_not_shared，存储拒绝使用 capture_not_saved，真正读取失败及未知写入保持各自类型。通知继续使用既有 peek/ack，手动 Unknown/Partial 恢复是新 intent，身份重绑定规则不变。

自动接收在原生准备前检查授权、generation 与 revision；撤销事件拒绝，过期完整事件仅保存并回 Superseded，之后的发布前准入、claim-before-I/O 和 typed outcomes 仍执行。现有版本 CAS 只证明历史摘要迟到更新的拒绝边界；O04 的资源下载/载荷提交及未来文件 lease 仍未实现，不可据此宣称资源生命周期验收通过。

本项无持久、helper、网络或本地契约版本变更；新增通知使用已有字符串 kind。执行和自动验收见改进执行记录第 13 节；真实 GUI/来源/目标、Windows 原生及双设备继续独立登记。


## 13. 全阶段审查后的准入澄清与接缝整改

O10 的完成时 revision/wake 检查保留：worker 尚未通过此检查时，只是临时观察；已被替换的结果丢弃，未承诺每次 OS 复制都入历史。通过检查后开始保存的旧事件可以留在历史，但迟到结果不能重新 supersede 新 intent、ACK 新 revision 或花费新复制的重试预算。

自动广播在设备发现、路由/信任锁和队列 capacity 等待结束后再核对 generation、原生 revision 与 wake generation；短 engine guard 持有至同步入队，背压不持有该锁。这关闭进程内可观测的等待竞态，不把本机自动回归当作真实 OS/双设备原子性证明。

完成采集与 history worker 后消费 snapshot/portable scratch，以不可变 CapturedBroadcast 保持编码正文与剩余 admission；临时采集预算缩减后才申请独立 SharedEvent 正文预算。全局 256 MiB 不提高，真实并发压力仍可拒绝。

已知 HTML 依赖统一检查 CSS import/escape/entity 及 SVG href/xlink:href；缺少闭合证明时保守 withheld。literal 格式 CSS 和导航链接仍可用，原始归档字节保持不变。legacy CNS1 存储与 CNA1/macOS opaque publication 的范围差异明确保留；Windows 名称资格共用一个策略函数，Win32 ID 映射及解码仍属平台准备。

接收侧存储失败使用 receive_not_saved(peer)，本机采集失败继续 capture_not_saved；两端双语提示区分操作设备。未使用的三条发布捷径删除，实际 RestorePlan 与 admitted 发布路径保留。版本与 Release NO-GO 边界不变；证据见改进执行记录第 14 节。

## 14. O04：派生资源层与提交身份

原 HTML／CNA1／base parts 与 bindings 不改写。Manifest 3 的单一 `archive_layer` 保存严格可证明的派生 CBP1，尾部可选 `archived_info` 记录派生格式／数量／完整性与预览尺寸。只增加 archive_revision／更新 archive_digest；data_hash、capture/content/dedupe、原件域、source 与 favorite 保持。v2 字段次序／缺省序列化／冻结 digest 不变，新语义不得伪装为 v2，旧 reader fail-closed。

URL 是位置，不是身份。新捕获同 URL 可得到不同 PNG，不同 URL 同字节去重；已有层只补缺项，0 新资源不增版本，不自动刷新线上图片。read／commit 均验证派生，不允许合法 identity 替换另一篇文稿或擦除 MissingFormats。

下载不持 DB 锁。当前 authority 下的 SQL CAS 覆盖 version／capture_id／content_id／dedupe_key（含 NULL）／archive_revision／archive_digest，并核对原加密 manifest 与 affected rows。删除／supersede／迁移或重开拒绝迟到结果，正常新 copy 不误失效。取消覆盖 admission／DB-lock 等待，payload 准备后事务前再查；事务开始后保持原子提交。

inline／external 都先检查非驱逐式 quota／free-space reservation，不 trim 原记录／收藏。prepared 外部载荷由 pending guard 回滚清理，提交后再回收旧 layer。SQL trigger／错误 CAS／删除与 GC／真实文件 DB 重开／Runtime reopen-during-fetch 分别回归；degraded RAM 不宣称持久。证据见 [O04 报告](../COMPOSITE-CLIPBOARD-O04-RESOURCE-ARCHIVE-2026-10-06.md)。

## 15. O05：来源范围证明与单调派生

Manifest **4** 在既有 archive_layer 上追加 `source_info { origin, nodes[index,src_sha256] }`，method 为 `portable-cbp1+resource-archive+source-context`。grant 是短期授权能力，不能成为内容身份；origin 是授权范围，不能取代原件指纹。原 HTML／CNA1／data_hash／capture/content/dedupe／source／favorite 不变，只有 archive_revision／digest 更新。schema 13、HistoryIdentity 1、CNA1 1、RestorePlan 1、helper 与网络版本不变。

每次提交先相对现有层验证单调新增，再合并同 origin、无重复 index 的已存来源证明，最后相对原件验证整体；保存与重开均检查 proof.origin 等于原 reference 的 canonical origin。来源层后追加公开资源不能把已授权 blob 当作 public URL，也不能覆盖／删除原资源或 bindings；proof 总数仍最多 8，不提高预算。partial 之后可再次显式授权补缺；0 新资源不自动刷新。

文件导入持有 regular-file descriptor，Unix final component no-follow／nonblocking，检查 descriptor 与路径 identity／mtime／ctime／type／length；Windows 使用 reparse-point 拒绝与限制 share mode，但原生验收仍 UNKNOWN。读取／JSON／base64 与派生准备在有界 owned worker；admission 和共享 256 MiB 内存 reservation 随实际 worker 存活，取消不提前释放。提交仍持 DB 锁做严格载荷／证明验证和原子 CAS，未声称完全消除提交锁开销。

registry 撤销、过期、消费／重放、DB 重开／supersede、选择行不一致与 late dialog／grant 都独立拒绝；过期在载荷准备后事务入口复查。文件授权／packet 可由用户自行修改，source_info 不是浏览器认证签名。回滚 reader 必须支持 v4 或恢复原备份，不能丢层伪装旧版。证据见 [O05 报告](../COMPOSITE-CLIPBOARD-O05-SOURCE-CONTEXT-2026-10-06.md)。
