# TailSync 剪贴板逐项改进方案

日期：2026-10-06，Asia/Shanghai。状态：S0、O11-D1/D2、O01、O02、O06、O10、O03、O04 及各轮审查整改的本机部分完成。最新 O05 由一个 DeepSeek/high 子代理完整交接，主代理独立证实六项缺陷并二次补修，29 条同源码最终门禁通过，达到可选手动文件桥的本机 LOCAL_COMPLETE。仅同源静态 PNG；真实浏览器／登录/blob／GUI／目标 App、Windows 原生及设备验收仍 UNKNOWN。完整 A15 未完成，本阶段停止，全部可追溯历史 FAIL 保留，Release NO-GO。

逐项进度和本轮效果：[执行记录](COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md)。

问题及证据：[反馈、调研与差距汇总](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/COMPOSITE-CLIPBOARD-FEEDBACK-AND-RESEARCH-2026-10-06.md)。

## 1. 目标、约束和采用原则

**首要验收目标：同一来源、同一目标、同一网络条件下，历史恢复不比直接粘贴额外丢失内容。** 图片位置、链接、表格结构及对象可编辑性分别检查。将可编辑对象变成相似截图不能算原样恢复。

第二层目标是关闭来源、重启 TailSync/电脑、断网及跨设备后的持久恢复。这需要实际资源保存和明确兼容范围，不由第一层通过自动推出。

沿用当前 NativeSnapshot、PortableBundle、history_parts、ClipboardPort、effect journal、能力协商、复合分块与预算机制。未在执行记录标为已实施的新增字段、接口或协议名仍为**拟议设计**，不能作为现有能力宣传。实施前须检查当前注册表及 schema，不复用冲突消息值。

初稿授权仅为准备文档与方案。用户随后于 2026-10-06 授权逐项实施，并要求每完成一项任务就暂停汇报；以下“步骤”和“完成标准”按该节奏执行。保留当前工作树、日常 App、历史及已封存证据，不直接改动用户库来验证。

### 1.1 目标数据流

~~~mermaid
flowchart LR
    A[来源剪贴板稳定采集] --> B[原始项目与格式字节]
    B --> C[加密历史及资源引用]
    B --> D[分类 缩略图 预览派生]
    B --> E[兼容表示与资源归档]
    C --> F[准备恢复计划]
    E --> F
    F --> G[检查资源和版本]
    G --> H[集中写入与持久回执]
~~~

原始表示保持不可变；派生内容不能替代原件。归档资源可以增补，但事件身份稳定，不自动重写当前剪贴板。

### 1.2 逐项映射和建议优先级

P0 表示原样保留、数据身份或真实恢复的基础；P1 表示直接可见体验与常见资源能力；P2 表示按设备和来源条件扩展。优先级是本方案建议，不是已完成状态。

| 项目 | 建议优先级 | 主要依赖 | 交付 |
|---|---|---|---|
| F01 单图分类与提示 | P1；真采集失败根因可升 P0 | O01、O10；显示识别依赖 O02 的实际内容 | 正确图片展示、提示归因 |
| F02 移除普通行复制文字入口 | P1，可独立先做 | 无数据模型依赖 | 简化主操作 |
| F03 弱化技术信息 | P1 | O10；完整性呈现保留审阅 | 内容优先列表 |
| F04 富文本预览 | P1 | O01、O02、O11 | 图文可阅读、无副作用预览 |
| F05 QQ 图文 | P0 复现；修复按实测根因安排 | O01、O02、O11；需要 QQ 来源样本 | 来源兼容回归 |
| F06 第一张图片缩略图 | P1 | O01、F04 的内容提取 | 正确首图与顺序 |
| O01 原始/派生隔离 | P0 | O11 的兼容存储方案 | 未改写的原件 |
| O02 扩大原生保留 | P0 | O01、O11、O10 | 有界不透明格式留存 |
| O03 原始图片与动画 | P1 | O01、O02 | 原始编码留存及静态兼容 |
| O04 资源归档 | P1 | O01、O06、O10、O11 | 自包含持久图文 |
| O05 来源上下文适配 | P2 | O04；来源端授权和适配条件 | 登录/blob 等场景增强 |
| O06 恢复计划与目标适配 | P0 | 沿用现有 Prepare/Publish，扩展 O01/O11 | 明确的输出集合 |
| O07 原生同步 | P2 | O01、O02、O06、O11；第二台设备 | 同平台原始内容同步 |
| O08 资源块传输与生命周期 | P2；已有资源生命周期回归随 O04 | O04、O07/O09、O10、O11 | 去重、续传、完整提交 |
| O09 Windows/跨平台 | P2 | O06、O11；Windows 原生环境 | 五类常见内容实测 |
| O10 因果规则及提示语义 | P0 | 保留现有 journal/revision 接缝 | 无串内容、无迟到覆盖 |
| O11 身份、去重、迁移兼容 | P0 | 当前 schema/manifest/协议审计 | 不误合并、不丢旧数据 |
| O12 验收与性能 | 全程 | 逐项随实施增加样本 | 对照证据与范围 |

## 2. 六项用户反馈的改进方案

### F01：单图分类与系统提示

**问题边界：**分类与提示分开复现，不能将所有提示归咎于复合状态。

**步骤：**

