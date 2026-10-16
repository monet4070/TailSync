# TailSync 图文复合剪贴板执行方案

日期：2026-10-03。适用工作树：`/Users/monet/TailSync/TailSync-rc-phase35`，核查基线 `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。状态：设计与源码调研完成，产品实现和原生验收尚未执行。

本方案依据用户提供的《COMPOSITE-CLIPBOARD-REMEDIATION-SPEC.md》、当前 TailSync 生产链路，以及 Ditto、CopyQ、Maccy、Deskflow 的具体源码制定。目标是保存、同步并恢复一次复制的多种格式，覆盖文字、图片和富文本；同时保留现有配对、文件批次、加密存储和历史收藏语义。

推荐采用两层模型：平台层保留源应用提供的格式集合，共享 Core 生成对端可用的标准格式集合。先实现格式捕获与回放，再补足跨平台图片依赖；无需先开发一个完整富文本编辑器或重新构造全部文档语义。网页图片可用性和微信粘贴能力必须通过原生实验确定，不能由容器设计推导为已支持。

本文的代码结构、限额和阶段划分均为 TailSync 的设计建议。外部项目事实在相应段落附有固定提交链接；没有构建、运行或验收这些参考项目。

**一、开源实现与采用决策**

调研固定提交如下，后续实现应沿这些提交复核，避免默认分支变化导致证据漂移。

| 项目 | 调研提交 | 主要参考价值 |
|---|---|---|
| Ditto | `a80fd3536bc407cac8cce43b5682022282ace983` | Windows 多格式采集、存储、OLE 恢复及网络打包 |
| CopyQ | `d473e274238954fea8c1d39738973ce1c9ff10d7` | MIME 容器、采集有效性检查和渲染分离 |
| Maccy | `9165a3cc0945b02901b3ffe5e73ed0223e8106c4` | macOS 多格式回放、来源标记、应用兼容处理 |
| Deskflow | `081f6478e654a112168879e67a41ce7d7e4ae3af` | 跨平台剪贴板格式转换及整组序列化 |

**Ditto：一个 Clip 拥有多个格式。**

`CClip` 使用 `m_Formats` 保存格式列表。采集循环读取配置支持的格式；启用 SupportAllTypes 时，通过 `EnumClipboardFormats` 枚举可用格式。读取文字摘要没有终止后续格式采集。数据库 `Main` 保存逻辑条目，`Data` 保存父条目 ID、格式名称及字节。恢复时，`COleClipSource::PutFormatOnClipboard` 将多个格式交给同一个 OLE 数据源。来源：[采集](https://github.com/sabrogden/Ditto/blob/a80fd3536bc407cac8cce43b5682022282ace983/src/Clip.cpp#L349)、[格式选择](https://github.com/sabrogden/Ditto/blob/a80fd3536bc407cac8cce43b5682022282ace983/src/CopyThread.cpp#L74)、[数据库](https://github.com/sabrogden/Ditto/blob/a80fd3536bc407cac8cce43b5682022282ace983/src/DatabaseUtilities.cpp#L721)、[OLE 回放](https://github.com/sabrogden/Ditto/blob/a80fd3536bc407cac8cce43b5682022282ace983/src/OleClipSource.cpp#L1262)。

其网络发送在一组 START/END 中遍历 Clip 的格式，以格式名称和长度发送各格式字节；接收端按名称映射成本机格式并积累成一个 Clip。这是多格式组传输，不是依次发送独立的文字复制和图片复制。来源：[发送](https://github.com/sabrogden/Ditto/blob/a80fd3536bc407cac8cce43b5682022282ace983/src/Client.cpp#L200)、[接收](https://github.com/sabrogden/Ditto/blob/a80fd3536bc407cac8cce43b5682022282ace983/src/Server.cpp#L267)。

采用：逻辑条目与格式集合分离；格式按组存储、投递和恢复；Windows 注册格式使用名称映射。TailSync 保留自己的 Noise 认证、消息 ID、可靠投递、加密存储和限额，不迁入 Ditto 的网络栈。

**CopyQ：MIME 容器有价值，采集策略不能照搬。**

CopyQ 使用 `QVariantMap` 保存 MIME 到数据的映射，通过 `createMimeData` 重建一个 `QMimeData`，平台一次发布。其 `cloneData` 路径明确在存在文字时跳过二进制图片，注释解释这是为了避免某些应用生成大型表格图片时阻塞。因此，容器支持多格式不等于其默认采集行为满足 TailSync 的图文完整性目标。来源：[采集与回放](https://github.com/hluk/CopyQ/blob/d473e274238954fea8c1d39738973ce1c9ff10d7/src/common/common.cpp#L159)、[平台发布](https://github.com/hluk/CopyQ/blob/d473e274238954fea8c1d39738973ce1c9ff10d7/src/platform/dummy/dummyclipboard.cpp)。

`ClipboardDataGuard` 检查数据对象生命周期、剪贴板序列变化、经过时间和各 MIME 的大小；文字与图片渲染由不同插件处理。这里的经过时间检查不能中断已经阻塞的系统读取。来源：[有效性与限额检查](https://github.com/hluk/CopyQ/blob/d473e274238954fea8c1d39738973ce1c9ff10d7/src/common/clipboarddataguard.cpp)、[富文本显示](https://github.com/hluk/CopyQ/blob/d473e274238954fea8c1d39738973ce1c9ff10d7/plugins/itemtext/itemtext.cpp)。

采用：MIME 集合、稳定性检查、大小限制和显示分离。TailSync 仍读取受支持的图片，并提供硬超时隔离；失败时记录缺失原因，不静默宣称完整。无需引入 Qt。

**Maccy：保留源格式并区分格式与 item。**

Maccy 枚举 pasteboard items 的类型，保存 `HistoryItemContent(type, value)`；恢复时回放保存的非文件格式，文件 URL 则使用多个 `NSPasteboardItem`。它的注释指出，将某些富文本格式按独立对象写入会造成重复粘贴。它还过滤动态类型、部分 Microsoft OLE 来源类型，并使用来源标记识别自己发布的数据。来源：[采集与回放](https://github.com/p0deje/Maccy/blob/9165a3cc0945b02901b3ffe5e73ed0223e8106c4/Maccy/Clipboard.swift#L75)、[格式记录](https://github.com/p0deje/Maccy/blob/9165a3cc0945b02901b3ffe5e73ed0223e8106c4/Maccy/Models/HistoryItemContent.swift)。

其 `testCopy` 同时检查文字、TIFF 和文件 URL 回放；这证明仓库有相关测试代码，不证明本次测试已运行或微信兼容。来源：[测试](https://github.com/p0deje/Maccy/blob/9165a3cc0945b02901b3ffe5e73ed0223e8106c4/MaccyTests/ClipboardTests.swift#L206)。

采用：先保存源格式；一个语义 item 内放多种表示；多个文件继续走文件流水线。TailSync 平台快照保留原始 item 边界，不直接照搬 Maccy 的扁平合并。来源标记作为辅助，防回音仍以 revision 回执为主。

**Deskflow：标准中间格式与平台转换器。**

Deskflow 的中间格式包括 UTF-8 文本、HTML 和位图。`IClipboard::marshall` 序列化格式数量及各格式的类型、长度、数据；平台转换器负责映射到系统剪贴板，Windows HTML 转换器生成 CF_HTML 头与片段偏移。来源：[格式定义](https://github.com/deskflow/deskflow/blob/081f6478e654a112168879e67a41ce7d7e4ae3af/src/lib/deskflow/IClipboard.h)、[整组序列化](https://github.com/deskflow/deskflow/blob/081f6478e654a112168879e67a41ce7d7e4ae3af/src/lib/deskflow/IClipboard.cpp)、[CF_HTML 转换](https://github.com/deskflow/deskflow/blob/081f6478e654a112168879e67a41ce7d7e4ae3af/src/lib/platform/MSWindowsClipboardHTMLConverter.cpp)。

采用：平台格式到标准格式再到目标平台格式的转换边界。TailSync 接收时先完整解析和校验，再发布；不复制该项目在反序列化开头清空目标的处理顺序。HTML 包装转换也不能替代图片资源捕获。

**二、当前 TailSync 接入点**

| 边界 | 已核查的生产文件 | 本次实施职责 |
|---|---|---|
| 双端监听 | `macos/src-tauri/src/clipboard.rs`、`windows/src-tauri/src/clipboard.rs` | 取稳定快照、分类来源、调度一个事件；移除文字排他分支 |
| macOS 系统 I/O | `macos/src-tauri/clipboard-helper.swift`、`macos/src-tauri/src/clipboard_file.rs` | 二进制快照协议、整组发布、读取与写入回执 |
| Windows 系统 I/O | `windows/src-tauri/src/clipboard_file.rs`、`clipboard.rs` | 原生多格式采集、内存所有权和发布 |
| 平台接口 | `shared/rust-core/src/sync.rs`、两端 `sync_adapter.rs` | `write_composite` 与原生写入结果 |
| 编解码和限额 | `shared/tailsync-protocol/src/lib.rs` | 复合包、分片、确认、静态长度校验 |
| 认证能力 | `shared/rust-core/src/secure.rs` | 能力、运行时开关、每个会话协商 |
| 可靠投递 | `shared/rust-core/src/peer/delivery.rs`、`peer/delivery/executor.rs` | 选格式、固定事件身份、分片确认与最终结果 |
| 可靠接收 | `shared/rust-core/src/peer/event_receiver.rs`、`event_receipts.rs`、`shared/platform-network-server.rs` | 重组、授权、限流、接收回执和最终提交 |
| 防回音 | 两端 `clipboard_change.rs`、`shared/rust-core/src/sync/engine_state.rs` | revision 来源分类先于哈希与事件门禁 |
| 历史生命周期 | `shared/rust-core/src/db/` | schema、格式关联、加密载荷、搜索、回收与迁移 |
| 应用用例与契约 | `shared/tailsync-runtime/src/history.rs`、`shared/schema/` | 恢复、分页、缩略图、DTO 生成 |
| macOS UI | `macos/swift-ui/Sources/TailSync/Views/HistoryRow.swift`、相关 Models/Services | 图文卡片、资源预览、整组恢复 |
| Windows UI | `windows/src/pages/history/HistoryItem.tsx`、相关 hooks/client/types | 图文卡片、资源预览、整组恢复 |

当前文档中的三处实现细节需要更新：能力定义不在 `tailsync-protocol/src/capabilities.rs`；示例命令值 10/11 已与配对命令冲突；实际历史列是 `data`，正文搜索包含解密扫描而非现成的全文索引。当前图片分片使用 zlib，并验证 `PackedImage`，不能把 HTML 混入现有 ImageChunk 就视为复用完成。

**三、目标契约与支持范围**

平台内的 `NativeSnapshot` 保存 revision、原始 item 序号、格式名称、字节、格式读取结果。共享 `PortableBundle` 表达一个语义剪贴板内容的标准表示及其资源。文件路径不塞进复合包，继续使用当前文件批次。

建议新增 `shared/rust-core/src/composite/`，拥有规则、转换结果、稳定指纹和支持范围；线上的有界描述类型与编解码放入 `tailsync-protocol`。系统读取及系统格式转换调用留在平台 Adapter。

```rust
// 概念接口，字段编码和具体所有权在 M1 冻结。
struct PortableBundle {
    version: u16,
    representations: Vec<Representation>,
    resources: Vec<Resource>,
    bindings: Vec<ResourceBinding>,
    completeness: Completeness,
}

