# S2 用户可见体验：详细执行方案

日期：2026-10-07，Asia/Shanghai。状态：**执行中；S2 首批 F02/F03 确定部分本机 LOCAL_COMPLETE，完整 S2 未完成。** 首次交付为 PLAN_ONLY；后续用户授权一个 DeepSeek/high 子代理实施，主代理独立复核和二次修复。详见[首批报告](COMPOSITE-CLIPBOARD-S2-01-UI-SIMPLIFICATION-2026-10-07.md)。

## 1. 目标与交付边界

S2 把已经完成的原件保存、资源归档和恢复基础，转化为用户能够辨认、预览和正确操作的历史体验。核心目标不是增加“富文本”标签，而是让用户看见实际保存的内容，并能按现有整组恢复规则使用它。

| 反馈 | 本阶段交付 | 完成判据 |
|---|---|---|
| F01 单图分类与提示 | 单图按图片呈现；图文按文稿呈现；核对提示来源与实际结果 | A01/A02；不能按格式数量分类，不能靠关闭通知消除误报 |
| F02 复制文字入口冗余 | 移除普通历史行“复制文字”；没有剩余操作时不显示空菜单 | A03；默认仍整组恢复，后端显式格式能力不删除 |
| F03 技术信息干扰 | 默认列表移除格式名/数量，诊断详情保留；不擅自删除完整性反馈 | A04；中英文、主题、键盘、辅助功能及真失败可见性通过 |
| F04 富文本不能有效预览 | 原生/HTML 图文能阅读文字、基本样式、图片及顺序，基本表格有明确支持范围 | A05/A06；预览不写剪贴板、不同步、不联网、不改原件 |
| F05 QQ 图文识别与恢复 | 对真实 QQ 复制行为复现、定位、修复并做直接粘贴/历史恢复对照 | A07；缺 QQ 现场条件时不得宣布完整通过 |
| F06 首图缩略图 | 按文稿中的首张图片定位，而不是按资源存储顺序或独立 PNG 抢占 | A08；首图不可用不拿第二张冒充 |

明确不做：O07 原生同步、O09 OLE/跨平台对象扩展、独立资源缺块协议、任意网页联网预览、QQ 私有缓存/凭据读取、全局纯文本粘贴快捷键拦截、自动安装/发布、替换用户日常 App。

本阶段面向 macOS 主体验和 Windows 共享契约/React 实现。Windows 原生运行仍需 Windows 环境；Mac host 测试不替代它。复杂 Office 对象、动画播放、任意 HTML/CSS 的像素级保真不属于第一版预览承诺。

## 2. 当前基线与已经核对的代码事实

唯一实施目录：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`。环境默认 cwd 指向另一份 checkout，所有命令都必须显式指定此目录。

- 分支：`codex/composite-clipboard`；HEAD：`f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。
- 产品/测试/工具指纹：`a996992fc3922c4fe37164f7241fee40780d751c3dde04042f396fb396d91096`，796 文件；本次核对与 O05 最终源码一致。
- schema 13、Manifest 4、HistoryIdentity 1、CNA1 1、RestorePlan 1 是当前基线；不能只用 HEAD 表示这份有大量未提交工作的候选。
- O05 的 29 条同源码门禁是已有局部验收记录，不直接继承为 S2 新源码的 PASS。真实 QQ、完整 GUI、Windows 原生、双设备和 hosted CI 仍 UNKNOWN，Release NO-GO。

下列路径均相对于上述唯一目录，实施前须重新核对调用链：