1. 使用截图、Preview、浏览器复制单图三类来源，记录来源版本、实际包 hash、条目/类型集合及直接粘贴结果。先确认“富文本”是哪个界面字段。
2. 引入展示语义判定，独立于存储 type=composite 和格式数量。单图的 PNG/TIFF/HTML 替代表示不自动构成图文文稿；有实际文本段落、附件顺序或富文本结构时才采用文稿展示。文件集合继续走文件规则。
3. 对只有文件名、URL、替代文字的图片条目，依据实测来源规则识别，不能用“存在 text 就是富文本”的条件。图片附带真实说明段落的场景另作样本。
4. 对提示取得时间、原文、发出应用和对应日志码。TailSync 成功/范围提示与真实 capture_failed、capture_not_saved、publish_unknown 分开；系统权限提示只解释来源，不通过关闭系统保护“修复”。
5. 将原生保存成功但不能跨平台转换的分支改为范围结果，不在本机发采集失败通知；真实保存失败和未知写入仍可见。

**落点：**Core 分类/展示 DTO、db/composite.rs、Mac clipboard.rs、HistoryEntry/HistoryRow、sync_warning 与通知消费路径。若 Windows DTO 共用，生成契约并保持旧端兼容。

**完成标准：**A01/A02；单图显示图片，默认恢复可粘贴到 Preview；正常成功无 TailSync 失败通知。提示来源和残余限制有记录，不以通知权限关闭作为通过。

### F02：简化“复制文字”操作

**拟采用方案：**从普通 Mac 历史行移除“复制文字”菜单项；没有其他有价值操作时不显示空省略号菜单。后端显式格式接口及规则继续保留，避免将 UI 精简变成数据或 API 降级。

不新增全局 Command+Shift+V 拦截。目标应用的纯文本粘贴继续由目标处理，说明中不承诺该快捷键普遍适用。是否保留“复制图片”等次级功能单独评估，不把删除文字项自动扩大为删除所有格式功能。

**落点：**HistoryRow 的 text/png 菜单生成、HistoryView 绑定、菜单/localization/AX 测试。普通双击、Return、预览恢复与异常“再次复制”入口保持整组语义。

**完成标准：**A03；普通行没有“复制文字”，无空菜单；整组恢复仍保留图文；目标应用支持的纯文本粘贴不会被 TailSync 抢占；Unknown/Partial 手动恢复入口仍可用。

### F03：历史列表信息减负

**确定可拟改部分：**默认行移除格式名称/数量等技术信息。缩略图、摘要和必要的时间/来源信息优先；是否保留大小按列表空间统一设计。诊断详情保留原始字段。

**完整性呈现建议，待用户审阅：**

- 本机可正常恢复时，不常驻“仅本机恢复”失败式标签。
- 不用泛化“内容不完整”取代具体说明。只有影响用户当前恢复或持久保存预期时，显示例如“图片尚未保存到本地，离线可能缺失”。
- 真实 Partial/Unknown、存储未保存等结果仍需要明确反馈与可执行操作。

用户仍保留完整性状态意见。因此实施可以先完成格式信息减负；去除/移动完整性 UI 单列可评审变更，不擅自认定已经获准。后台 completeness、native-only、回执与原因不删除。

**落点：**HistoryRow 信息布局、HistoryView 动作反馈、Loc 文案；数据契约保留。

**完成标准：**A04；中英文、深浅色、键盘与辅助功能可读；诊断可查看；失败反馈不会被布局精简隐藏。

### F04：有效的富文本预览

**步骤：**

1. 增加复合文稿预览能力，使用 O01 保留的原始表示与附件。Mac 原生 RTF/RTFD 在受监督 helper 中解码、提取文字样式和附件；输出有界的展示数据。此展示模型仅服务预览，不升级为恢复正文或全格式通用文档模型。
2. SwiftUI 承载原生文本/附件视图，优先展示文字、基本样式、段落和图片顺序。复杂样式即使预览能力有限，也不能覆盖原始数据或阻止原生恢复。
3. HTML 预览使用独立清洗后的表示。若采用 WKWebView，关闭脚本/表单/插件和外部导航，拦截外部网络、file/blob 来源读取；本地资产通过预览会话专用资源处理器按清单提供，不给任意路径访问能力。
4. 预览 API 根据保存内容选择 renderer，不仅依据列表 category。对原生附件提供实际图片，不将 native-only 的文字摘要当作完整预览。
5. 复用现有取消、generation、临时文件与缓存机制；关闭预览释放 lease。损坏/超限预览给出局部反馈，历史原件仍可按恢复规则使用。

**落点：**db/preview.rs、db/composite.rs、runtime history 预览接缝、契约生成、HistoryPreviewMaterial/ViewModel/View、Mac helper 的预览转换入口。上述新入口须版本化。

**完成标准：**A05/A06；中文 emoji、粗体、颜色、双图和基本表格可识别；原文与图片顺序正确；打开/关闭预览不改 clipboard revision、不发同步、不修改历史原件；外链预览无网络请求；迟到结果不显示到新选中项。

### F05：QQ 图文来源闭环

**先复现再确定适配，禁止只改标签交差。**

1. 固定 QQ/macOS/测试包版本，使用已有消息或本地草稿样本，记录单图、文字＋单图、文字＋双图、选区复制与应用“复制”动作的差别。此流程不要求发送消息给其他人。
2. 对同一目标比较直接粘贴与历史恢复；保存允许的诊断类型、长度、摘要与附件位置，合成样本才保存正文。确认是显示误判、原生附件未展示、格式漏采、临时资源缺失，还是来源复制方式只提供文字。
3. 原生附件已保存：修 F01/F04/F06 的识别链；漏采可持久格式：通过 O02 贯通采集和回放；资源缺失：采用 O04/O05 的来源适配。
4. 对确有必要的应用例外，按“实测来源＋格式＋动作”定义规则和回归，不能全局改写所有 HTML/RTFD，也不能未经授权读取 QQ 私有缓存/凭据。
5. 来源根本没提供图片且直接粘贴也缺图时，记录来源限制和可用复制方式；不能伪造完整记录。