struct Representation {
    mime: PortableMime,
    bytes: Arc<[u8]>,
}

enum PortableMime { PlainTextUtf8, HtmlUtf8, Rtf, Png }
enum Completeness { Complete, ExternalReferences, MissingFormats }
```

事件 ID 和时间沿用外部 `EventEnvelope`，不放入内容哈希。NativeSnapshot 的 revision 和来源标记不参与跨机内容哈希，也不当作对端可复用的 OS revision。

第一版支持一个语义图文内容、多格式表示和其中多张图片。平台可以采集多个 native items；只有证明它们是同一内容的重复/互补表示时才合并。无法无损映射到 Windows 的真实多 item 内容保留在本地历史，并报告不支持的跨平台形状；禁止静默取第一项。多文件仍按原批次同步。

NativeSnapshot 的原始格式及 item 边界保存在本地历史的 native 部分，PortableBundle 保存标准表示；同平台恢复可使用经过验证的 native 配置，跨平台仅使用可移植配置。未知注册格式默认拒绝，不把“按名称传格式”扩展成任意格式透传。

| 内容 | 采集与保留 | 跨平台转换与恢复 |
|---|---|---|
| 纯文本 | UTF-8 标准表示 | Windows UTF-16；macOS string |
| HTML 富文本 | 原始 HTML 及必要上下文 | CF_HTML 与 public.html 互转；保留 base/source URL 语义 |
| PNG/TIFF/DIB/DIBV5 | 保留允许的原格式；生成规范图片 | PNG 为可移植表示，目标端生成受支持系统图片格式 |
| RTF | 保留原始字节 | 经过目标应用实验后加入标准回放配置 |
| RTFD | 本地保存；在 helper 内提取附件及转换 | 不跨平台盲发原始 RTFD；M6 决定可验证的转换配置 |
| HTML 内多图 | 提取已存在的内嵌字节并建立资源关联 | 根据已验证配置生成带资源的 HTML/RTF/RTFD 表示 |
| 应用私有/OLE 对象 | 默认不进入可移植包 | 仅允许显式验证的同平台格式；系统句柄和进程指针不得序列化 |

独立 PNG 与 HTML 的某个 img 节点不一定指向同一张图片。没有可靠关联证据时，它们只作为独立表示保存，不擅自把图片插入文档。

内容哈希使用带版本、类型、长度和顺序边界的确定性编码，覆盖表示、资源和绑定；排除诊断、事件 ID 和自身来源标记。保留文字原有空白，不为了去重修改用户内容；图片在统一色彩/alpha 规则下生成像素指纹，用于原生转换后的回读核对。

**四、稳定采集与整组发布**

macOS 增加 helper 命令 `--read-snapshot`、`--write-snapshot`。Rust 通过有长度上限的二进制管道传递内容，避免 JSON/Base64 复制大型图片。快照包含读取前后 revision；二者不同返回 `Unstable`。helper 的进程级超时和终止机制继续保留，不将可能阻塞的 AppKit 内容读取搬入 daemon 或 SwiftUI。

Windows 枚举允许格式并在一个打开会话中读取；读取前后验证 sequence。DIB 必须验证头长度、尺寸乘法、stride、方向、压缩类型与 alpha；支持的注册格式按名称识别，不能把注册格式数字当作跨机身份。对 OLE 延迟数据提供者的阻塞先通过 M0 重现；若进程内读取无法受控，M2 加一个可终止的 Windows 读取 helper。普通 async timeout 不能取消卡住的原生调用。

读取结果区分 `Empty / Stable / Unstable / Busy / Failed`。Stable 内的每种格式再区分 `Captured / Absent / Failed / TooLarge`。受支持图片存在但读取失败，不等于纯文本。遇到这种情况先重试；仍失败时只以显式的 MissingFormats 状态提供降级结果，成功指标不计入完整同步。

监听器将当前“检测到变化立即更新 last revision”的行为改为观察和确认两步：只有稳定采集，或明确忽略/拒绝该 revision 后，才确认消费。重试失败的 revision 保留有限重试状态，不靠再次变化才触发读取。

发布前完成所有编码、格式注册和内存分配。macOS 为一个语义内容构造一个多格式 `NSPasteboardItem`；不把文字、HTML、图片作为三个独立对象追加。Windows 在一次打开会话内发布完整格式集合，检查每次系统调用及 HGLOBAL 所有权转移。文件回放继续由原接口负责。

`write_composite` 返回 `Published / Failed / Partial / Superseded`，附实际发布 revision 和观测到的格式指纹。部分发布不能作为成功返回，也不能直接删除自身写入记录。只在仍拥有该次 revision 时执行清理或恢复；不得为回滚覆盖用户后来复制的内容。macOS clear/write 之间仍可能被其他进程竞争，结果需要检测，不能宣称系统支持 SQL 式事务。

Windows CF_HTML 按 UTF-8 字节计算 StartHTML/EndHTML/StartFragment/EndFragment，支持合法的换行与可选上下文；复核中文、emoji 和终止符。规范来源：[Microsoft CF_HTML](https://learn.microsoft.com/en-us/windows/win32/dataxchg/html-clipboard-format)。

**五、防回音与本地更新优先级**

新增有界 `ClipboardOriginTracker`，保存多次写入的 revision、实际格式指纹、写入 token 和期限。平台在执行发布的同一操作内产生回执；不要在 helper 退出后单独读取当前 revision 再拼成回执。

顺序固定为：稳定快照 → 来源分类 → 去重/门禁 → Core 调度 → 历史/广播。精确匹配自身 revision 静默消费；External revision 即使内容相同也允许处理；Unknown 才进入保守哈希兜底。图片编码变化、系统合成格式和来源标记排除规则由统一规范化指纹处理。

可以借鉴 Maccy/CopyQ/Deskflow 添加 TailSync 的本地来源格式，包含随机写入 token。该格式只在本地发布，不进入网络、历史正文或内容哈希；来源格式单独存在不足以证明 SelfWrite，不能吞掉携带旧标记的用户新复制。

接收长图文时，在第一片获准进入时记录本地内容 generation，并在发布前再检查。用户后来复制、历史恢复或另一事件成功发布会推进 generation。已经过时的复合事件只入历史，返回 `Superseded`；不能在慢图文重组结束后覆盖新剪贴板。扩展当前文件批次的 generation 规则，而不是再加一套互不知晓的“最新事件”计数。

**六、网络包与成功确认**

能力新增 `composite_clipboard`，只在认证握手中双方都宣告且数据帧协商为 v5 时启用。增加构建 feature 和运行前开关 `TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD`。PeerInfo 的展示字段不作为能力的授权来源。

暂定新命令为 `CompositePayload=0x010e`、`CompositeChunk=0x010f`、`CompositeAck=0x0110`，M1 再核对完整注册表并冻结。三个值在本次基线未占用。v4 握手和现有旧命令数值保持兼容。

复合 body 使用版本化二进制 framing：小型 manifest 描述标准格式、资源、offset、length、digest，后接原始字节区。manifest 不包含 Base64 图片。解析时检查整数溢出、范围重叠、重复格式、绑定目标、尾随字节以及实际 digest，验证后才返回可发布对象。

以下是首版建议限额，属于设计值，M0 测量后在 M1 冻结；所有上限同时成立，不能单个合法而总量超限：

| 项目 | 建议上限 |
|---|---|
| 明文完整 EventEnvelope | 32 MiB，沿用当前最大内容帧量级，包含 framing 开销 |
| manifest | 64 KiB，内容正文存字节区 |
| 标准表示数量 | 16 |
| 图片资源数量 | 20 |
| 纯文本 | 1 MiB |
| HTML | 2 MiB |
| RTF | 4 MiB |
| 每图及本事件全部图片解码后的 RGBA | 各自不超过 32 MiB；尺寸另做 checked arithmetic |
| 分片数据 | 512 KiB；最多 64 片 |
| 组装并发 | 每认证 peer 1 组；全局 4 组，并受总内存预算约束 |
| 未完成组装 | 30 秒 idle timeout、60 秒总期限，并受现有事件时间窗口约束 |

小包可直接投递；大包一定分片，即使压缩没有收益也可发送未压缩分片。压缩选择 raw/zlib，不假设 PNG 再压缩总有收益。复用现有有界图片分片原语，但以独立 `CompositeAssembler` 和新的 payload 校验接线；第一版不必重构已经稳定的 ImageAssembler。

`CompositeAck` 明确区分分片接收与事件最终提交。分片确认绑定认证 peer、event ID、frame sequence、chunk index 和整组 digest。最终确认包含 `Applied / Superseded / ApplyUnknown / FailedPartial` 结果；Partial/Failed 不返回 Applied。现有 EventAck 的 16 字节消息 ID 格式不改变，旧端不会收到新命令。

可证明尚未发生系统副作用的 Busy/Failed 是可重试拒绝；实际 Partial 是明确失败终态，保留可恢复历史和自身写入信息，停止自动重复发布。`Applied` 只表示选择的格式组已发布并提交，完整同步指标还要求 completeness 为 Complete、目标转换没有损失；格式降级不能计入完整成功。

发送端以一个不可变对象共享复合载荷。为每个 peer 等待能力确定后选择编码；同一次事件对该 peer 的重试固定事件 ID、digest 和表示选择。重连后若能力减少，拒绝继续发送旧的复合编码；在尚未尝试发布的事件上可以重新选择降级表示，已经尝试过的事件不得换内容后沿用同一 ID。不能让 ACK 丢失后的重试变成第二次不同的复制。

未支持复合的旧端只能接收一个旧式事件：确有图片字节时按原需求优先选明确的图片表示；否则发送可用纯文本。多图降级选择需要有明确代表图，否则报告不能完整降级。新端本地历史仍保存完整组。禁止先 TextPayload 再 ImagePayload，因为那会形成两次互相覆盖的复制。

Admission 把复合首包计入事件预算，后续分片计入字节预算；绑定当前认证公钥指纹并在接收中继续执行撤销授权检查。重组失败和会话断开释放受控内存，不保留无期限隐式缓存。首次完整版本不增加跨进程分片续传；连接内失败按同一事件重新传组，保留后续持久续传的独立扩展空间。

**七、历史存储和崩溃边界**

采用 schema v12 的 `history` 主行加 `history_parts` 关联表，不增加固定 secondary_hash/secondary_meta。关联项同时覆盖格式表示和图片资源，可有多个组成部分。

```sql
-- 示意：实际迁移须同时检查当前 open/migrations/reads/lifecycle 契约。
CREATE TABLE history_parts (
    history_id INTEGER NOT NULL,
    part_id TEXT NOT NULL,
    role TEXT NOT NULL,
    format TEXT NOT NULL,
    data_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    stored BLOB NOT NULL,
    PRIMARY KEY (history_id, part_id)
);
CREATE INDEX history_parts_hash ON history_parts(data_hash);