| 当前事实 | 生产接缝 | 对本阶段的影响 |
|---|---|---|
| composite 入库分类主要看便携 PNG/资源 | `shared/rust-core/src/db/composite.rs` | 不能据此认定 native-only 附件不存在；不能只改前端标签 |
| 两端缩略图资格仍主要看 image_count | Mac `Models/HistoryEntry.swift`；Windows `pages/history/HistoryItem.tsx` | 需要新增有界展示事实，不能把资源数当文稿图片语义 |
| 图片读取优先独立 PNG，再取资源首项 | `get_composite_image` / `prepare_composite_image` | 原件、归档层及 native 附件应使用同一首图定位规则 |
| Mac cache 用 id，Windows Map 也以 id 为主 | `HistoryRow.swift`；`useThumbnailCache.ts` | 同一历史补图后可能仍命中旧图；必须补版本/会话绑定 |
| Mac 普通行菜单提供 text/png，且包含 O04/O05 操作 | `Views/HistoryRow.swift` | 删除 text 不能顺带删除归档、授权、导入、撤销或手动恢复 |
| 两端默认行显示格式计数和完整性文字 | `HistoryRow.swift`；`HistoryItem.tsx` | 技术信息隐藏与真实风险反馈必须拆开处理 |
| 共享 PreviewKind 当前为 Text/Image/File；Mac 无专用复合文稿 material | `db/types.rs`；`HistoryPreviewMaterializer.swift` | 文稿预览需要版本化接缝，不能把 HTML 当代码视图交差 |
| helper 已有有界 RTFD→portable 转换，但存在严格转换拒绝 | `macos/src-tauri/clipboard-helper.swift` | 可复用安全图像/属性处理；预览局部可读不等于便携完整可恢复 |

现有架构继续遵循 [ADR-002](adr/ADR-002-independent-history-preview-window.md)：一个可复用、非模态预览窗口，history/favorites 有明确 owner，逐项加载有界载荷。S2 的复合文稿预览始终离线；不能继承 SVG 已有的显式外源信任选项来放宽本阶段。

## 3. 五条不能破坏的约束

1. **原件不变。** 展示、分类、缩略图和预览都是派生结果，不替换原 HTML/RTF/RTFD/CNA1，不改 capture/content/dedupe/data_hash，不降低默认恢复保真。
2. **展示有证据。** 多格式不是多段内容；附带 URL、文件名或替代文字不是自动认定的正文；只有保存并校验过的附件才声称可显示。
3. **副作用隔离。** 打开、换项、关闭预览及加载缩略图不写系统剪贴板、不触发同步、不隐式联网、不生成一次新的复制事件。
4. **身份贯穿。** DB authority、history identity、归档版本、请求 generation 和 preview owner 覆盖 worker、响应、cache 与 renderer；取消后不得显示迟到结果。
5. **证据分级。** 自动测试、私有 board、完整 GUI、真实 QQ/目标应用、Windows、双设备、CI 分别登记；缺条件记 UNKNOWN，不用模拟证据填满现场验收。

## 4. 关键设计：一套内容事实，三个消费者

### 4.1 共享有界内容分析

拟增加一个专供展示的有界分析结果，供分类、首图和文稿预览复用。名称、字段和 API 均是设计建议，尚未实现；先审计现有契约再确定注册方式。

最少包含：选定表示及来源、原生 item 边界、实际正文存在性、基本样式/表格存在性、按内容顺序的图片引用、首图定位、可展示/缺失/不支持原因、输入身份与分析版本。

不创建“可替代原件的统一文档格式”，不建立第二套恢复模型，不把所有来源格式无差别拼接。不同表示冲突时选择一份具有明确依据的正文；预览、分类和缩略图记录同一选定表示。

分析路径：

1. 从 DB 取得当前身份和所需最小 owned payload，避免持 DB 锁进行 AppKit 解码、HTML 全文解析或图片转换。
2. HTML 复用现有资源绑定和归档证明；顺序来自原正文节点。原件/派生层关系必须验证，不重新信任 packet 自报来源。
3. RTF/RTFD 在受监督 helper 中处理保存的字节，保留附件字符位置；只接受已持有的 regular-file 内容，不加载路径、远程附件或来源应用缓存。
4. 多个原生 item 保留边界及顺序；不能将独立图片强行插到没有位置证明的 HTML 中。
5. 生成有界摘要和资产引用，回到当前 identity/generation 验证后再使用。

列表只按需分析可见项；不得每次刷新都解密/解析全部历史。复用同一 identity 的分析，缓存必须有总量和释放边界。第一版优先不增加 DB schema；若确需持久分析索引，先列出迁移、配额、升级/关闭态读取和回退设计，单独审阅后再实施。

### 4.2 分类语义与筛选一致性

| 保存内容事实 | 建议展示 |
|---|---|
| 一张实际图片，仅附带可证明为替代表示的 URL/文件名/alt | 图片 |
| 有实际正文和有位置的图片，或有基本富文本结构 | 文稿/图文 |
| 纯文字或既有 code/website 等文字内容 | 沿用已有分类规则 |
| 文件集合 | 沿用文件规则，不被正文/图片判定抢占 |
| 未解析、坏数据、无法证明文字与图片关系 | 保守图标和准确诊断，不冒称完整图文 |