**落点：**helper/DB 原生链、内容识别、预览/缩略图；QQ 适配只有在证据明确后才建立。

**完成标准：**A07；同目标直接粘贴可得的图文，历史恢复不额外少图；分类和预览正确；关闭来源后测试单列；缺 QQ 条件时标记待设备/应用验证，不用合成 RTFD 宣称 QQ 通过。

### F06：按文稿顺序生成首图缩略图

**步骤：**

1. 从 HTML 图片绑定的节点顺序、RTFD 附件字符位置、原生项目顺序提取第一张可用图片。使用统一的首图解析规则。
2. 文稿的独立 PNG 替代表示不能压过正文中实际第一张插图；单图条目可优先用自身高质量图片表示。
3. 首图定位保存 representation/节点序号/resource 引用等派生信息；按内容摘要去重图片不会改变图片出现顺序。
4. 在有界 worker/helper 中生成缩略图，复用现有 48pt 布局和缓存预算；原生附件不能只因便携 image_count=0 而跳过。
5. 无可用字节或解码失败时使用准确的内容图标，不下载外链，不用无关的第二张图片冒充首图；“首图不可用，展示首个可用图”若采用，需明确是降级规则。

**落点：**get_composite_image/prepare_composite_image、内容提取、HistoryEntry.hasThumbnail、HistoryRow 和缩略图 cache。

**完成标准：**A08；首图为红、第二图为蓝的样本显示红图；调换图片顺序后更新；重复图片去重、资源存储顺序变化及存在独立 PNG 时均不选错；原生 RTFD 附件有缩略图。

## 3. 十二项技术优化的改进方案

### O01：原始表示不可变，派生内容另存

**措施：**采集先记录来源实际提供的 item/type/bytes；RTFD→HTML/text、TIFF→PNG 进入独立派生集合。原始 HTML、文本、RTFD 和 TIFF 不因转换成功被替换。补来源平台、原始顺序、派生来源/转换器版本等元数据；由 O11 定义版本和兼容规则。

原件即使不能生成 PortableBundle，也能通过原生存储路径保存。便携资源提取可修改派生 HTML，但保留原件。显式转换不能污染下一次默认原生回放的集合。

**落点：**Mac helper readArchiveData、NativeArchive/CNA1 IPC、Core from_archive、Manifest/parts、runtime PreparedRestore。macOS helper 协议现为 3，Windows 保持 2；健康探测拒绝旧 helper，旧 CNS1 发布接口保留。