CREATE TABLE history_composite_events (
    source_device_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    digest TEXT NOT NULL,
    history_id INTEGER,
    timestamp_ms INTEGER NOT NULL,
    PRIMARY KEY (source_device_id, event_id)
);
```

`history.type='composite'`，主行 `data` 保存加密的文本/HTML manifest 与绑定信息，`data_hash` 为整组哈希；large parts 的 stored 使用版本化外置引用，小项可内联加密。主体重复与图片资源去重是不同问题；图片同内容共享文件时，清理必须查询全部存活引用。主行与关联行在同一 SQLite 事务中提交和删除；不能假定示意表自动级联，须确认连接外键配置或在既有删除事务中显式删关联行。

复用 private_fs、crypto 和原子文件替换原语，新增复合引用 codec。提交前写外置数据，事务失败后按存活引用清理；崩溃孤儿通过有界回收处理。扩展 external_payloads_for_ids、validate_live_payload_references、remove_unreferenced_payload_paths，以及空间预检、存储迁移、不可用恢复和 WAL 管理，防止新类型绕过既有安全清理。

保留现有正文加密策略。生产 `db/reads.rs` 的分块读取、取消检查和文字解密搜索必须识别 composite，同时维护同步查询的兼容路径；不能只修改 `queries.rs`。列表读取 manifest 的文本摘要和首图元数据，不解码整组图片。主行 size_bytes 表示逻辑总量，物理配额继续依据实际文件占用，重复资源不重复算物理空间。

收藏保护、整组删除、去重和计数使用一个主行。含图 composite 与 image 共用图片保留预算，纯富文本 composite 与 text 共用文字预算，物理存储仍受总配额约束；应在 manifest 元数据中保存预算类别，不能在 Trim 中全量解码。旧 text/image/file 记录保留原格式，不批量转换。

接收复合内容采用前向状态：`Validated → NativePrepared → EffectClaimed → EffectReported → HistoryCommitted → Completed`。复用当前持久 claim 的受限写入原语，新增独立版本的复合回执文件和状态；保持旧 remote-event-receipts-v1.json 格式可读。回执只存认证来源、事件 ID、digest、状态和结果，不存剪贴板正文或源路径。

history_composite_events 的事件映射与主历史行在同一事务中提交，使同一认证来源与事件 ID 的历史写入幂等；digest 不同则拒绝碰撞。主行被用户删除后保留有界事件墓碑、清空 history_id，阻止合法重试把已删除历史恢复出来。事件映射与回执的回收期限须覆盖原事件仍能通过时间戳校验的窗口，不能先回收再允许重放。

在任何原生副作用之前持久 claim。同一次事件的历史提交采用事件身份去重，ACK 丢失后的重试不刷新时间、重建历史行或重复写剪贴板。原生成功而历史失败，只重试历史。崩溃后只有 claim、无发布结果时，不自动重做系统写入；保存可恢复历史并返回 ApplyUnknown，使发送端不会把结果算成完整应用成功。系统剪贴板与 SQLite 不具备跨系统事务，方案保证有界、可解释恢复，不承诺 exactly-once 成功粘贴。

若 Adapter 能证明失败发生在任何系统修改之前，可以撤销 claim 后重试；发生部分修改或无法确认时保留 claim。用户从历史明确执行的恢复是新的本地操作，不沿用网络失败事件的发布尝试。

v12 建立迁移前备份和明确的旧程序打开策略。关闭复合网络能力只回退投递路径，不等于 schema 自动降级。产品回滚应使用升级前备份或兼容 v12 的构建；不得让旧程序将新复合数据按纯文本解析、Trim 或清理。

**八、网页图片资源和目标应用配置**

先区分独立图片格式、HTML 中的 data URI、RTF/RTFD 中的内嵌资源和外部 URL。源应用未提供图片字节时，TailSync 无法凭 HTML 包装恢复图片字节。第一阶段保留引用与上下文，标记 ExternalReferences；不能把它统计为离线图文成功。

M6 提供资源补齐模块，优先处理已有内嵌字节及可验证的附件关联。对仅有网页 URL 的场景，另设明确的资源抓取选项：有限并发、总时间/大小预算，只允许受限 HTTP(S)，不附带浏览器 cookie/认证信息，不读取任意 file URL、不请求本机或私有地址，重定向后继续检查。无法取得的 blob/登录资源保留缺失状态；需要来源应用适配的场景列为后续能力，不能伪装成已解决。

资源重写保留原始格式副本，生成可移植副本。先验证目标应用接受 data URI、RTF 内嵌图片或 macOS RTFD 附件中的哪一种，再启用相应配置；不假定 data URI 在微信、Word、浏览器编辑器之间通用。临时附件若为粘贴所需，在系统剪贴板仍引用它时保持有效，通过版本回执和租约回收，不能在发布完成后立即删除。

文字与独立图片并存、却没有源文档顺序时，默认发布它们作为可选表示。若产品需要合成图文，定义显式的“文字在前、图片在后”恢复操作；不将这项新编辑行为混同于保真同步。

UI 使用文本摘要、缩略图和图片数展示一个逻辑条目；默认恢复整组，另提供复制文字/复制图片。预览先使用安全文本与受控图片，富文本预览不得执行脚本或自动加载外部 URL。已含外部引用或缺失资源时显示相应状态，避免卡片存在就被认为全链路完整。

**九、实施阶段与交付物**

依赖顺序：M0 → M1 → M2 → M3 → M4 → M5 → M6 → M7。各阶段可以用 feature 保留回退路径；不得通过只移除 continue 就提前启用双事件广播。

| 阶段 | 具体工作及文件 | 输出与退出条件 |
|---|---|---|
| M0 真实格式与粘贴可行性 | 新建 `tools/clipboard-probe/` 诊断工具和合成网页夹具；不读取/上传用户历史 | 记录来源应用、版本、平台、格式、长度、图片是否内嵌和粘贴结果；完成 Chrome 图文、多图、Word 附图、文字带图片格式、纯图片、文件组基线；分别测试微信、Word/Notes 和纯文本编辑器 |
| M1 契约和纯规则 | 新建 protocol composite codec、Core composite 模块；修改 secure 能力；补合法/恶意/旧版本 fixtures | 模型、命令值、限额和错误语义冻结；格式映射/哈希确定；协议拒绝截断、重叠、错绑定、解压炸弹；旧握手/帧测试保持兼容 |
| M2 双端稳定 I/O | helper、clipboard_file、clipboard_change、sync_adapter；必要时 Windows 隔离 helper | 一个快照不混入两次复制；原生同平台多格式回放成立；部分写入可识别；来源分类早于去重；存在文字仍采集图片；繁忙/超时/格式失败有可重试结果 |
| M3 本地复合历史闭环 | schema/migrations、复合引用、entries/reads/queries/preview/storage/lifecycle/favorites；runtime 和生成 DTO | 无网络也可保存、搜索、收藏和重启后恢复图文；删条目不误删共享资源；Trim、满盘、不可用目录、迁移失败和回滚可验证；旧历史继续可读 |
| M4 跨机可靠投递 | delivery/executor、复合 assembler、event_receiver、network server canonical include、两端 watcher | 新端→新端完整发布；新端↔旧端和运行时关闭能力正常；最终 ACK 后才计完整交付；缺片/重试/崩溃/撤销授权/用户新复制均符合规定 |
| M5 UI 与 API | HistoryRow、HistoryItem、Models、ApiClientHistory、Windows client/hooks、两端 restore 路由 | 一个图文卡片、文字搜索、缩略图、完整恢复和格式选择；通过生产 DTO 生成并检查契约；错误和不完整状态进入现有历史提示槽位 |
| M6 图片依赖与应用适配 | Core 资源绑定规则、平台 RTF/RTFD 转换、受限资源抓取、目标格式配置 | 合成内嵌双图离线恢复；真实 Chrome 页面按来源分类验收；明确微信支持的组合及限制；只有 URL 的场景要么补齐成功，要么保留不完整状态，不能宣称达标 |
| M7 原生包与双机验收 | macOS helper 包装/签名/最低系统版本、Windows 原生构建打包；现有验收证据格式 | 同一候选源码/包的 Mac↔Windows 双向验证，LAN 和当前支持路由、睡眠/重连、历史重启、旧版包互通；微信目标门槛单独记录，失败则功能范围未完成 |

M2/M3 可以暂不切换自动监听，仅通过测试接口验证闭环；生产开关在 M4 后启用，并在 M7 通过后确定默认策略。未实现或未验收的阶段保持 Pending/Unknown，不能由构建成功推导完成。

**十、必须覆盖的验收场景**

| 类别 | 场景 | 判定 |
|---|---|---|
| 格式完整性 | Text+PNG、Text+HTML、Text+HTML+图片、多图、仅富文本 | 允许格式全部捕获；不存在文字抢占图片；没有原格式时不编造 |
| 平台回放 | 中文/emoji CF_HTML、透明 PNG、DIB 方向/stride、同内容多表示、多 native items | 正确字节边界和图像；同一内容不重复粘贴；不可映射 shape 明确返回 |
| 来源因果 | 连续自身写入；用户 50 ms/500 ms/1 s/29 s 内复制相同内容；携带旧来源标记 | 自身写入零广播；用户新 revision 可以同步 |
| 稳定读取 | 读取间隙被覆盖、provider 延迟、读取超时、sequence=0/回绕、睡眠唤醒 | 不发布混合快照；失败可重试；不因检测器提前确认而丢失事件 |
| 网络 | 缺片、重复片、错摘要、错误 ACK、超限、ACK 丢失、撤销配对、不同能力的多个 peer | 无部分内容写入；无重复副作用；仅确认正确事件；每个 peer 独立选择编码 |
| 优先级 | 图文 A 传输中用户复制 B；两个 peer 到达；旧事件重试 | A 不覆盖 B；明确终态；跨连接规则一致 |
| 持久化 | claim 前后、系统发布后、DB 提交前后、最终 ACK 前后崩溃 | 不重复自动应用不确定事件；不重复历史；ApplyUnknown 可解释 |
| 存储 | 重启恢复、收藏保护、整组删除、共享图去重、配额、满盘、目录迁移及故障 | 图片不丢失；不删仍被引用的资源；无明文正文落盘 |
| 真实应用 | Chrome/Word→Mac↔Windows→微信/Word/Notes/纯文本编辑器 | 记录实际应用版本、格式和可见结果；以具体组合判定支持范围 |
| 性能 | 大型表格、32 MiB 上界、多格式冗余、连续复制、4 组并发 | 测量捕获延迟、主线程卡顿、RSS、队列与超时；M0 基线后冻结性能预算 |

至少执行 10,000 次合成远端写入回路，要求无自动回传；该本地测试不替代两台设备交替复制与目标应用粘贴验收。第三方应用生成剪贴板属于原生集成验收，不称为普通单元测试。

**十一、实施时的验证入口**

以下命令为后续实现阶段的检查清单，本次仅编写方案，未执行这些产品测试。实际每阶段先跑有意义的聚焦回归，涉及共享接线和最终合入再运行相应全量门禁。

```bash
node scripts/run-isolated-tests.mjs -- cargo test --locked --manifest-path shared/tailsync-protocol/Cargo.toml
node scripts/run-isolated-tests.mjs -- cargo test --locked --manifest-path shared/rust-core/Cargo.toml
node scripts/run-isolated-tests.mjs -- cargo test --locked --manifest-path shared/rust-core/Cargo.toml --features protocol-file-sliding-window,protocol-image-compressed-chunks
node scripts/run-isolated-tests.mjs -- cargo test --locked --manifest-path shared/tailsync-runtime/Cargo.toml
node scripts/run-isolated-tests.mjs -- cargo test --locked --manifest-path macos/src-tauri/Cargo.toml --lib
node scripts/run-isolated-tests.mjs -- bash scripts/check-windows-host.sh
node scripts/run-isolated-tests.mjs -- swift test --package-path macos/swift-ui
node scripts/check-local-contracts.mjs
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
npm --prefix windows test
npm --prefix windows run build
npm --prefix windows run lint
```

M1 新 feature 建立后，增加 composite feature 开/关组合的 Core、平台及互操作测试；执行各受影响 Rust crate 的 fmt/clippy，以及当前规范要求的 Swift SVG 测试。macOS 实包执行 `check_macos_sources.sh`、最低部署版本检查与 `bash macos/scripts/verify_macos_bundle.sh`；Windows host 检查会裁掉 Windows 专属代码，必须另外取得 Windows 原生 CI、包和运行结果。

原生测试使用合成内容与隔离的 TailSync 数据目录。真实系统剪贴板实验需先保存并在结束后恢复用户剪贴板，避免以读取格式调研为由改动日常数据；暂停旧实例后确认候选实例和端口归属。所有测试结果记录 exact SHA、功能开关、包路径与平台，保留源码测试、CI、实包和双机证据的区别。

**十二、实施范围和当前未解决条件**

推荐按上述阶段实现：格式集合保真、标准格式转换、有界可靠事件、一个逻辑历史条目、revision 因果回执和明确的目标应用配置。第一阶段没有必要迁入 Qt/MFC、实现完整 DOM 编辑器、自动截屏网页，或把文件批次改成复合图片传输。

本方案可以根治当前两端文字排他造成的遗漏，也能保留源应用已提供的图文表示。但任意网页登录图片、任意应用私有对象和微信任何版本的一次粘贴图文，不属于已获证据支持的能力。M0 与 M6 必须完成这些边界的原生验证或补充适配，M7 达标后才能声明原需求全部完成。
