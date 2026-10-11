# TailSync 剪贴板反馈、调研与当前差距汇总

日期：2026-10-06，Asia/Shanghai。

后续进度：用户已授权逐项实施，每项完成暂停汇报。S0 本机基线与接缝复现结果见[执行记录](COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md)；本文保留汇总时的证据分类，具体复现不能自动等同于用户现场根因。

配套文档：[逐项改进方案](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/COMPOSITE-CLIPBOARD-IMPROVEMENT-PLAN-2026-10-06.md)。

## 1. 本文结论与范围

用户的核心要求是：**同一来源、同一目标下，通过 TailSync 历史恢复后粘贴，不应比直接粘贴额外丢失内容、格式、图片位置或对象可编辑性。** 内容不完整时增加标签，不算完成这项要求。

本次整合六项用户反馈、两份外部调研文本，以及本轮对当前代码的核对。六项反馈保留 F01—F06；从调研和代码差距中整理出的十二项优化方向编号为 O01—O12。它们是设计输入，不是十二个均已复现的产品缺陷。

用户于本轮授权整理文档并提出方案。本次交付范围为文档，不包含产品实现、安装或发布。用户对完整性状态仍保留意见，不能据此认定已决定删除所有相关 UI 或后台状态。

原始反馈记录继续保留：[用户反馈原始台账](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/COMPOSITE-CLIPBOARD-USER-FEEDBACK.md)。

## 2. 当前基线与证据分类

### 2.1 本地基线

| 项目 | 本轮核对值 |
|---|---|
| 实际工作树 | /Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35 |
| 分支 | codex/composite-clipboard |
| HEAD | f719d581a1aa5198a1b3f2483222df5c69e3dfc0 |
| 状态 | 有未提交的实现、测试和文档；HEAD 单独不能标识候选 |
| 产品/测试/工具源码指纹 | cd9268e75f38d52431f985ef8dadd8c4c910eee3b09f638d289fa71a007ec1eb，756 文件 |
| 会话中提供的 Mac 测试候选 | 2.3.2，2026-10-05-225456-composite；复合功能开启，universal |
| 指纹范围 | 采用现有 composite-evidence.py 方法；Markdown/docs 不参与此源码指纹 |
| 用户反馈的包绑定 | 尚未独立核验用户实际运行包、来源应用版本及完整操作序列 |

本轮重算了源码指纹并读取关键生产路径，没有重跑完整 R6，也没有操作用户系统剪贴板、QQ 或目标应用。此前 R6 本地证据及 R7 范围见[单 Mac 验收报告](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md)。本次文档不能形成新的产品或设备验收 PASS。

### 2.2 用语

| 分类 | 含义 |
|---|---|
| 用户报告／待复现 | 用户实际观察，尚未取得本轮可核对的复现证据 |
| 用户偏好 | 产品交互要求，不需要包装成代码缺陷 |
| 代码已确认 | 本轮读取的生产路径确实存在该行为；不自动证明某次用户症状由它导致 |
| 已有基础 | 当前有实现接缝；不等于指定应用、Windows 或双设备验收通过 |
| 拟议优化 | 值得实施的设计方向，尚未实现 |
| 保留意见 | 用户尚未作最终决定，不能擅自改写成确定需求 |

## 3. 六项用户反馈

### F01：单图被判断为富文本，复制出现系统提示

**原话：**“现在单独复制一张图片也会被判断为富文本，而且复制还会弹出系统提示”。

- 性质：两个现象，均待复现，分别记为 F01-a 分类与 F01-b 提示。
- 目标：单张图片应直观显示为图片；正常采集和本机可恢复内容不应触发误导性失败提示。
- 相关代码事实：所有便携复合内容使用 composite 存储类型；分类取决于便携 PNG/资源。另有原生保存成功后仍记录 capture_failed 的分支（E04）。
- 尚未确认：用户所说“富文本”对应哪个标签；提示是 TailSync 通知、复制失败反馈，还是系统权限提示。E04 不足以证明 F01-b 的具体原因。
- 关联方向：O01、O02、O10；配套方案 F01。

### F02：“复制文字”按钮多余，倾向纯文本粘贴快捷键

**原话：**“加入复制为文本按钮很多余，我认为可以command+shift+V纯文本粘贴就可以了”。