优先使用可选、版本化的展示摘要；不要直接向严格 decoder 塞入未注册的 `rich_text` category。现有合法 category/categories、搜索筛选、计数和配额语义须在本批一起审计。

若图片事实需要修正既有分类字段，使用现有字段和明确 classifier version，提供有界、可取消、identity/CAS 保护的派生元数据重建；仅改分类列，不改原载荷/摘要，不在重建时自动 trim 历史。新旧历史、收藏及筛选必须一致。文稿展示名与当前筛选类别的映射须写成测试，不容许“图标已改，图片筛选仍漏掉”的半修复。

### 4.3 首图规则与缓存

- 文稿首图：HTML 节点序号、RTFD 附件字符位置、原生 item 顺序；资源去重、存储顺序和独立 PNG 不决定正文首图。
- 单图条目：可使用经过证明属于同一图片的高质量兼容表示。不能因存在 PNG 就假定它对应文稿首图。
- **默认采用严格首图规则：**首图缺字节或解码失败，显示内容图标/缺失反馈；不偷偷换成第二图。“首个可用图”如另行采用，必须作为单独降级决策审阅。
- cache key 至少绑定 DB authority、entry/capture identity、archive revision/digest、分析/renderer 版本和缩略图规格；不只用 id/data_hash。
- 补图、删除、重开库、换 owner、版本变化及取消时同步处理 cache 和 in-flight 请求。实际 worker 未退出前不能提前释放内存 lease。
- 保留当前 48pt 缩略图布局和既有约 4 MiB cache 额度，不为 history/favorites/新 renderer 无限制复制 cache；成本包含实际解码像素和编码缓冲。

### 4.4 文稿预览与安全

首版支持：中文/emoji、段落、粗体/斜体/下划线、受控颜色、图片位置和基本表格。复杂排版、不可读附件及不支持样式须给局部说明；预览降级不改变原生恢复可用性。

建议优先使用有限 block/run/table/asset 的展示数据和受控 renderer；HTML 只允许白名单样式/标签，RTF/RTFD 映射同等基本事实。避免把任意原 HTML 直接传给 `dangerouslySetInnerHTML` 或 AppKit HTML importer。若最终采用 sandbox iframe/WKWebView，CSP、导航代理、脚本/表单禁用和资产会话处理器必须成为执行门，而不是只做正则清洗。

- 禁止 script、事件属性、iframe/object、表单、meta refresh、CSS url/import、远程字体和自动外部导航。
- 图片只来自已校验、已保存的本次资产清单；无任意 file/path/http/blob 访问。renderer 私有内存 Blob URL 与来源网页 blob 不混同，替换/关闭必须 revoke。
- 链接首版可展示文字，不自动打开；任何后续点击外部打开功能须单独明确用户动作和 URL 准入。
- 普通图文预览不写明文临时文件，不借用 clipboard materializer；既有 Quick Look 文件路径不作为绕过安全门的通道。
- 保留当前二进制预览通路及身份/长度/hash 验证；新增 document kind/格式时显式版本化，旧命令和严格 decoder 的兼容路径有负向测试。
- 不提高两端 1 MiB API line 上限，不把大图塞进 JSON control line；正文和资产用有界 binary framing，不共享长期 token、不削弱父 PID/socket 认证。

**候选子预算，实施时先核对既有更严格限制：**序列化展示载荷不超过 16 MiB、文字不超过 1 MiB UTF-8、block/run 总数不超过 8,192、表格 cell 不超过 2,048；图像数量/字节/尺寸/累计像素取现有 policy 和候选上限的较小值，累计解码像素候选上限 1,600 万。单次文稿转换候选 watchdog 4 秒，新增文稿转换任务最多 1 个；缩略图进入有界低优先级队列。

这些是拟定准入值，不是已有测量结果，也不是承诺放宽准入。既有 64 MiB 预览上限、256 MiB pipeline budget 和更严格格式限制继续有效；实际编码、解码、中间缓冲都计账。超限明确降级，不能放宽预算使测试通过；解析本身须受节点/深度/尺寸约束，不能先无限解析再截断输出。

## 5. 批次、依赖和交付