**本机进展：**O01 已实现并完成本项自动验证，详见[执行记录第 9 节](COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md#9-o01原件与派生表示隔离)。七组实际新包 helper→DB 重启→私有 board 回放逐字节保留原件；显式兼容转换不影响默认原件恢复。未完整取得原件时默认恢复在发布前拒绝，仍可选择有效的显式兼容格式。A09 的 TextEdit 等真实目标验收尚未完成；O02 的合成原生证据见第 10 节。

**完成标准：**A09；构造 RTFD 与原 HTML/文字不同的样本，采集/入库/重启/恢复后原始格式关键字节摘要一致；派生数据可重建且不替换原件。只有 TailSync 自身来源标记、revision 等明确排除项允许变化。

### O02：扩大原生采集与回放，建立精确准入规则

**措施：**

1. Mac 按 item.types 枚举，优先采集常用表示，再在总预算内采集可安全持久化的不透明字节类型。保留类型名称和条目边界；不合并独立项目，不按第一条截断。
2. helper、数据库过滤和 write-native 接受规则同时改造。类型不能因为未知就丢弃，也不能因为存在数据就宣称所有类型可回放；已知临时/对象依赖类型使用实测例外。
3. 在读正文前检查禁止记录/敏感标志的元数据；阻止整条采集时不剥掉标记后继续保存。来源应用身份无法可靠确定时不把当前前台应用冒充剪贴板所有者。
4. 常用格式优先；可选格式受单格式、总字节、项目、类型数和时间上限约束。慢 provider 放在受监督进程，终止后确认资源回收；只接纳完整帧且 revision 校验通过的数据。
5. 对不透明内容仅按字节保存和经允许的平台接口回放；不任意反序列化/执行。策略规则精确到格式及可靠来源条件，并有样本依据。

纯文件选择继续沿用文件管线。混合项目中的文件 URL、附件路径属于带依赖的表示，必须由文件/附件物化器核验和重建；不能将一个路径字符串当成资源已归档，也不能通过扩大原生名单绕过现有文件授权和生命周期规则。

**落点：**helper、db/composite、native codec、ClipboardPort/平台 resolver；Windows 普通内存格式与 OLE 另见 O09。

**完成标准：**A10/A11/A16；合成自定义字节类型经重启同平台回放仍一致；敏感标记整条无历史/日志正文；未知慢格式不挂 GUI、不串到后续复制；额度拒绝不误报完整。

**本机进展：**O02 已完成有界不透明字节采集、存储拒绝与同平台回放；四种精确敏感标记在读正文前阻止整次采集。A10/A11/A16 的合成原生 provider、生产 supervisor、DB 重启与私有回放证据见[执行记录第 10 节](COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md#10-o02原生字节准入与敏感采集阻止)。未知类型仍无安全内容去重键；已知文件/承诺依赖未物化，真实来源/目标应用及 Windows/双设备仍 UNKNOWN。

### O03：保留原始图片编码及动画

**措施：**在原生/资源层保存来源提供的 PNG/TIFF/JPEG/GIF/WebP 等实际编码和 MIME/尺寸/帧信息；必要时生成静态 PNG 兼容表示或预览封面，记录为派生。来源只给静态像素时不声称恢复动画。

不得为了扩大动画支持直接放开现有静态 PNG/APNG 解码限制。原始数据留存、静态兼容解码和动画预览分别预算、分别实现；多帧解码按累计像素/帧数准入。第一版可先保存和回放动画原件，预览只显示明确的封面。

**落点：**原生类型枚举、资源元数据、图片 prepare、缩略图/预览与 O11 的 schema；跨平台新编码必须协商，旧 PNG 消费者走明确兼容表示。

**完成标准：**A12；有 GIF 原件时关闭来源后原件摘要/帧数不变；透明 PNG、JPEG/TIFF 色彩/尺寸按支持范围比较；静态回退不能冒称动画成功。

**本机进展：**原始编码与静态封面分离、单帧 APNG、容器结构／预算、原件指纹、加密历史重开与正式列表已验证；最终 18 项门禁通过，helper 图像函数 57 个断言通过。完整 A12 的真实来源／目标、最终 native 发布、Windows、双设备与 CI 仍 UNKNOWN。详见 [O03 验收文档](COMPOSITE-CLIPBOARD-O03-ACCEPTANCE-2026-10-06.md) 和执行记录第 15 节。本轮停止，下一项 O04 未启动。

**本机进展：**O03 首轮实现与聚焦自动验证已完成（见 [O03 验收草稿](COMPOSITE-CLIPBOARD-O03-ACCEPTANCE-2026-10-06.md)）：仅读头的有界图片元数据、原件留存与静态投影分离、显式静态封面、动画原件的独立身份域；Core 665＋Protocol 48＋集成/Runtime/macOS/Windows-host/Clippy/fmt/contracts 等 13 条聚焦门禁 exit 0，源码指纹稳定。真实 GUI/目标应用、Windows 原生、Mac↔Windows/双设备与 hosted CI 仍 UNKNOWN，Release NO-GO。

### O04：来源端资源归档与位置绑定

**措施：**

1. 按已有原生附件、已有图片字节、内嵌资源、允许的公开网络引用的顺序取得资源。不同表示中的图片不得凭“看起来有一张图”推断位置关系；关联必须有可验证依据。
2. 扩展资源层为 bytes＋MIME＋摘要＋长度＋获取来源；保留原始引用和文稿节点关系。按内容去重，不能以 URL 作为内容身份。
3. 初版提供来源端显式“保存图片到本地”动作，或明确开启的归档策略；无默认静默开启新联网能力。实现普通 HTTP(S) 引用，不自动携带浏览器 cookies/凭据。
4. 获取器限制总量/并发/截止，逐次校验重定向和实际连接目标，防止地址绕过访问本机/内网；文件/非 HTTP(S) 引用进入独立授权适配。
5. 获取成功以资源准备＋事务提交更新对应历史的归档版本，保留原 HTML 和原件；删除过的条目不复活，任务取消或版本不匹配则丢弃结果。
6. 来源已含两张字节完整的图片时不额外下载。预览及接收端不自动联网；失败具体到缺哪项资源，原始内容不丢弃。

**落点：**Core 有界 ResourceResolver（拟议）、资源/绑定持久化、历史生命周期、来源平台调用、O06 恢复物化。

**完成标准：**A13/A14；临时 URL 在归档后失效，断网/重启仍能在正确位置恢复双图；相同 URL 改内容与不同 URL 同内容正确处理；下载后新 clipboard revision 不变；删除/取消/满盘不留下可见半提交或孤儿载荷。

**本机进展：LOCAL_COMPLETE。** 显式联网动作、public-only static PNG resolver、精确位置 bindings、内容去重、Manifest 3 加密派生层、完整身份／manifest CAS、取消／重开拒绝、失败清理、列表／生成 report 与两端入口已实现。23 条最终门禁同源码 PASS；文件 DB 重开、实际生产 HTTPS 和私有 native board 2 PASS。只补尚缺资源，0 新资源不增版本，原 HTML／CNA1／内容身份与收藏保持；entity URL、CSS/srcset/SVG/object、其他图片编码与登录态仍 withheld。完整 A13/A14 的真实来源／目标、GUI、Windows 原生、双设备／CI UNKNOWN，Release NO-GO。详见 [O04 报告](COMPOSITE-CLIPBOARD-O04-RESOURCE-ARCHIVE-2026-10-06.md) 和执行记录第 17 节；本轮停止，O05 未启动。

### O05：登录态、blob 与来源上下文增强

**措施：**公开资源归档稳定后，针对有实测需求的来源提供可选适配。浏览器扩展可在用户允许的复制动作中取得当前页面可访问的资源字节及位置关系，以本地受认证通道提交给来源端 TailSync。

只传实际资源和最少 provenance，不同步浏览器 Cookie、token、私有文件路径或会话。blob 地址不发给接收端“自行下载”。无扩展时基础采集仍可用；无权限、来源关闭或字节取不到时保留原件和具体限制。

**落点：**拟议来源适配层、受认证本地 API、O04 资源归档；QQ 专有资源适配须先由 F05 定位，不假设浏览器方案可解决 QQ。

**完成标准：**A15；有/无授权、拒绝/撤销、页面 blob、来源关闭的边界可重复；传输及日志不含凭据；长尾未实测时保持待验证，不承诺任意登录网页。

**本机进展：LOCAL_COMPLETE（可选手动文件桥）。** 显式 grant／ticket／packet、两端 authenticated id＋path API、Chromium MV3 activeTab＋scripting、Manifest 4／可重新验证来源证明、partial→source→public 单调补缺与旧层保全已接线。六项独立 RED 转 GREEN，29 条最终门禁同源码通过；真实文件 DB／CNA1／离线三图位置／fresh helper 私有 board 验证通过。v2/v3／schema／身份／helper／网络版本和 1 MiB API line cap 不变。该文件桥不是浏览器签名或 native messaging；响应丢失的 grant 受 TTL/cap 约束，Windows session close 只验证 late UI 隔离。真实 browser／授权／blob／无扩展／来源关闭、GUI／目标 App、Windows 原生／双设备／CI 全部 UNKNOWN。详见 [O05 报告](COMPOSITE-CLIPBOARD-O05-SOURCE-CONTEXT-2026-10-06.md) 与执行记录第 18 节；本轮停止，不自动进入 O07／O09。

### O06：准备恢复计划，再集中发布

**措施：**扩展现有 PreparedRestore/ClipboardPort，形成有明确输入版本和输出清单的 RestorePlan（拟议）：

- 原生自包含且平台支持：选择原始集合。
- 原始只有失效外链但资源已归档：选择资源补齐的派生集合，原件仍保留。
- 异平台：选择对方支持且实际资源到齐的兼容表示。
- 用户显式选择格式：创建独立转换计划，不改变以后默认整组恢复。

准备阶段完成解码、资源验证、必要转换、临时文件与 lease 后才能进入 publish。不要先清空再下载。Mac 自包含图文输出可增加 RTFD 物化；Windows CF_HTML/带图 RTF 按经过测试的目标实现。不能把所有派生格式无差别混到原生集合。

自动同步不知道未来粘贴目标，使用已验证的通用组合；用户主动在 TailSync 执行针对当前目标的操作时，才采用有证据的目标适配。协议事实只确认保存/写入，不确认实际粘贴。

**落点：**runtime history、Core sync preparation、Mac/Windows ClipboardPort、资源 materializer；复用 journal 和 operation permit。

**本机进展：**O06 已实现 RestorePlan 1 与资源/原件/异平台/显式格式选择，历史与自动接收共享准备门；当前输出为 owned inline bytes。冻结源码、17 项新增回归、26 命令实际结果、全新包私有 board 证明与回退见[执行记录第 11 节](COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md#11-o06准备恢复计划再集中发布)。A09/A13/A17 的真实应用/公开 URL/设备验收未完成，O04 文件 materializer/lease 仍后续。

**审查整改：**移除无消费者的计划身份/版本/输入摘要与失效发布包装层；输出诊断清单按需生成。HTML 共用词法 inventory 并逐节点检查图片来源；菜单与准备共用便携格式规则，能力查询解析一次，列表复用已解密 manifest。生产 feature 门禁、取消与源字节保活及跨层回归结果单独记录于[执行记录第 12 节](COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md#12-审查整改)。O06 原包和证据保持历史记录，本次源码验证不替代新包、GUI 或真实设备验收。

**完成标准：**A09/A13/A17；准备失败无 native write；每次成功恢复一次整组提交；临时文件在当前剪贴板仍依赖时不能被 GC；缺资源、Partial、Unknown 不显示整组成功。

### O07：同平台原生同步与细分能力

**措施：**保留现有复合协议，增加明确版本化的原生归档能力。传输来源平台、item 边界、稳定类型名称、字节引用、必要资源清单与摘要。CNS1 是本地 IPC，不能直接未经设计作为可信网络载荷复用。

发送端根据双方认证后的能力选择原生/便携表示；Mac↔Mac 可以保留适合回放的原始格式，Mac↔Windows 使用已支持的兼容表示。无法回放的原生字节若仅归档，必须区分“保存”与“可恢复”，不让接收端执行不透明对象。

新消息值和版本由活动注册表分配；旧端保留合法单表示兼容策略，源端完整历史不因降级被删。不能塞进旧 TextPayload。

**落点：**protocol、secure 能力、delivery/receiver、db native parts 和 platform restore；契约/格式规则同步两端。

**完成标准：**A18/A19；两 Mac 原生保真、重试不重写；异平台无非法私有格式发布；旧端不能把包当文字；撤销信任后无副作用。单 Mac 只能验证 codec/模拟两进程，真实双设备保持待验证。

### O08：资源块传输、完整提交与引用生命周期

**措施：**先保留现有 bounded CBP1 分块路径作为小内容路径；在实测大附件需要时增加“内容清单→缺失 blob→校验→整条提交”。复用文件传输的字节块能力，不创建每张图片一条文件历史或独立自动写入事件。

blob 以摘要/长度/编码身份校验，跨事件复用和续传需要受信任会话与本地授权边界。半成品不可作为完成历史/发布成功；全包持久提交后才确认保存，再经 generation/授权检查决定是否写剪贴板。

引用计数/lease 覆盖历史、收藏、pending 接收、下载、预览、恢复和当前剪贴板依赖。最后一个有效引用消失后才清理；删除与迟到任务并发时不能复活条目或误删共享资源。回执丢失补 ACK，不重新执行 OS 写入。

**落点：**delivery/receiver、分块/文件块接缝、资源存储、GC/trim/favorites 和 effect journal；不直接重写现有文件产品语义。

**完成标准：**A14/A18/A20；缺块/错误摘要/断线后恢复正确；多条历史共享资源删除一条不破坏另一条；收藏/当前剪贴板资源不被配额清理；最后释放后无泄漏。

### O09：Windows 常见格式、OLE 与跨平台目标适配

**措施：**已有 Text/CF_HTML/RTF/PNG/DIB/文件路径维持并补原生实测。注册格式以名称持久化与重新注册，不能同步机器局部数字 ID。CF_HTML offset 继续按 UTF-8 字节计算，保留上下文。

普通可持久内存数据与 IDataObject/OLE 格式分开；虚拟文件可能来自流或其他 STGMEDIUM，需受预算的显式适配，不能盲目复制句柄。先以常见五类内容和附件做深，复杂 Office 对象的编辑语义单列，不承诺 HTML 表格等于原公式/对象。

**落点：**Windows helper、clipboard_composite、Core 类型映射、CF_HTML/bitmap、文件接收与 O06。Mac 上的 MSVC host check 不替代原生构建和运行。

**完成标准：**A19；Windows portable/安装包自检及原生应用粘贴、双向图文/中文 emoji/透明图片/表格链接；目标版本与结果逐项记录。缺 Windows 时不伪造 PASS。

### O10：因果、防迟到与提示语义

**措施：**保留 revision-before/after、origin-before-dedupe、claim-before-I/O、NoEffect/Published/Partial/Unknown/Superseded 等现有规则。新采集、资源获取、转换、预览都绑定 capture/event ID 及对应内容版本。

新复制出现时不把旧格式拼进去；下载完成只更新相应历史；准备/发布前再次检查用户 intent、授权与 generation。超时不能以“没有回执”推断“没写过”，只有证明无副作用才自动重试。

结果内部区分：保存成功、本机可恢复、跨平台可转换、资源已归档、存储失败、真正读取失败及未知写入。它们是事实字段，不要求新增常驻状态标签。原生本机可恢复只因便携转换不支持时，不发 capture_failed 通知。

提示可执行性逐条检查：旧 clipboard 已被替换时，“重新读取旧剪贴板”不能成为假修复；未知结果手动再次恢复是新 intent；已删除/变更内容不能绑定到其他记录。通知可见性与 ack 继续遵守现有规则。

**落点：**capture_retry、clipboard monitor、origin/supervisor、sync generation、effect journal、sync_warning、HistoryRestoreRetryIdentity、Swift/React feedback。

**完成标准：**A02/A16/A17；快速连续复制、可选 provider stall、写后丢回执、补图迟到及撤销信任，均不串内容/不自动覆盖新复制；真失败仍能被用户发现。

**本机进展：**O10 的采集/重试、准备前准入及提示归因修正已完成，18 条最终检查通过，见[执行记录第 13 节](COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md#13-o10因果防迟到与提示语义)。原生保存成功不再因便携转换不支持而报 capture_failed；旧 revision 的结果不绑定到新复制，旧原生快照接口不能只存摘要就宣称保存成功。A02/A16/A17 的本轮证据限定生产接缝自动回归和 UI 单元测试；完整 GUI/系统通知、真实慢 provider、Windows 原生/双设备仍 UNKNOWN。O04 的实际下载与载荷补齐仍未实施，现有版本 CAS 只证明已实现的历史摘要更新。

### O11：身份、完整集合去重、schema 与回退

实施细化：[ADR-005](adr/ADR-005-composite-content-identity.md)；O11-D1 设计及规则模型完成；O11-D2 存储、去重与重试已接线，本项自动验证完成并暂停。旧 protocol=2 投影不给安全原生去重键；O01 的 macOS CNA1 已提供 Exact 角色，完整读取与闭合依赖另行检查。资源提交与应用/设备验收仍随 O04/后续阶段完成。

**措施：**

1. 区分来源事件身份、原始内容摘要、归档/派生版本摘要。原始内容摘要包含平台、项目顺序、类型名称及原始字节；排除 TailSync 自写标记、系统 revision 等非内容字段。无原生内容时使用带明确域/版本的便携摘要。
2. 目前非 native-only 以 PortableBundle 摘要去重，可能合并便携相同但原生不同的条目。新去重须保留不同原生集合，不能仅以文字、便携 HTML 或可见缩略图判相同。来源分类指纹、协议摘要和历史去重摘要各自定义，不能混用。
3. 原事件/capture 身份保持稳定；资源补齐更新归档版本，不新建一次复制、不重写系统 clipboard、不复活删除 tombstone。预览缓存和 RestorePlan 绑定版本，防止旧结果替换新内容。
4. 不就地更改旧 data_hash 含义来破坏重试身份。新摘要显式带版本；旧历史保留原摘要及旧读取路径，重试对原件身份和当前存在性核验。
5. 审核当前 v12 schema、Manifest version=1、CNS1/helper 协议 2、CBP1 和本地契约的严格解析点。需要新结构时分配 vNext schema/manifest/IPC/网络版本，具体号码在实施审计后确定。不能只手改 generated Swift/TS。
6. 新字段覆盖配额、收藏、删除、共享资源、degraded 恢复和迁移；预算既计原始与派生持久数据，也计峰值准备内存。历史原来漏掉的字节不能凭迁移“补回来”，标明 provenance。
7. 升级前保存数据库一致快照及所有必要外置 payload/清单；回退保全升级后数据，再到新目录恢复旧快照。旧二进制不得直接打开新 schema。关闭功能仍可读取已升级历史，不自动降库。

**落点：**db/composite/entries/migrations/schema/lifecycle/storage、runtime retry 身份、protocol codec、契约生成及包自检。

**完成标准：**A09/A14/A21；便携相同原生不同的两条历史不误合并；仅派生补齐不改变原事件；旧记录读/恢复/收藏完整；迁移断电/满盘保留原件；真实旧 reader 与新 reader 分别验证回退目录。旧截图/旧 helper PASS 不能替代新版本验证。

### O12：来源—目标、持久化与性能验收

**措施：**为每个实际来源建立成对样本：原生直接粘贴与历史恢复。保存类型/项目清单、关键字节摘要和目标实际表现；排除自身 token 后比较，不只看截图。应用版本、实际包/源码指纹、配置及网络条件必须可追溯。

源/目标优先覆盖 QQ、TextEdit、Preview、浏览器；Word/微信按可用环境追加。关闭来源、重启/断网/跨设备分层记录。敏感正文不写公共日志；合成样本用固定内容，真实用户内容仅在明确允许的范围取证。

性能比较当前优化构建与新候选：空闲、连续复制、单图/双图、未知类型、预览切换、恢复和资源任务。记录 CPU/RSS、p50/p95、native/转换次数、缓存命中、wire bytes 和资源释放。保留现有显式 256 MiB pipeline budget 与并发限制；超过预设回归阈值需解释或修正，不能直接放宽预算。

**拟定门槛：**同机重复测量后，普通路径 p95 与空闲 CPU 不出现无解释的超过 10% 回退；长序列 RSS 无持续线性增长，最后 owner/lease 释放恢复预算；没有必要时不重复生成已有兼容表示。10% 是候选工程门槛，先建立有噪声范围的基线，不是假称已有测量。

**完成标准：**本节矩阵及每项 Exit Gate 有真实结果；本地自动、完整 GUI、来源/目标、Windows、双设备分别登记；缺条件标记待验证。当前发布 NO-GO 不由文档或本地全绿改变。

## 4. 验收矩阵

“自动”表示可在隔离数据/私有 board 中编程验证；“应用”表示必须操作真实来源或目标，不由合成样本替代。下表登记完整验收目标；S0/O11 的接缝复现与回归在执行记录单列，不直接将整条验收目标标为通过。

| 用例 | 输入与检查 | 覆盖项目 | 条件 |
|---|---|---|---|
| A01 | 截图/Preview/浏览器单图；类别、图像尺寸/透明度和恢复 | F01、O03 | 自动＋真实应用 |
| A02 | 成功采集/原生保存不误报警；真失败仍提示；提示发出方归因 | F01、F03、O10 | 自动＋GUI/系统通知 |
| A03 | 普通行无复制文字/空菜单；完整恢复、纯文本粘贴、异常再复制 | F02、O06 | UI 自动＋目标应用 |
| A04 | 元数据减负，中英/深浅色/AX/键盘；完整性设计单独审阅 | F03 | UI 自动＋视觉/辅助验证 |
| A05 | 中文 emoji、粗体颜色、双图和基本表格富文本预览 | F04、O01、O06 | 自动＋GUI |
| A06 | 预览打开/关闭/快速换项无 clipboard 写、无外部请求，取消释放 | F04、O10、O12 | 自动＋GUI |
| A07 | QQ 单图、文字＋双图、选区与复制动作；直接/历史粘贴成对 | F05、O02、O04 | QQ＋目标应用 |
| A08 | 首图/第二图颜色不同，独立 PNG 干扰、去重与存储顺序变化 | F06、O04 | 自动＋GUI |
| A09 | 来源 RTFD、HTML、text 不同；原字节保留，派生不污染恢复 | O01、O06、O11 | 自动＋TextEdit 等 |
| A10 | 自定义字节类型、多个原生项目及混合文件/图文，重启后名称/顺序/摘要和依赖一致 | O02、O07 | 自动；实际应用另测 |
| A11 | 禁止记录标记、未知格式慢 provider、超预算；无正文泄漏 | O02、O10 | 自动＋受控原生 provider |
| A12 | GIF 原件、透明 PNG、JPEG/TIFF；原件和静态兼容分别核验 | O03 | 自动＋支持该格式的目标 |
| A13 | 公网测试夹具 URL 归档后失效，双图位置；断网/关闭来源恢复 | O04、O06 | 自动＋实际目标 |
| A14 | 下载/接收中删除、收藏、满盘、迁移、GC，共享资源保全 | O04、O08、O11 | 自动＋隔离存储 |
| A15 | 登录/blob 来源：授权、拒绝、撤销、无扩展、页面关闭 | O05 | 来源适配环境＋人工授权 |
| A16 | 新复制打断旧采集/补图；版本变更时无混合快照或迟到写入 | O02、O10 | 自动＋受控原生竞态 |
| A17 | 写后回执丢失/Partial/Unknown；无自动重写；显式恢复新 intent | O06、O10 | 自动故障包＋GUI |
| A18 | 同平台原生同步、缺块/错摘要/断线续传、撤销、回执丢失 | O07、O08 | 自动模型＋两台真实设备 |
| A19 | Windows CF_HTML 字节偏移、DIB、RTF、OLE/文件及旧端协商 | O07、O09 | host 检查＋Windows/双设备 |
| A20 | 真实传输/预览/转换的编码次数、峰值内存、缓存和最终释放 | O08、O12 | 自动优化构建；设备网络另测 |
| A21 | 便携相同原生不同、补齐版本、旧历史与 vNext 完整回退 | O11 | 自动＋实际旧 reader/隔离包 |

### 4.1 单 Mac 当前可推进与条件

| 范围 | 可推进内容 | 不能由此认定 |
|---|---|---|
| 单 Mac 自动 | 编解码、原件/派生隔离、去重、类型保留、资源绑定、受控 URL、DB/GC/回退、私有 board 竞态、UI 规则 | QQ/Word 等真实来源—目标成功 |
| 单 Mac GUI/真实应用 | TextEdit/Preview/可用浏览器、实际 QQ、预览/首图/菜单、直接与历史粘贴对照 | Windows、双设备或任意应用兼容 |
| 需要用户/系统条件 | 来源授权、登录/blob 扩展、通知显示、VoiceOver/IME、敏感样本许可 | 自动测试可替代实际授权/体验 |
| 需要更多设备 | Windows 原生、Mac↔Mac/Mac↔Windows、旧端与真实网络 | MSVC host check 或本机双进程等于设备验收 |

## 5. 实施顺序与阶段 Exit Gate

### S0：基线与问题复现

固定 candidate 指纹和本文 21 项拟议用例的执行条件；优先 F01/F05 的实际包/来源/目标记录。复用原 R1—R7 证据索引，但不继承新源码或新包的 PASS。

**退出门槛：**能区分分类、预览、采集、资源、写入和目标选择；F01 提示发出方清楚，F05 有源快照与直接粘贴基线，或者条件明确标为待验证。

### S1：本机原样保留和恢复

先设计 O11 的身份/版本/迁移，再实施 O01、O02、O06 的最小闭环和 O10 语义修正；优先 QQ 真实样本涉及的类型。O03 原件保存可随类型扩展进入。

**退出门槛：**A09/A10/A11/A16/A17/A21；源字节不被派生覆盖，无误合并、无迟到写入。真实应用条件缺失的项保留待验证，不阻止其余隔离自动工作。

### S2：用户可见体验

F02/F03 的确定部分可独立提前；完成 F01 分类、F04 文稿预览、F06 首图及 F05 相应展示修复。完整性 UI 保留意见独立呈现供审阅。

**退出门槛：**A01—A08；实际图文可辨认和恢复，界面精简不隐藏真失败，预览无写剪贴板/网络副作用。

### S3：持久资源和公开引用归档

实施 O04，接入 O06 的自包含物化、O08 的生命周期要求及 O11 的归档版本。先公开/受控引用，再对 O05 长尾建立可选适配。

**退出门槛：**A12—A15/A21；关闭来源、URL 失效、断网和重启后已归档内容仍恢复；晚到结果只更新合法历史，不覆盖新复制。没有权限的长尾不计成功。

### S4：原生/跨平台同步和传输优化

实施 O07/O09，再根据实测瓶颈扩大 O08 缺块能力；避免一次引入所有私有对象或插件框架。

**退出门槛：**A18/A19/A20；实际设备/应用矩阵逐项通过，旧端和新版协商清楚，完整内容持久提交与系统发布各有证据。设备不足保持待验证。

### S5：封存与发布判定

按最终源码运行必要本地矩阵并封存包、日志、配置和应用观察；更新本地/原生/目标/双设备边界。文档完成、源码完成、本地完成、设备通过和发布许可分开记录。

**退出门槛：**无未解释回归；实际要求的 R7 条件齐备才讨论候选验收。不得用这份新方案直接把现有 NO-GO 改成 GO。

## 6. 既有接缝和验证入口

### 6.1 优先沿用的文件

| 范围 | 现有入口 |
|---|---|
| Mac 快照/恢复与转换 | [clipboard-helper.swift](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/src-tauri/clipboard-helper.swift)、[clipboard_composite.rs](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/src-tauri/src/clipboard_composite.rs) |
| Windows 快照/恢复与转换 | [helper 入口](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/tools/clipboard-helper/src/main.rs)、[clipboard_composite.rs](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/windows/src-tauri/src/clipboard_composite.rs) |
| 稳定采集与告警 | [clipboard.rs](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/src-tauri/src/clipboard.rs)、[Core composite](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/composite/mod.rs) |
| 原生/便携协议 | [native_clipboard.rs](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/tailsync-protocol/src/native_clipboard.rs)、[composite.rs](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/tailsync-protocol/src/composite.rs) |
| 历史与预览 | [db/composite.rs](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/db/composite.rs)、[db/preview.rs](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/db/preview.rs)、[runtime history](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/tailsync-runtime/src/history.rs) |
| 提交与回执 | [ClipboardPort](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/sync/port.rs)、[receiver](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/peer/composite_receiver.rs)、[delivery](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/rust-core/src/peer/delivery.rs) |
| UI/契约 | [HistoryRow](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/swift-ui/Sources/TailSync/Views/HistoryRow.swift)、[HistoryView](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/macos/swift-ui/Sources/TailSync/Views/HistoryView.swift)、[契约生成](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/shared/schema/generate-local-contracts.mjs) |
| 当前约束 | [ADR-004](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/adr/ADR-004-composite-clipboard-contracts.md)、[现有修复执行方案](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/docs/COMPOSITE-CLIPBOARD-REPAIR-EXECUTION-PLAN-2026-10-04.md) |

### 6.2 后续验证要求

- 使用现有 run-isolated-tests.mjs 的数据隔离；不改系统 HOME，不污染日常配置。
- 改 core/model/receiver 时，运行对应生产接缝回归和实际故障复现；DTO 改动运行契约生成/check，Swift/React 同步覆盖。
- 改 helper/协议/manifest 后，实际新包 self-check、生产 resolver、签名/架构/最低系统与新源码指纹重新绑定。
- 最终范围变更后按 run-composite-r6.py 当前完整矩阵生成新 run；不能过滤必要测试、伪造计数或改写旧封存 run。
- GUI 会话核对 TCP 19890 和精确候选；正常退出已有 App，使用隔离数据与短 socket，并保护日常日志。剪贴板取证必须在授权范围内，私有 board 不替代 general board 与真实目标验收。
- 每项提交证据包含问题编号、源/目标/版本、源码/包指纹、原始结果和边界。失败日志和被推翻的假设保留。

初稿交付时，所有 F/O 项目均处于计划或待复现阶段；后续状态以逐项执行记录为准。现有已实现基础不重列为“未完成重写”，真实设备缺口也不伪装为完成。