- 性质：交互简化偏好。
- 目标：普通历史恢复以完整内容为默认，减少行内格式操作。
- 事实边界：Command+Shift+V 由目标应用解释，不是所有 Mac 应用统一支持的纯文本粘贴能力；部分应用使用其他组合键。
- 设计方向：移除普通历史行的“复制文字”入口；保留后端显式格式能力，不注册抢占目标应用的全局纯文本快捷键。没有剩余操作时，移除空的省略号菜单。
- 关联方向：O06；配套方案 F02。

### F03：历史下方格式信息无用，完整性状态暂保留意见

**原话：**“历史记录下面的信息必要性不大，我认为显示有什么格式是无用的，还有我对完整性状态暂时保留意见”。

- 性质：弱化技术信息的偏好；完整性状态的去留尚未确定。
- 目标：列表优先展示内容、缩略图与实际恢复操作；技术字段不占据日常浏览位置。
- 当前显示：格式数、图片资源计数、仅本机恢复或内容不完整（E07）。
- 设计方向：普通行隐藏格式元数据；完整性呈现提供单独设计建议，保留用户审阅决定。后台读取、存储、写入和回执事实继续独立保存。
- 关联方向：O06、O10；配套方案 F03。

### F04：富文本预览无法有效使用

**原话：**“现在的预览功能对于富文本内容毫无办法”。

- 性质：功能缺口报告，具体样本待复现。
- 代码事实：现有预览 material 包含文字、图片、PDF、Quick Look 和 unsupported，没有专用复合图文 material（E06）。
- 目标：能阅读文字与图片，并理解图片位置及基本样式；预览派生数据不覆盖恢复原件。
- 边界：预览支持度与原生恢复保真度分别验收，不要求第一版渲染任意应用专有对象。
- 关联方向：O01、O04、O06；配套方案 F04。

### F05：QQ 的文字＋图片消息被归为文本

**原话：**“QQ复制的富文本消息（文本+图片）在历史记录中归纳为文本”。

- 性质：真实来源应用兼容性问题报告，待复现。
- 需要分开的判断：来源有没有发布图片数据；TailSync 有没有采集/保存；保存的原生附件有没有被展示逻辑识别；恢复后的粘贴是否正确。
- 相关代码事实：native-only 历史的便携部分是文字摘要；分类和 image_count 主要观察便携图片，因此原生附件仍可能存在而界面显示为文本（E03）。
- 不成立的推论：不能只改“富文本”标签便宣布 QQ 图片保存成功；也不能仅从“文本”标签断言图片丢失。
- 关联方向：O01、O02、O04；配套方案 F05。

### F06：含图富文本应使用第一张插图作为缩略图

**原话：**“含有图片的富文本的缩略图我认为应该用富文本中的第一张图片而不是显示文本图标”。

- 性质：内容识别与视觉展示要求。
- 目标：按原内容出现顺序，选取第一张实际可读取的图片；单图条目正常展示该图。
- 当前限制：图片读取优先找独立 PNG，其次取资源存储顺序中的首项（E05）；没有统一覆盖 HTML 节点顺序和原生附件位置。
- 边界：同一资源可以在文稿中重复出现；资源去重顺序不能代替文稿顺序。未保存/不可解码图片不能冒充真实缩略图。
- 关联方向：O01、O04；配套方案 F06。

## 4. 两份调研的来源与可采用结论

### 4.1 来源

| 编号 | 文件与适用范围 | SHA256 |
|---|---|---|
| R-A | [原始格式保留与资源持久化调研](/Users/monet/.codex/attachments/0881566f-90b5-40e5-9e7a-8ce9b5f0b332/已粘贴的文本.txt)；以会话描述的本地实现为参照 | 60c15b3efb5e64646123a6487fc7c5635445a995fb2383794aa38e7509652682 |
| R-B | [技术栈与内容包方案](/Users/monet/.codex/attachments/594fa29e-a069-477d-890e-792a1f00e1ca/已粘贴的文本.txt)；主要参照公开 main，本地实现已经前进 | 2c2446ddd8c5452db4f10a66b0178bd31b02fd5a85cd08d71d1036709286d760 |

这两份调研是方案参考，不是新增用户症状。文中竞品结论基于其引用的公开资料；本轮未逐项复核竞品当前提交、许可证或做横向实机测试。实施某项借鉴时须固定具体来源版本，并以 TailSync 回归样本验收，不能直接继承竞品能力声明。

### 4.2 两份调研的共同主张