当前：S2-00 的源码/隔离 UI 基线已建立，完整真实来源基线仍 UNKNOWN；S2-01 的 F02/F03 确定部分本机 LOCAL_COMPLETE；S2-02—07 **NOT_STARTED**。QQ 的来源基线先在 S2-00 尝试取得，根因修复集中在 S2-06；没有现场条件不阻止通用实现，但阻止 F05 完整验收。

| 批次 | 任务 | 主要依赖 | 退出后交付 |
|---|---|---|---|
| S2-00 | 冻结基线、建立样本和现场条件 | 当前 O05 final | 对照清单、已确认/UNKNOWN、不可覆盖的基线 |
| S2-01 | F02 与 F03 确定部分 | S2-00 | 菜单简化、技术信息下沉及 UI 回归 |
| S2-02 | 共享内容分析和原生提取 | S2-00 | 有界分析、顺序定位、负向/身份回归 |
| S2-03 | F01 分类与 F06 首图 | S2-02 | 展示/筛选一致性、首图、版本化 cache |
| S2-04 | F04 后端预览契约与生命周期 | S2-02/03 | 文稿 binary payload、安全准入、取消/释放 |
| S2-05 | F04 两端 renderer 和完整交互 | S2-04/01 | 文字/双图/表格预览、owner/键盘/AX |
| S2-06 | F05 QQ 闭环与 F01 提示现场验证 | S2-03/05 与真实应用 | 精确根因修复、真实对照或明确 UNKNOWN |
| S2-07 | 总回归、性能、包与证据封存 | 前述已授权批次 | 分层验收报告、剩余问题、停止汇报 |

### S2-00：基线、样本和复现条件

工作：冻结当前源码/dirty 状态；保存分类、菜单、预览、首图的现状证据；核对实际测试包/源码绑定。真实来源操作使用隔离 App/数据，不把旧运行包当作新源码证据。

建立最少样本：纯文字、单 PNG、截图/Preview/浏览器单图、PNG＋替代 URL/alt、图片＋真实说明段落、红图在前蓝图在后、独立绿色 PNG 干扰、重复红图、原生 RTFD 双附件、HTML 归档双图、未归档外链、损坏/超预算图片、基本表格、Partial/Unknown、native-only、有收藏/多 item。

QQ 若可用，固定版本和复制动作，采集单图、文字＋单图、文字＋双图、选区复制与应用菜单复制。优先本地草稿/合成素材，不要求发送消息；不读取真实聊天正文、general clipboard 或应用缓存来替代授权。

退出门：样本可重复；用户报告和已复现根因分开；实际包和原件摘要可追溯。未取得 QQ/系统提示条件登记 UNKNOWN。

### S2-01：低风险交互简化

工作：Mac 普通行去掉 text 格式入口；Windows 对应入口按同一普通行策略保持一致。保留经 `restorable_formats` 允许的图片等次级功能、O04/O05 操作和异常手动恢复。菜单标签不再误称所有操作都是“复制格式”。

默认行隐藏格式名、格式数和资源数；诊断详情仍可查。完整性/风险反馈先保持可见，布局与技术计数解绑。整组恢复、收藏、删除、Space/Return/双击和预览恢复不改变。新增操作或“详情”入口不得造成空菜单或挤占主内容。

退出门：A03；A04 的确定部分；真实 Partial/Unknown、未保存和不可恢复反馈未被隐藏；所有 O04/O05 入口仍可达。测试使用实际生产菜单策略/组件，不能复制一份过滤逻辑后自证正确。

### S2-02：共享内容分析与 native-only 接缝

工作：明确表示选择；提取 HTML/RTF/RTFD 的正文、基本结构与附件位置；为 native-only 提供实际内容事实。复用现有受监督 helper 和原件读取，不在 GUI thread 调用阻塞解码。

先写负向回归：独立 PNG 不是文稿附件、资源顺序不是节点顺序、多个 item 不串接、未知格式不伪造完整、坏 RTFD 不损坏原件、深层 HTML/超限输入在准入阶段停止。对不支持属性允许预览局部退化，不修改便携完整性/恢复门。

退出门：原始 hash/capture/content/dedupe/favorite/source 保持；首图能定位至具体表示和节点/附件；worker 有真实取消/超时边界；同一分析用于后续三个消费者。