1. 原生格式、兼容表示、依赖资源分别保存；预览、搜索和分类是派生结果。
2. 能保存、能同平台回放、能跨平台转换是三种能力；不会转换不能成为删除原始数据的理由。
3. 一次复制保留条目边界与各条目格式集合；恢复前准备数据，再集中发布。
4. 内嵌图、原生附件优先复用；外链资源在来源端按策略取得实际字节，并绑定正文位置。
5. 多格式集合不自动等于自包含富文本；独立 PNG 不能凭空补全 HTML 的图片位置。
6. 复制版本、操作 ID、资源提交和迟到任务需要统一因果关系；补图完成不能重写后来复制的内容。
7. 默认操作围绕完整内容恢复；具体失败给出真实可执行的动作。
8. 同来源—同目标的即时保真，与关闭来源/重启/离线后的持久恢复，分别验收。

### 4.3 竞品参考如何分工

| 参考 | 可借鉴方向 | 不据此作出的承诺 |
|---|---|---|
| Maccy | Mac 条目级原始类型读取、回放及有复现支撑的例外规则 | 任意私有类型都可持久化，或应照抄其条目合并规则 |
| Ditto | 同条记录的格式名称和字节集合、Windows 注册格式处理 | 默认保存全部私有格式，或任意网页图片离线归档 |
| CopyQ | 整条恢复与显式格式转换分开 | 其目录同步等于 TailSync 的跨设备协议 |
| CrossPaste | 内容集合、展示与保存分离、采集版本一致性 | 类型列表代表任意对象跨平台等价可编辑 |
| SyncClipboard | 来源端取得浏览器图片实际内容 | 已解决任意选区的多图、样式和登录资源归档 |
| Paste | 默认保留格式、内容优先的交互体验 | 未公开的内部采集、私有格式或资源归档细节 |

### 4.4 旧 main 判断与当前实现的差别

| R-B 中针对旧实现的建议 | 当前代码位置／事实 | 本次处理 |
|---|---|---|
| 改文字优先读取，避免跳过图片 | Mac 开启态已有稳定快照与复合分支；legacy 分支仍存在作兼容用途 | 复用开启态链路，检查真实来源保真 |
| 新建原生快照 helper | CNS1、NativeSnapshot、原生恢复均已有 | 扩大保留范围，隔离派生结果 |
| 为历史添加多格式内容 | v12 composite、history_parts、native parts 已有 | 增量补元数据与生命周期，不推倒存储 |
| 新建整组发布接口 | ClipboardPort、PreparedRestore、native/composite 发布已有 | 增强选择与准备规则 |
| 新增复合协议和能力协商 | CompositePayload / Chunk / Ack 及能力协商已有 | 对原生传输与资源块扩展显式演进 |
| 建立版本校验、防重复和持久回执 | snapshot revision、来源分类、effect journal、generation 已有 | 保留并扩展回归，不把它们全部列为未实现 |

## 5. 当前代码差距及对应优化方向

| 编号 | 优化方向 | 当前事实／差距 | 关联反馈 |
|---|---|---|---|
| O01 | 原始表示与派生表示隔离 | RTFD 转换可替换快照中的原 HTML/文字；TIFF 派生 PNG 也混入同一集合 | F01、F04、F05、F06 |
| O02 | 广泛原生保留及明确准入 | helper、数据库和回放均有固定格式限制；扩大支持必须贯通三个环节，并检查敏感标记及读取预算 | F01、F05 |
| O03 | 原始图片编码与动画保留 | 原生范围主要含 PNG/TIFF；便携 PNG 解码拒绝动画，不能据此声称保存 GIF/WebP 原始语义 | F01、F04 |
| O04 | 图片/附件资源归档 | 已有 PNG 资源与绑定；外链、其他附件与资源获取流程尚不完整 | F04、F05、F06 |
| O05 | 登录态及 blob 来源适配 | 通用 URL 获取不能取得所有来源上下文；拟议可选来源端适配，不同步凭据 | F05 的可能长尾原因，尚未确认 |
| O06 | 恢复计划和目标适配 | 已有 Native/Portable 选择；尚需明确自包含程度、原始与归档版本选择及经过实测的目标组合 | F02、F03、F04 |
| O07 | 同平台原生同步与能力细分 | CNS1 当前是本地 helper 协议；复合网络包主要传便携表示，native-only 分支本地保存后停止发送 | F05 的跨设备延伸 |
| O08 | 资源清单、缺块传输和生命周期 | 已有复合分块及编码缓存；尚未形成独立资源缺块协议，不能混同文件事件 | F04 的大图/多附件延伸 |
| O09 | Windows 原生与 OLE 适配 | 已有 Win32 常见格式/CF_HTML/DIB 基础；更复杂对象、虚拟文件需明确适配和 Windows 原生验收 | 跨平台方向 |
| O10 | 保留因果规则，区分失败和能力范围 | 原生历史保存成功仍记录 capture_failed；新增补图、预览、转换需遵守已有版本/claim/generation 边界 | F01、F03、F05 |
| O11 | 完整内容去重与版本/迁移兼容 | 非 native-only 条目当前以便携 bundle 摘要去重；不同原生集合可能被便携摘要合并；新元数据、资源补齐需保持稳定事件身份 | F04、F05、F06 |
| O12 | 真实应用与持久恢复验收 | 单元、helper 和字段匹配不证明 QQ/Word/浏览器实际粘贴；需补来源—目标和关闭来源/离线矩阵 | F01—F06 |