### S2-03：展示分类、首图与版本 cache

工作：接入图片/文稿/文字/文件展示；同步新旧历史、收藏、搜索/筛选和计数；原生附件不能因为 portable image_count=0 被漏掉。展示状态未知时允许加载占位，而不是错误宣称纯文字。

统一新旧 get/prepare 图片路径，严格首图定位；加入红/蓝/绿色干扰、去重和归档补图测试。修正两端 cache key 与迟到 in-flight 结果；快速滚动、切历史/收藏、删除/重开库不回显旧图。

退出门：A01 的自动部分与 A08；单图不因三个格式显示图文，真实文字＋图片不降为单图；新旧记录筛选符合约定；首图不可用降级准确；缓存原额度内稳定。

### S2-04：文稿预览后端与安全契约

工作：审计命令/类型/二进制版本；新增有界 document payload，生成 Swift/TS/本地 decoder/注册清单，不手改 generated 文件。控制面只传身份/版本/长度等元数据，资产通过有界 binary 路径传递。

解析、解码、资产处理在 owned worker/helper，计入 admission/lease；不持 DB 锁做大解码。每次响应及 renderer 安装前复验 identity/generation；切项、关窗、删除、归档版本变化、DB reopen 和超时均有测试。

退出门：旧 text/image/file 预览兼容，未知 document 版本 fail closed；拒绝脚本/外源/file/path；原认证和 line caps 不变；取消不会提前释放仍运行任务的租约；没有明文临时文件或 orphan payload。

### S2-05：两端文稿 renderer 与完整交互

工作：Mac 原生受控文字/附件视图和 Windows React 受控文稿视图消费同一基本展示模型；验证中文/emoji、粗体颜色、两图顺序和基本表格。超限/不支持内容给局部反馈，并保留原样恢复操作。

复用独立预览窗口与 owner；单击选中、Space 预览、Escape 关闭、Return/双击整组恢复保持既有行为。搜索/选择文字、IME、焦点和 VoiceOver/键盘可达性按支持范围测试；渲染失败不能让历史窗口进入不可操作状态。

退出门：A05/A06；受控观察证实没有网络请求、clipboard revision 变化或同步事件；连切、关窗、收藏 owner 隔离、图片/文本/PDF 等既有预览无回归。单元测试不代替 GUI/AX 验收。

### S2-06：QQ 与系统提示的真实闭环

工作：对 S2-00 的 QQ 场景逐层定位：来源类型/字节→helper→加密 DB→内容分析→列表/预览→RestorePlan→同目标粘贴。

| 实测根因 | 修复边界 |
|---|---|
| 附件已保存，只是列表/预览没识别 | 共享分析、分类、renderer、首图；不碰 QQ 缓存 |
| 可持久格式在采集/存储路径漏掉 | 修该格式准入与端到端回放，增加真实来源 fixture |
| 临时资源未保存 | 复用 O04/O05 已有显式授权能力；不声称文件桥自动支持 QQ |
| 来源动作只提供文字，直接粘贴也缺图 | 记录来源限制/可用复制方式，不伪造完整内容 |
| 需 QQ 特定例外 | 仅按实测版本/格式/动作匹配，并有正负回归；不全局改写 RTFD/HTML |

提示单独取原文、时间、发出应用与稳定日志码。成功保存不报读取失败；真存储失败/Unknown 仍可见；系统权限提示不通过关权限或关通知“修复”。

退出门：A07 与 A02 的现场部分；同一 QQ 来源、同一目标，历史恢复不额外少图，分类/预览正确；关闭来源/断网/重启结果单列。目标本来不支持的样式不能算 TailSync 保真失败，也不能把“看起来相似”当作对象可编辑性通过。

### S2-07：整体验收、性能与封存

工作：冻结最终产品源码，一组完整门禁绑定同一指纹；需要原生演示时另建隔离候选，绑定实际 helper/App hash，保护日常 App 与现有包。自动/GUI/QQ/Windows 的结果分别保存。

性能覆盖空闲、连续复制、列表滚动、首图加载、快速换项、重复打开文稿、取消和关闭。报告 p50/p95、CPU/RSS、解码/转换次数、cache 命中和最终 lease 释放；普通路径无解释的超过 10% p95 回退作为候选审查阈值，不伪称已有测量基线。

退出门：本方案矩阵逐项登记；无未解释的原件/副作用/安全回归；证据、失败重跑、文档、源码和候选可追溯。停止汇报，不自动进入 S4，不把 S2 完成改成 Release GO。

## 6. 专项回归矩阵

以下是待实现/待执行门，不代表已通过。

| 编号 | 样本/操作 | 必须断言 | 对应 |
|---|---|---|---|
| Q01 | PNG/TIFF/HTML 为同一单图替代表示 | 图片展示；不按格式数判图文 | F01/A01 |
| Q02 | 图片带 URL/alt 与图片带真实说明段落 | 两类有依据地区分；不吞正文 | F01/A01 |
| Q03 | native-only RTFD 含附件、portable image_count=0 | 不漏缩略图资格/图文事实 | F01/F04/F06 |
| Q04 | 新旧历史、收藏、图片筛选与计数 | 分类映射一致；重建不改原件、不 trim | F01 |
| Q05 | 普通行只剩 text 动作 | 没有复制文字和空菜单 | F02/A03 |
| Q06 | PNG、归档/授权/导入/撤销、Unknown 手动恢复 | 必要入口保留且实际服务端接受 | F02/A03 |
| Q07 | 隐藏技术计数；真失败/Partial/Unknown | 诊断可查，失败仍可见 | F03/A04 |
| Q08 | 中文/英文、深浅色、长内容、键盘/AX | 不裁剪主内容；动作可达 | F03/A04 |
| Q09 | 中文 emoji、粗体颜色、双图、基本表格 | 文字/样式/位置在支持范围正确 | F04/A05 |
| Q10 | 红图第一、蓝图第二、独立绿 PNG | 首图为红，不受独立表示干扰 | F06/A08 |
| Q11 | 资源重排、重复图片去重、节点顺序互换 | 首图随文稿顺序变化 | F06/A08 |
| Q12 | 第一图不可用、第二图可用 | 准确降级，不冒称第二图为首图 | F06/A08 |
| Q13 | 同 id 补资源/新 archive revision | cache 失效；旧响应不能覆盖新图 | F06/A08 |
| Q14 | history/favorites 换 owner、DB reopen | 不复用旧库/旧 owner 的资产和授权 | F04/F06 |
| Q15 | script/event/iframe/form/meta/CSS url/import | 被拒绝/剥离；无网络和导航 | F04/A06 |
| Q16 | 外链、file、来源 blob、未保存图片 | 不隐式读取或下载；显示缺失占位 | F04/A06 |
| Q17 | 文稿打开/切换/关闭 | clipboard revision、sync count、原件 hash 不变 | F04/A06 |
| Q18 | 取消、超时、删除、重开、版本变化 | 迟到不显示，实际 worker/资产/lease 最终释放 | F04/A06 |
| Q19 | 大输入/深嵌套/坏图/解压炸弹/多附件 | 准入阶段停止；GUI 不阻塞；额度不放宽 | F04/O12 |
| Q20 | 旧 text/image/file 命令与未知 document 版本 | 旧兼容保留；未知版本 fail closed | F04 |
| Q21 | QQ 单图/文字单图/双图/两种复制动作 | 真实来源—同目标直接/历史粘贴成对 | F05/A07 |
| Q22 | QQ 关闭来源、断网、TailSync 重启 | 持久效果与即时效果分别报告 | F05/A07 |
| Q23 | 成功保存但便携不支持；真实读取/存储失败 | 不误报成功为失败；真失败不静默 | F01/A02 |
| Q24 | 重复预览/滚动/换项/关闭长序列 | 无线性 RSS 增长、不重复无谓解码、预算恢复 | O12 |

## 7. 自动门禁、原生验收与磁盘控制

### 7.1 每批与最终门禁

每批先原复现/新增负向测试，再最小修改、原探针 GREEN、相邻回归。最终按变更范围运行 Protocol、Core default/enabled、DB/integration、Runtime、Mac Rust、Windows-host Rust、Swift、React、TypeScript、fmt、Clippy、契约/命令生成、production feature/visibility 和 diff-check；必须包含 O04/O05 授权/归档/取消非回归。

沿用隔离 runner，以下是当前已核对过的入口示例，不是本阶段已执行结果；聚焦过滤须按实际新增测试名填写，不能过滤掉必要回归：