### 5.1 代码证据目录

| 编号 | 本轮确认的行为 | 入口 |
|---|---|---|
| E01 | 固定 allowedTypes；读取原生项目并执行 RTFD/TIFF 转换 | [Mac helper](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/src-tauri/clipboard-helper.swift:20) |
| E02 | 转换成功时跳过来源 HTML/文字，再写入转换结果 | [readSnapshotData](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/src-tauri/clipboard-helper.swift:174) |
| E03 | native-only 使用文字摘要；分类看便携 PNG/资源 | [add_native_snapshot](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/db/composite.rs:164)、[分类](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/db/composite.rs:395) |
| E04 | native-only 成功保存后记录 capture_failed，并不广播 | [Mac 采集分支](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/src-tauri/src/clipboard.rs:395) |
| E05 | 缩略图取独立 PNG，或首个存储资源 | [get_composite_image](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/db/composite.rs:504) |
| E06 | 预览 material 尚无专用 rich/document 类型 | [预览 material](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/swift-ui/Sources/TailSync/Models/HistoryPreviewData.swift:102) |
| E07 | 行内常驻格式/图片数量及范围标签；提供 text/png 格式菜单 | [历史行信息](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/swift-ui/Sources/TailSync/Views/HistoryRow.swift:120)、[格式菜单](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/swift-ui/Sources/TailSync/Views/HistoryRow.swift:258) |
| E08 | native parts 保存范围有第二层白名单 | [原生入库过滤](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/db/composite.rs:270) |
| E09 | bundle 内资源为 PNG；HTML 提取不联网；便携 PNG 不接受动画 | [PortableBundle](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/tailsync-protocol/src/composite.rs:206)、[HTML 提取](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/composite/html.rs:225)、[PNG 解码](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/composite/mod.rs:18) |
| E10 | 默认恢复优先原生；CNS1 不发送给 peer | [恢复选择](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/tailsync-runtime/src/history.rs:348)、[本地协议](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/tailsync-protocol/src/native_clipboard.rs:1) |
| E11 | 非 native-only 摘要来自便携 bundle；重复非收藏记录可能被替换 | [摘要与去重](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/db/composite.rs:208) |
| E12 | 分块、能力协商、准备历史后 claim、保存不等于发布等接缝已有 | [分块](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/composite/chunks.rs:1)、[能力](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/secure.rs:24)、[接收流程](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/peer/composite_receiver.rs:245) |

E01—E12 是代码事实，行号随实现变化可能移动；F01/F05 的具体用户根因仍待真实样本确认。当前发现不意味着旧 R1—R6 的每项测试失败，也不意味着真实来源保真已经通过。

## 6. 总体产品目标与优先级建议

1. 优先保存原始数据并做好本机回放，消除管理器引入的损失。
2. 图文应可识别、可预览，首图缩略图和简化信息帮助用户选择内容。
3. 将已保存的资源做成可持久恢复内容；外链归档与即时恢复分开承诺。
4. 同平台原生同步、跨平台转换、复杂对象逐步扩展；继续使用现有 SwiftUI/Tauri/Rust 架构。
5. 区分收到、持久保存、系统写入与目标实际粘贴；后台事实不可删，普通界面无需常驻所有技术状态。
6. 每个优化由配套方案编号对应，结果须落到实测，不以标签、截图或“支持格式”名称代替。

具体步骤、验收用例和依赖见[第二份文档](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/COMPOSITE-CLIPBOARD-IMPROVEMENT-PLAN-2026-10-06.md)。