```sh
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline -p tailsync-core --lib -- --test-threads=1
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline -p tailsync-runtime --all-features --features tailsync-core/protocol-composite-clipboard -- --test-threads=1
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline --manifest-path macos/src-tauri/Cargo.toml --features protocol-composite-clipboard --lib -- --test-threads=1
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline --manifest-path windows/src-tauri/Cargo.toml --features protocol-composite-clipboard --lib -- --test-threads=1
node scripts/run-isolated-tests.mjs -- swift test --package-path macos/swift-ui
node scripts/run-isolated-tests.mjs -- windows/node_modules/.bin/vitest run --root windows
node scripts/check-local-contracts.mjs
cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings
git diff --check
```

enabled Core 的最终矩阵保留已有 repair-regressions/test-support/composite 等必要 feature 组合；生产包不得混入 test-support。默认构建、功能开关关闭态、已升级历史读取均要验证。已有 ignore/skip 与 warning 单列，禁止新增 ignore/allow 或修改断言使结果假绿。

### 7.2 GUI 和实际应用验收

使用隔离、指纹绑定的候选及可公开合成样本。优先 TextEdit/Preview/浏览器，再做实际 QQ；Word/其他目标按可用条件补。对列表、菜单、双图/表格预览、首图、快速切项和失败反馈保存操作记录与必要图像；辅助功能使用实际 AX/VoiceOver 条件验证。

真实 general clipboard、来源登录、QQ 或目标应用操作先确认测试会话和用户内容范围；不用私有 board PASS 代替。无法取得窗口自动化/权限时交付可重复的人工步骤和 UNKNOWN，不虚构观察结果。Windows 原生单列，真实双设备不属于本阶段的替代证明。

### 7.3 构建缓存与磁盘

复用 `target/o04-shared`，不按每批/每个 agent 新建独立大 target。Rust 串行，设置 `CARGO_INCREMENTAL=0`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_BUILD_JOBS=4`；Swift/前端复用已配置缓存，不重复安装现有依赖。

每批记录 target/证据/临时包体积和剩余空间。低空间时先报告、暂停大构建并按已有清理授权范围处理纯构建产物；不删除封存日志、原件 fixture、旧测试包、用户库或其他工作树。失败日志按独立 ID 保存，但不复制无用的大 target 到证据目录。

## 8. DeepSeek/high 实施与主代理复核模式

实施继续沿用已要求的模式，但不是本次写文档时自动派发：

1. 每批只使用一个内置 Multi-Agent 子代理，模型 `cline-pass/deepseek-v4.1-flash`，high，`fork_turns: "none"`；不使用 bash/`codex exec` 后台代理替代。
2. 完整明文 message 包含本批 task id、唯一绝对目录、当前源码指纹、允许修改范围、禁止项、前序状态、具体负向用例、交付和停止条件；不能仅传“参考上文”。
3. 核实当前递送配置和子代理回执：task id、实际 cwd/branch、范围/约束都正确后才允许实施。递送失败不视为开工，不靠无任务代理假交付。
4. 子代理实施并提供 HANDOFF：改动清单、原失败/修复、命令/实际退出码/raw log、已验证与 UNKNOWN、剩余风险。禁止重用 log 文件覆盖失败尝试。
5. 主代理等待完整交接并停止其写入后，再冻结源码、独立构造/运行探针、审查生产调用链、做二次修复；不与子代理同时改同一产品树。
6. 最终同源码重跑、封存并汇报本批。继续下一批须按用户逐项授权节奏；不能把一次 S2 文档请求解释为自动跑完所有后续阶段。

子代理工作期间主代理可以阅读、设计独立探针和核对边界，不提前修改产品；变动中的诊断测试不算最终验收。真实 GUI/用户内容操作不因为委派而扩大授权。

## 9. 证据、失败处理与回退

新证据使用新的 S2 批次/run 目录；不写入或重封 O05 及更早已封存证据。每次命令 ID 不可覆盖，保存 command/env/exit/duration、source before/after、raw log SHA256 和实际断言。

每批交付至少包括：输入/交接/最终指纹、精确增量 patch、独立 RED/GREEN、最终矩阵、原件保全/副作用检查、支持范围、UNKNOWN、二次修复说明、磁盘体积和停止结论。测试数量不得跨重叠 suites 累加成“唯一用例总数”。

回退原则：

- 预览或展示失效时回退到已有文字/图片/unsupported 路径，保留原件和恢复操作，不把 renderer 失败当作存储丢失。
- 派生分类/cache 可重建，不能凭展示失败删除历史、修改原内容或恢复旧 payload 覆盖新归档。
- 新契约保留旧路径；必要版本变更须有 fail-closed 和独立兼容证明。若引入 schema 变更，先审批迁移/回退，不直接运行旧 reader 打开新库。
- 不 stage/commit/reset/clean、不新建分支、不重置整棵工作树。所有已存在的 dirty 工作必须保留；修改回退仅针对本批精确增量。
- 安全边界失败、原件变化、迟到写剪贴板、收藏/删除保护破坏、不可追溯验收或预算失控都是停止门，不能只写“后续优化”放行。

## 10. 仍需明确的产品与现场决策

| 决策 | 本方案默认处理 | 若用户选择其他方案 |
|---|---|---|
| F03 完整性标签去留 | 仅删除格式计数；完整性/真风险反馈不擅自删除 | 单列可评审 UI 变更及失败反馈回归 |
| 首图不可用 | 内容图标/准确占位，不换第二图 | “首个可用图”须明确降级文案和验收 |
| 纯文本粘贴快捷键 | 交给目标应用，不做全局拦截 | 任何新快捷键能力另立范围 |
| QQ 和系统提示现场条件 | 可推进通用工作，但现场部分 UNKNOWN | 用户提供合成样本/可用版本/允许的测试会话后闭环 |
| Windows 原生环境 | 共享契约/React/host 可验证，native UNKNOWN | 有 Windows 后运行原生交互/包验收 |
| 复杂排版/对象和动画预览 | 保留原件；首版明确局部显示边界 | 新能力不能夹带进入 F04 基本支持 |

这些待决策项不会阻止 S2-00/01 的确定部分。没有完整性 UI 新决定时，只报告 F03 已完成的子范围，不宣称用户保留意见已经解决。

## 11. 最终完成口径与建议首批

- **IMPLEMENTED：**生产接线存在，但未完成全部验证。
- **LOCAL_COMPLETE：**当前冻结源码下自动、持久化与所列接缝门通过；真实应用/GUI缺口仍单列。
- **S2 已验证范围完成：**F01–F06 的已授权子范围及 A01–A08 对应自动/GUI/来源目标条件逐项满足；任一真实 QQ 条件缺失，不能称完整 F05 或完整 S2 现场闭环。
- **F03 部分完成：**格式信息减负完成，完整性呈现仍待决策；必须明确此保留项。
- **Release NO-GO：**即使 S2 本机和目标应用完成，也不替代 Windows/双设备/CI、R7 或发布许可。

首批已按 **S2-00＋S2-01 的明确范围**实施：固定源码/隔离 UI 基线，交付“移除普通行复制文字、隐藏格式计数、不动完整性决定、不破坏 O04/O05 操作”。已完整交接、独立复核、二次修复并通过 15 条同源码最终门禁，真实来源/GUI 仍 UNKNOWN。本批停止，下一项为 S2-02；不一次让子代理包办全部 S2。

## 12. 依据

- [总方案：F01–F06、A01–A08 与 S2](COMPOSITE-CLIPBOARD-IMPROVEMENT-PLAN-2026-10-06.md)。
- [执行记录：第 17/18 节的最新 O04/O05 状态](COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md)。首页旧总表不作为新阶段完成依据。
- [原始用户反馈：F03 完整性状态保留意见](COMPOSITE-CLIPBOARD-USER-FEEDBACK.md)。
- [O05：当前授权、来源证明、生命周期及安全边界](COMPOSITE-CLIPBOARD-O05-SOURCE-CONTEXT-2026-10-06.md)。
- [ADR-002：独立预览窗口与有界载荷](adr/ADR-002-independent-history-preview-window.md)。
- [ADR-004：复合原件/恢复契约](adr/ADR-004-composite-clipboard-contracts.md)。
- [ADR-005：内容身份与归档版本](adr/ADR-005-composite-content-identity.md)。

本文件是持续执行方案；实际实施/验收以独立首批报告和封存证据为准，文档写入本身不代表用例通过。本次只更新此新方案的首批检查点，不改动 O05 封存文档或证据。
