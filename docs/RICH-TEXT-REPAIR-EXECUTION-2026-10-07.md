# 富文本收缩方案：修复设计与执行记录

日期：2026-10-07；最终复核：2026-10-09。用户目标：按照[范围方案](RICH-TEXT-PRODUCT-SCOPE-PROPOSAL-2026-10-07.md)开始设计修复。

工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，HEAD `f719d58`，保留既有未提交工作。当前候选是工作树源码，不是已安装 App 的验收结果。

## 执行清单

2026-10-09 用户进一步确认以可用图文恢复为目标、保持现有预览不变。新增执行记录见 [图文恢复收敛](RICH-TEXT-COMPATIBLE-RESTORE-2026-10-09.md)。以下 R1–R4 历史本地门禁仍绑定各自封存源码，不能证明本轮改动后的目标应用或发布验收。

| 工作 | 必须交付的行为 | 状态 |
| --- | --- | --- |
| R1-A 提示归因与简化 | 装饰简化不报失败；真实缺图/截断明确提醒；原因去重并折叠 | 源码、二次修复和本地自动回归完成；真实 GUI/辅助功能待验 |
| R1-B 可用内容恢复 | 缺图可复制已保存部分；原件优先；结果准确说明缺失；原始历史不变 | 源码与生产接缝自动回归完成；真实目标应用粘贴待验 |
| R2-A 自动保存设置 | 两端持久化默认关闭；仅新本机复制；关闭取消；旧条目手动 | 源码、共享有界队列、两端生产接线、预览刷新及本地自动回归完成；真实 GUI 待验 |
| R2-B 常见公开图片 | PNG/JPEG/静态 WebP；有界下载、解码、配额、预览刷新；原始网络字节独立持久化 | 源码、原图/PNG 同事务保存、重启/失败/配额/清理及自动/手动生产接缝本地验证完成；真实网络、GUI 与设备验收待验 |
| R2-C 下载资源同步 | 补全结果能传播到可信接收端；删除/迟到/重试安全 | 协议/DB/自动与手动接线、父代理二修与最终本地门禁完成；真实网络/设备仍待验 |
| R3 阅读语义 | 基础 CSS 强调、颜色/高亮、标题、链接、引用、代码、列表、简单表格 | 双 DeepSeek/high 交付、父代理独立二修与 20 项同指纹最终本地门禁完成；真实 GUI/应用/设备待验，见 R3 验收 |
| R4 真实场景 | 同目标直接/历史成对对照；来源关闭、重启、离线；跨平台另验 | 准备 PREPARED：工具/样本/候选静态包及 22 项当前源码本地门禁完成；真实 GUI/应用/网络/设备 UNKNOWN，见 R4 计划 |

范围内的后续任务不因为第一项局部通过而自动标为完成。

## R1-A：提示规则

保留底层原因，展示层按用户影响聚合。字体、字号和普通装饰简化为信息，不进入警告横幅；缺图、截断、正文/内容结构缺失、像素预算和有效来源警告继续可见。默认一处短汇总；诊断原因与英文细节折叠。不能把 `styles_partial` 推导为 `native_styles_unavailable`。

主代理负责独立复核两端规则一致性、无损保留诊断、可访问性和资源释放。子代理限定写 UI/本地化/相关测试；Core、契约、设置和恢复接口由主代理负责。

## R1-B：恢复规则

### 原件与兼容副本

原件满足目标准入及资源闭合时继续优先原样恢复。对于可解析 HTML 中的缺图，只在已验证的保存内容上构造新的兼容副本：保留正文顺序、已有基础格式和可用资源，在缺图位置保留说明，清除未闭合的图片加载依赖，再执行现有严格准入及依赖校验。

原始 bundle、native archive、身份与完整性字段不变。完整副本的闭合仅陈述本次选定输出，不等于原记录完整。缺失/损坏的原件、无法验证的格式、超预算、native-only 摘要不能通过此路径冒充恢复。

### 结果与发布

增加可用内容恢复的成功报告，说明缺图数量。它与 helper 的 `Partial/Unknown` 区分：前者表示准备时选定的有限内容全部发布成功，后者表示系统写入不完整或结果未知。后者仍走现有失败及禁止自动重写规则。

恢复计划仍在唯一原生发布之前确定；保持 generation/revision 检查、身份校验、取消和原生发布串行。下载完成不重写当前剪贴板。两端普通历史恢复入口消费同一结果，不用假错误表达一次已完成的复制。

### 回归范围

- 已保存正文、粗体和一张图，加一张未保存图：默认恢复可用内容，图片位置正确，原始载荷不变，报告准确。
- 完整 HTML/RTF 和原生快照继续优先原路径，字体等原格式不从简化预览重建。
- 空正文且无可用图片、损坏资源、未知原件缺失、无可信 native-only 正文：拒绝或明确限制，不能输出一条错误提示就算复制成功。
- 评论/脚本中的伪图片、重复属性、畸形 HTML、未知依赖和超预算：闭合检查仍有效。
- `Partial/Unknown/Superseded` 不变成成功；准备期间新复制、删除或身份变化不产生迟到写入。
- 两端正常成功、可用内容成功、真正失败的用户反馈分别验证。

## R2 与 R3 的接线约束

设置默认关闭并兼容旧设置文件；通过生产 DTO 生成两端契约，禁止只手改 generated 文件。后台下载队列属于共享运行时，平台只提供本机采集事件和 I/O；远端接收事件不触发自动联网。复用当前 URL、资源预算与 manifest CAS，保存取消/删除结果。公开资源和原始剪贴板图像编码分别验证。

阅读模型的扩展按需添加有默认值的字段，再生成消费者契约。保留现有消费者版本/旧文稿读取兼容规则。HTML 和原生格式只映射明确可证明的语义，不构造完整 CSS/Word 引擎。链接默认不请求网络，仅显式用户点击允许地址才打开。

## 验证与交付约束

所有测试使用 `node scripts/run-isolated-tests.mjs --` 隔离配置与数据。Rust 统一使用本轮 `target/rich-text-r1`；Swift 使用 `/private/tmp/tailsync-rich-text-r1-swift-20261007`。不读取用户 DB/系统剪贴板、不停止日常 App，不覆盖旧候选包及已封存证据。构建缓存与历史资源分别处理。

分别记录源码、自动回归、生产接缝、包、GUI、Windows 原生和双设备证据；缺设备时保持未验证。范围文档中 R4 的真实效果不能由单元测试替代。Release 判定保持既有未通过状态，完成本轮修复不等于完整发布验收。

## 本轮派发

子代理 ID：`01a1167b-11f0-7c13-99ef-a329edafc910`，模型 `cline-pass/deepseek-v4.1-flash`，reasoning `high`，完整明文任务，关闭历史继承。已确认任务 ID `TAILSYNC_RICH_TEXT_R1_NOTICE_20261007_HIGH`、唯一绝对工作目录和限定写集。父代理并行推进 R1-B；交付后独立审查和二次修复。

### 最新调度：仅复用一名子代理

用户反馈两路负载仍不足，进一步调整为仅复用一名 DeepSeek/high 子代理。主代理在子代理执行期间等待，不同时推进其他实现或测试；子代理完全交付后，主代理才独立审查、二次修复及验证。2026-10-07 已请求关闭并收到两个 R2 子代理的 shutdown 通知；它们不再恢复，既有源码修改不回滚。两个旧的 pending-init 子代理也已请求关闭并核实不再存在，以免后续意外启动。

执行顺序：

1. 只保留 R1-A 提示子代理 `01a1167b-11f0-7c13-99ef-a329edafc910`，等其完整交付；主代理随后复核 R1-A 与已实施的 R1-B，二次修改并运行专项回归。
2. 向同一名子代理发送 R2-B 图片的完整明文任务，确认任务、工作目录、写集、此前修改和剩余工作；交付后主代理复核及二修。
3. 向同一名子代理发送 R2-A 设置与队列的完整明文任务，确认回执；交付后主代理复核及二修。
4. 再按同样的串行流程推进资源同步、阅读语义和真实场景验收；不新开或恢复其他子代理。

主代理复核不等于子代理交付已验收；关闭或排队也不等于任务完成。测试与构建共享本轮缓存并顺序执行，避免并行 Rust 构建造成额外磁盘和内存压力。

### 用户要求回退后的当前状态

2026-10-07 用户要求回退本轮混乱的子代理修改。本节替代前述继续派发安排；本次仅执行回退与验证，不恢复或另开子代理。

三个本轮子代理均已关闭并核实句柄不存在。按其实际执行记录确认，R2 图片与设置子代理没有修改工作树源码；R1 提示子代理实际修改 9 个文件。对 R1 的成功工作树补丁进行精确反向撤销，涉及 Swift 文稿 material/view/测试/本地化，以及 Windows 文稿 renderer/测试/本地化/CSS。没有使用 HEAD 整体覆盖、reset 或 clean。

保留主代理已经实施的可用内容恢复、恢复报告/接口/契约、来源归因和基础 CSS 解析。仅额外保留主代理恢复报告入口依赖的 `history.restore.availableContent` 中英文文案，避免回退提示方案后将主代理已有功能的反馈变为原始 key。

回退前备份、补丁来源、反向补丁和哈希核验记录位于 `artifacts/rich-text-repair/2026-10-07-subagent-rollback/`。9 个文件与逐补丁反演所得预期内容逐字节一致；反演后重新应用原补丁能复原回退前内容。首次哈希核验确认其余 1175 个源码文件、Git HEAD 和暂存区不变。本执行记录随后单独更新，不属于子代理源码回退。

已安装 App、旧测试包、封存证据、用户数据和系统剪贴板不受本次回退影响。回退不等于富文本修复目标已完成，R1-A 与 R2 的交付仍未完成。

回退后隔离验证：Windows 文稿 renderer/review/页面三组共 18 项通过；Swift `HistoryPreviewDocumentTests` 18 项通过；`git diff --check` 通过。验证后再次核实还原文件哈希及暂存区不变，除本执行记录外的其他源码未发生变化。这些专项结果不代表整包、真实 GUI、Windows 原生或双设备验收。

### 用户重新授权：两个子代理继续

用户随后明确要求使用两个子代理继续。本节替代此前停止派发与单代理调度；旧子代理不恢复。从回退后的当前源码重新保存基线，证据目录 `artifacts/rich-text-repair/2026-10-07-two-agents-r2/`，记录 1184 个源码文件哈希、两套互斥写集及 Git 暂存区快照。

- 提示子代理 `01a116dc-da6a-7b53-9a02-b590c042e8b7`（Boyle），任务 `TAILSYNC_RICH_TEXT_R2_NOTICE_20261007_HIGH`：仅两端文稿提示、相关本地化/CSS和回归九个文件。
- 图片子代理 `01a116dc-dac8-7a73-ac53-e407af2d74ef`（Wegener），任务 `TAILSYNC_RICH_TEXT_R2_IMAGES_20261007_HIGH`：仅公开图片归档、独立有界解码模块、模块注册、Core 依赖及三个锁文件；锁文件只有该子代理写入。

均使用内置 Multi-Agent、`cline-pass/deepseek-v4.1-flash`/`high`、`fork_context: false`，已核实 CPA 兼容开关开启。两个子代理先只回执任务和绝对目录，经主代理核实后收到 START 开始实施；没有第三名子代理。原始派发明文保存在 `delegation.json`。

主代理先复核已保留的可用内容恢复接缝，仅修改 HTML 兼容恢复、Runtime 相关回归及本执行记录，不与两套写集重叠。待子代理完整交付后，主代理逐文件独立审查、补边界回归和二次修复，再执行真实工作树门禁。禁止通过临时源码副本或手改生成 DTO 掩盖问题，不将局部交付当作自动设置、资源补全同步、阅读语义或真实目标应用验收已经完成。

## 本轮交付与主代理独立二修

截至本次交付，完成的是 R1 的源码/自动回归与 R2-B 的格式归档子项，不是整个富文本方案。首轮两个子代理均已交付并关闭；主代理未把其在途全量测试当作最终验收。

独立审查后，又派发两套互斥的边界二修任务，仍为内置 DeepSeek/high，完整明文、无历史继承、回执后 START：

- 提示：Lorentz，`01a116fb-3b5b-75f2-82f0-c326cb994233`，限定原九个 UI/本地化/测试文件，实际修改其中七个。
- 图片解码：Franklin，`01a116fb-3bbd-7872-b3a1-62243597d302`，唯一源码写集 `shared/rust-core/src/composite/network_image.rs`。

两个新代理的运行上下文均核实为 `cline-pass/deepseek-v4.1-flash`/`high`，已完整交付并关闭。恢复旧句柄的回执阶段曾发现模型变为 `gpt-6.1-sol`/`xhigh`；在 START 前关闭，未产生实质读写，再显式指定 DeepSeek/high 重派。记录见 `delegation-review-v2.json`。图片代理曾用 Python 编辑临时测试，违反编辑约束；已提醒，临时测试与调试输出已清除，后续改动使用 apply_patch。这些调试试验不计为正式通过证据。

### 已修正的实际问题

1. **普通装饰与真实损失混淆。** Core 的字号/行距/不支持实色等普通装饰只标记 `styles_partial`；实体、嵌套表格、活动标记等 `unsupported_markup` 仍明确警告。两端不按英文 detail 或 `styles_partial` 猜测结构是否丢失，也不再推导虚假的原生样式缺失。普通简化放入折叠说明，真实问题最多一条合并提示。
2. **运行时缺图没有进入汇总，或正常图片也误报。** 两端按实际解码/图片计划判断，正常图片不警告；真正缺资源显示“图片未保存”，已保存但解码或预算受限显示“图片无法显示”。Windows 新条目/新 stamp 不继承旧 blob URL 的失败状态。
3. **浏览器上下文阻止可用内容恢复。** 真实 base/CF_HTML SourceURL 经过验证；恢复兼容副本去除 base 并补齐相对链接，更新已保存图片绑定偏移。无值 href 保持原样，不插入虚构地址。重复属性、活动依赖和不安全上下文继续拒绝。完整原件的字体和格式继续优先，不从统一预览重建。
4. **下载图片的安全/预算边界不完整。** PNG 在解码前检查 caller 的像素/RGBA 限额；扫描不再在 IDAT/VP8L 处提前结束而漏掉动画标志；WebP 的 RIFF 长度、填充与截断受校验；JPEG/WebP 应用 EXIF 方向。派生 PNG 用流式编码与有界、封顶几何扩容的 writer，显式 finish，避免先生成整幅压缩结果或返回缺 IEND 的“成功”图片。
5. **累计像素预算误把 RGBA 字节当像素扣除。** 已修正单位，并在解码前应用剩余累计预算。解码和绑定处理进入共享有界 blocking worker，取消/截止能够返回；仍运行的 worker 持有准入，不因等待者取消提前释放。
6. **常见网络图片地址被不必要地拒绝。** 公开图片的相对 URL 按已验证的 base 解析，查询参数中的 `&amp;` 等支持的实体规范化；归档提交验证使用同一规则。URL/DNS pin、公网地址、无凭据、重定向和字节限制不放宽。私网、控制字符、反斜线、重复/无效 base 与空 src 不产生请求。
7. **底层正文告警在文稿生成时被丢弃。** RTF 缺少原生正文分析时，分析层存在原因，但原先的文本文稿路径忽略全部分析原因。现保留这些原因至最终文稿；底层与 UI 均按 `(code, detail)` 去重，不吞掉同类的不同诊断。新增真实分析入口回归证明 RTF-only 空正文不会悄悄消失告警。本次没有新增通用 RTF 解析器。

主要真实 RED：主代理的 `parent-pixels-red.log`、`parent-anchor-red.log`、`parent-image-context-red.log`、`parent-body-warning-red-02.log`；图片代理的 `images-review-v2/red-before-fix.log`（8 项行为失败）；提示代理的 `notice-review-v2` RED 日志。首轮图片代理及主代理正文告警首个日志的编译失败不算语义复现。

### 冻结源码的主代理门禁

全部来自真实工作树，隔离数据与配置。Rust 最终全量串行，共用 `target/rich-text-r1`；Swift 共用既有 scratch。没有复制源码或手改生成 DTO 来绕过检查。

| 门禁 | 最终结果 | 主代理日志 |
| --- | --- | --- |
| Core 全量 lib | 778 PASS，4 ignored | `parent-core-final-04.log` |
| Runtime 全量与集成 | 90 PASS，其中恢复计划 11 项 | `parent-runtime-final-04.log` |
| Windows UI 全量 | 62 文件，387 PASS | `parent-windows-test-final-02.log` |
| Swift 整包测试 | 322 PASS，4 skipped（总计 326） | `parent-swift-test-final-02.log` |
| Core/Runtime clippy | `--all-targets -D warnings` PASS | `parent-clippy-final-04.log` |
| 生产契约检查 | PASS，未手改生成文件 | `parent-contracts-final-05.log` |
| Windows 前端构建 | tsc + Vite PASS，真实 dist | `parent-windows-build-final-01.log` |
| macOS adapter 编译 | 启用 composite feature，包含 tests，PASS | `parent-macos-check-final-04.log` |
| Windows adapter 宿主编译 | 同上，PASS；不是 Windows 原生验收 | `parent-windows-host-check-final-04.log` |
| 本轮 Windows 文稿 lint | deny-warnings PASS | `parent-windows-scoped-lint-final-01.log` |
| 全量 Windows lint | exit 0，22 条未修改模块警告 | `parent-windows-lint-final-01.log` |
| 本轮七个 Rust 文件格式 / diff | PASS | `parent-format-final-v3.log` / 最终保护核验 |

Core 的 ignored 含三个性能基线和一个由父测试调用的 subprocess worker，不算本次逐项执行。Swift 的四个跳过项要求真实 key-window/WKWebView 测试宿主，不能把整包零失败当作这些原生门禁已通过。Windows 宿主编译还有 cfg/dead-code 警告；这些文件本轮未改，不以编译 exit 0 冒充零警告。

Windows adapter 初次编译因之前清理后不存在 `windows/dist` 失败；随后对当前源码进行真实前端构建，再通过宿主编译。未创建占位 dist、修改 Tauri 配置或覆盖旧 App 包。中间失败与旧源码日志保留，不替代最终日志。

证据根：`artifacts/rich-text-repair/2026-10-07-two-agents-r2/`。最终 839 个产品/测试/工具/配置/锁文件指纹为 `3e09a059e661c630f333b0dee3ffabc2821326777075e272c2e9360b784c8ad6`，见 `parent-source-final-v3.json`。1184 个基线文件逐项保护核验及暂存区核验见 `parent-validation-final-v2.json`，不是把 1184 个文件都称作新增源码。基线快照统一排除了 artifacts；其中现有的 R0 规范失败清单另与 O03 历史指纹核验，确认没有新增或改写该清单。

### 未完成的内容与下一项

- R2-A：源码、本地自动回归和生产接线已完成；真实 GUI、实际帖子复制和正在下载时关闭开关仍待实机验收。
- R2-B：网络原始 JPEG/WebP 字节尚未单独持久化，目前保存内部静态 PNG 派生资源；这是下一项。自动队列及当前预览刷新已接线，不等于网络原字节保留已经完成。
- R2-C：补全同步、远端接收、删除/迟到/重试，以及真实双设备验证仍待完成。
- R3：新增标题、超链接目标/安全打开、引用、代码及高亮模型/两端展示仍待完成；基础强调和实色改善不等于这些功能已经具备。
- R4：真实帖子、目标编辑应用的直接粘贴/历史恢复对照、来源关闭/重启/断网、Windows 原生、辅助功能与双设备验收仍 UNKNOWN。

本轮没有更新已安装 App 或发布候选包，没有读真实历史 DB/系统剪贴板，没有停止日常 App、stage、commit、reset 或 clean。Release 仍为 NO-GO，整个方案不能标为 complete。

## R2-A 交付：自动保存网络图片（2026-10-08）

本节是最新一轮交付；前述 R1/R2-B 的数量与指纹是其各自时点的历史证据，不认证本节源码。

### 双子代理与主代理职责

- 设置子代理 Mill，`01a11727-963d-73b1-b291-8cb12351d246`，任务 `TAILSYNC_R2A_SETTINGS_20261008_HIGH`：仅 Core 设置与设置回归。
- UI 子代理 McClintock，`01a11727-95c0-7e52-aa84-d1857a463c9a`，任务 `TAILSYNC_R2A_UI_20261008_HIGH`：两端历史设置区、Swift 设置模型、本地化及相关回归。
- 两名均为内置 Multi-Agent、`cline-pass/deepseek-v4.1-flash`/`high`、完整明文、`fork_context: false`。先 ACK 后 START；实际各轮运行上下文与限定写集已核对。均完整交付并关闭，没有第三名代理或后台 CLI 替代。
- 主代理负责队列、生命周期、采集/设置/删除接线、归档提交通知、当前预览刷新、生产类型生成，以及完整交付后的独立审查、二次修复和最终门禁。

### 用户可见行为与安全边界

1. 历史设置区增加一个“自动保存网络图片”开关，字段 `auto_save_network_images`。两端持久化，默认关闭；旧配置缺字段也为关闭；非布尔值和未知字段不放宽。Windows 类型与 schema 由生产 Rust 重新生成。
2. 开启只授权此后新观察到的本机复制；自写、来源不明和远端接收不入队。不扫描旧历史，不改变当前系统剪贴板。采集重试保留首次观察的授权票据，避免开启开关后追溯下载此前正在重试的内容。
3. 共享队列最多一个活动任务、八个等待任务，只持有小型身份/authority 信息，不为每次复制创建无界任务或保留正文副本。队列满时原记录不丢失，仍可手动保存图片。
4. 关闭取消未完成的**自动**下载并撤销旧票据；已保存资源保留；显式手动保存不依赖此开关。删除、清空和退出取消相关自动任务。读前、提交前验证记录身份、数据库 authority、manifest CAS、取消与存储配额，迟到结果不能复活记录或覆盖手动补全。
5. 复用既有公开 URL、DNS pin、无凭据、重定向、下载/像素/解码预算，不放宽私网或原有格式限制。
6. 真正提交成功后从 DB worker 内发布历史通知；即使等待者在提交后取消，通知不丢失。两端列表/首图沿用缓存身份更新；当前文稿预览先校验保存 stamp，仅内容变更才重新加载，不因无关历史通知反复重建。关闭、导航、恢复或迟到校验不重新打开窗口、不自动恢复剪贴板、不抢焦点。刷新只保留轻量 request/stamp，不额外持有解码原件。

### 主代理独立审查与二次修复

- 复现并修复普通开关更新仍等待无关历史限额处理的问题：仅当历史限额或存储配额改变时才执行限额 DB 工作。设置持久化失败仍不提交；限额真的改变时的原有处理不跳过。保留 RED 日志与 GREEN 回归。
- 将授权绑定于首次观察与设置代际，而非每次采集重试重新取票；关闭再开启后旧票据无效。
- 为自动任务增加采集时身份/authority 的读前检查；同一记录身份保持不变但 DB 重新打开时，也不能沿用旧 authority 联网。
- 将通知放到真实归档提交内，补齐手动归档的通知；补充删除/清空、单 worker、停止、手动补全竞争等边界回归。
- 修正预览刷新使用可缺省 frame 字段重建请求的问题，保留安装时已验证的确切请求；修复新必填设置字段对应的完整测试 fixture 和新增 Clippy 问题。
- 缩短开关说明，区分自动取消与手动操作。移除在未挂载 SwiftUI `@State` 上赋值却声称展示开启状态的假预览；只保留默认布局渲染，开关往返和保存/失败由独立回归证明。

### 最终本地门禁

| 门禁 | 本轮最终结果 | 边界 |
| --- | --- | --- |
| Core lib | 786 PASS / 4 ignored | 全量串行；既有子进程 worker/性能基线未计通过 |
| Runtime | 102 PASS | 含自动队列 12 项和既有恢复/归档/预览接缝 |
| Windows UI | 397 PASS / 62 文件 | 包含开关、保存失败与预览刷新；不是原生 Windows |
| Swift | 333 PASS / 4 skipped，共 337 | 四项真实 key-window/WKWebView 宿主门禁仍跳过 |
| Core/Runtime Clippy | `--all-targets -D warnings` PASS | 无放宽规则 |
| 两端 adapter/test 编译 | composite feature PASS | Windows 在 macOS 宿主编译，不是 Windows 原生验收；既有 cfg dead-code 警告保留 |
| Rust Settings 与本地契约 | 生产生成/一致性检查 PASS | 没有手改 generated DTO |
| Windows 构建 | 真实 tsc + Vite PASS | 没有伪造 dist，也没有构建新安装包 |
| Windows 全量 lint | exit 0，22 条既有警告 | 按文件、规则、消息及重复次数与上轮逐项一致，无新增；窄范围 deny-warnings 因其中三条既有警告失败，不称零警告通过 |
| Rust 格式与 whitespace | 本轮 23 个 Rust 文件无新增格式问题，diff-check PASS | 两个历史命令文件原有恢复行的格式差异保留，没有顺便重排旧代码 |

首次误用默认并行 Core harness，因全局 pipeline budget 竞争出现 8 项失败；保留原始日志。按上一轮已采用的 `--test-threads=1` 完整重跑得到上述结果，没有跳过用例或放宽生产内存预算。默认并行 harness 不认证通过。错误 feature 名、lint 工作目录、临时请求类型和验证 runner 路径等中间诊断尝试不计作当前源码门禁。

补充核查了采集 `PipelineLease` 与自动归档的真实调用链：下载、资源绑定、该路径的 DB 读取与提交未再申请这个全局 ledger 的租约，而是使用既有下载并发、单图及累计像素/字节限制。因此未把“采集租约必然导致自动下载准入失败”的猜测认定为缺陷，也未据此修改代码。静态核查不等于峰值内存测量，不能宣称全进程内存硬限为 256 MiB；真实并发压力仍待验证，见 `parent-budget-review-01.json`。

### 证据、保护与剩余工作

证据根：`artifacts/rich-text-repair/2026-10-08-r2-auto-images/`。最终 844 个产品/测试/工具/配置/锁文件指纹为 `93b16b6e5c83626da4e2dabe622478c4ca444f6c73a1533f99b19d2f60676d66`，见 `parent-source-final-01.json`；最终验收索引 `parent-validation-final-01.json` 仅引用该指纹的父代理最终日志。基线保存 1185 个非 artifacts 文件；本轮修改/新增 50 个源码/测试/契约文件及本执行记录，HEAD 与暂存区不变，其余基线逐项核对无改动。

已复用原有 Rust/Swift 缓存，没有创建构建缓存副本。验证后磁盘可用约 83 GiB，Rust 本轮共用缓存约 9.7 GiB、Swift 369 MiB、Windows dist 2.5 MiB。未删除用户资源、既有 App/包或封存证据。

下一项是 R2-B 网络原始字节持久化，然后 R2-C 补全同步、R3 阅读语义和 R4 实机场景。当前图片补全只保存本机内部静态 PNG 层，不宣称下载原件已保留或下载完成后的资源已同步到另一设备。已安装 App 未更新；真实 GUI、目标编辑应用粘贴、Windows 原生、跨设备和 Release 仍未通过。整体目标继续 active，不标 complete。

## R2-B 交付：网络原图字节独立保存（2026-10-08）

本节更新当前进度，不改写此前阶段的历史结论、日志和指纹。R2-B 的本机保存实现及本地自动验证完成；不等于 R2-C 同步、真实网络/目标应用或发布验收完成。

### 双子代理与主代理职责

- Producer 子代理 Parfit，`01a1175b-2008-7101-b2f1-950fe6048756`，任务 `TAILSYNC_R2B_PRODUCER_20261008_HIGH`：仅修改 `shared/rust-core/src/composite/resource_archive.rs`，使成功下载结果返回未经修改的原图字节及其位置、格式和派生资源关联。专项 34 PASS，含 7 项新增回归。
- Codec 子代理 Hypatia，`01a1175b-2093-79d2-8d54-92fac32073a7`，任务 `TAILSYNC_R2B_CODEC_20261008_HIGH`：新增 `shared/rust-core/src/composite/network_originals.rs`，并仅在 `composite/mod.rs` 注册模块；交付专项 21 PASS。
- 两名均使用内置 Multi-Agent、`cline-pass/deepseek-v4.1-flash`/`high`、完整明文和 `fork_context: false`；先 ACK 后 START，Cargo 串行。实际全部运行上下文的模型/强度、ACK 早于工具、明确实际 workdir 和最终完整交付已核验，工具调用分别为 84/66 次。两名均已交付并结束，续接未重启旧代理或新增第三名。环境默认 cwd 仍是另一 checkout，不能把它当作实际写入工作树。
- 主代理负责 DB 与 runtime 接线、独立审查、生产接缝回归、二修、源码冻结和最终门禁。子代理报告不是父代理验收的替代品。

### 保存行为与边界

1. 成功且通过既有静态安全解码的 PNG/JPEG/WebP，保留 HTTP 响应的原始图像 body，另存内部预览 PNG。原 body 不替换成 PNG，也不进入 `NativeArchive` 的原始剪贴板域。失败、动画、SVG 和超预算图片不保存原 body。本轮不新增格式支持或 UI。
2. 新 `NIO1` 二进制载荷按 HTML 图片位置记录 `image_index/resource_id/format/bytes`。PNG 去重不会丢失逐位置的原图；不同原 body 派生出同一 PNG 时，两份原 body 仍保留。验证确切 HTML binding、格式与静态解码结果，不能用存在一个资源或匹配一个哈希冒充正确对应。
3. 数量最多 8、单原图最多 8 MiB、累计原图最多 16 MiB；单图 4 Mi、累计 8 Mi 像素，单派生 PNG 最多 16 MiB，均沿用原有限额。NIO1 还拒绝非零 reserved、未知格式、重复序号、资源 0、截断、trailing 和累计超限。codec 的结构校验与 `validate_originals` 的图像/绑定校验职责分开。
4. `history_parts` 增加一个加密 `network_originals` sidecar，与 `archive_layer`、manifest 和 archive identity 在同一 CAS 事务提交。重试按位置合并，不抹掉此前成功位置；旧手动归档接口及来源上下文补全保留已有原图。同一位置的不同原 body 不能覆盖已有原件。预留配额同时计算原 body、派生载荷和加密开销，拒绝后不遗留新外部文件。
5. 正常预览/分析不读取原图 sidecar，也不把它计入预览工作区；继续消费派生 PNG。显式原图读取校验载荷长度、哈希及原图与 PNG 的对应。原图损坏会明确失败，但不会单独使仍完好的派生预览不可用。收藏、清空、删除和 GC 复用通用载荷生命周期；共享载荷仍有存活记录时不删除。
6. runtime 自动队列及显式手动归档均接入同一原图提交接口，仍保留取消、身份、数据库 authority、迟到/删除保护和成功提交通知。下载完成不写当前系统剪贴板，不新开预览、不抢焦点。

### 兼容与身份规则

- 本地 manifest **v5** 显式标识新角色；必须有派生 archive layer 和恰好一个有效 NIO1 sidecar。来源 proof 可同时保留，记录采用对应完整性方法。
- 旧 v1–v4 继续读取，没有新增 manifest 序列化字段或重定义旧冻结哈希。schema 仍为 13，PortableBundle/传输协议未扩展。
- **旧客户端不认识 v5，会显式拒绝新记录**；不能据此宣称降级旧 App 后新记录可读。旧格式兼容与向旧包回退是两个不同承诺。
- 下载补全只改变 archive revision/digest；capture/content/dedupe 身份和历史 `data_hash` 保持，NativeArchive 编码仍逐字节相同。保存网络原 body 不代表历史恢复已经改为向目标应用输出 JPEG/WebP 原编码。

### 主代理独立审查与二次修复

- 真实 RED→GREEN：已有共享原图文件损坏时，旧文件复用仅看 `is_file`，可能把损坏文件误报为保存成功。新增 `network_originals_corrupt_shared_file_cannot_be_reused_as_a_successful_save` 先真实失败，再在新 sidecar 的外部持久化分支 bounded 读回密文、解密核对长度与 BLAKE3；不匹配则回滚。既有存活共享文件不删除、不重写。RED 日志保留，不能计为 PASS。
- 每份外部载荷成功写出后立即安装回滚 guard；第二份保存/SQL 失败能清理本次第一份，既有共享文件受到保护。原图输入先验证，避免合并掩盖重复序号，再验证累计原图预算与派生对应。
- 补齐空 originals 不要求 HTML 的旧文本兼容回归；修复 runtime 重开测试使用不可见 `open_at` 的编译错误，改用公开的 composite probe policy，并确实关闭旧 DB 后重开。
- DB 专项 10 项覆盖三格式原字节重开、旧接口/部分重试、SQL 双层回滚、原图配额、损坏与预览隔离、取消/旧身份/删除/重复输入、加密及收藏回收、NativeArchive 不变、来源 proof 两种提交顺序和共享损坏文件误报。Runtime 新增 3 项覆盖手动三格式、失败位置重试及自动队列的真实 producer→DB 接缝。
- 修复 codec 测试的 Clippy 字节字符串规范问题，无新增 allow、跳过或放宽预算。最终 codec 共 22 项回归。

### 最终父代理门禁

以下 11 项门禁均绑定最终相同源码指纹 `4ae1e9953580ce9bdb8c8ab2bc6e23d92c4894d03375b1de82bb42017c2cea5f`。Rust 全量串行，数据/配置使用隔离 runner，共用既有缓存。

| 门禁 | 本轮最终结果 | 日志/边界 |
| --- | --- | --- |
| Core lib | 825 PASS / 4 ignored | `parent-core-final-01.log`；含 codec 22、producer 34、DB 新回归 10 |
| Runtime | 105 PASS | `parent-runtime-final-01.log`；新增原图生产接缝 3 项 |
| Windows UI | 397 PASS / 62 文件 | `parent-windows-test-final-01.log`；不是原生 Windows 验收 |
| Swift 整包 | 333 PASS / 4 skipped，总计 337 | `parent-swift-test-final-02.log`；未认证 key-window/WKWebView 四项实机门禁 |
| Core/Runtime Clippy | `--all-targets -D warnings` PASS | `parent-clippy-02.log`；同最终源码 |
| macOS adapter/test 编译 | composite feature PASS，JSON 零编译警告 | `parent-macos-check-final-01.log` |
| Windows adapter/test 宿主编译 | composite feature PASS，4 条既有 unique 警告 | `parent-windows-host-check-final-01.log`；与 R2-A 诊断一致，不是 Windows 原生 |
| Rust Settings 生成检查 | PASS | `parent-settings-contract-final-01.log`；未手改 generated |
| 本地契约检查 | PASS | `parent-local-contracts-final-01.log` |
| Windows 构建 | 真实 tsc + Vite PASS | `parent-windows-build-final-01.log`；未构建新安装包 |
| Windows lint | exit 0，22 条既有警告，无新增 | `parent-windows-lint-final-01.log`；按原始诊断与重复次数核对，不能称零警告 |

本轮 7 个 Rust 文件以 rustfmt stdin/skip_children 只读核对全部一致，`git diff --check` PASS。Core ignored 为三个手动性能基线及一个由父测试调用的 subprocess worker；Swift 四项 skip 仍要求真实 GUI 测试宿主。本轮未测量全进程峰值内存，不宣称局部预算等于全进程硬限。

中间失败完整保留：codec attempt1 是父代理 rustfmt stdout 的文件名头误入新测试文件，attempt2 是其测试辅助偏移和累计像素阈值错误，attempt3 通过；runtime 首次编译诊断、共享损坏原图 RED、首轮 Clippy 不计作最终通过。Swift 首次在构建阶段 exit 139，未开始执行用例，根因未证实；用同一 scratch `--jobs 1 -v` 完整重跑通过，没有移除测试或扩大 skip。最终验收只引用最新相同源码的通过日志。

### 证据、保护与下一项

证据根：`artifacts/rich-text-repair/2026-10-08-r2b-network-originals/`。`parent-source-final-01.json` 保存 847 个产品/测试/工具/配置/锁文件清单；`parent-validation-final-01.json` 是父代理最终验收索引；`delegation-audit.json` 保存模型、收件/交付与实际工作树核验；`parent-evidence-seal-final-01.json` 封存本轮文档和日志。1190 个基线非 artifacts 文件逐项保护，本轮仅 7 个产品/测试文件及本执行文档改变，新增其中 3 个文件；HEAD/暂存区不变，其余 1185 个基线文件不变。R2-A 封存证据及其基线另作逐字节复核，未改写。

复用 `target/rich-text-r1` 与既有 Swift scratch，没有新增缓存副本、删资源或覆盖旧 App/候选包；验证时磁盘约 82 GiB 可用。未读真实 DB/系统剪贴板、停止日常 App、stage、commit、reset 或 clean。

下一项是 **R2-C：下载补全同步与可信接收**，随后 R3 阅读语义、R4 真实帖子/目标编辑应用直接粘贴与历史恢复对照、来源关闭/重启/离线、Windows 原生、辅助功能和双设备验收。本轮保存仅本机，不宣称补全资源已同步到另一设备。已安装 App 未更新，真实 GUI/目标应用/设备门禁仍 UNKNOWN，Release 仍 NO-GO，整个目标保持 active。

## R2-C 交付：下载图片补全同步（2026-10-08）

**R2-C LOCAL_COMPLETE；真实设备 UNKNOWN；Release NO-GO。** 本节是最新进度，前节“不含补全同步”是 R2-B 时点的历史边界，不改写其封存证据。详细行为、预算、测试与回退见 [本轮验收](RICH-TEXT-R2C-ACCEPTANCE-2026-10-08.md)。

两名内置 `cline-pass/deepseek-v4.1-flash`/high，完整明文 ACK 后 START：Carver 负责 ARU1 编码/派生，Dewey 负责事件映射/DB 同事务合并。写集互斥，已完整交付并关闭，无第三名、独立 chat 或 CLI 背景 agent。父代理冻结首交付、独立复核、二修和最终验收。

- 原始 SharedEvent 在真正广播前绑定本机历史、原事件 ID 与 base digest；自动下载放在原始入队之后。下载恰好先于绑定完成的手动竞争也有补偿发送。两端自动 worker、macOS API、Windows Tauri 命令和 Windows API 都通过同一 typed commit hook 和有界合并队列接线。
- 新独立 archive capability、三条命令、ARC1 chunk 和认证 DB 接收；旧客户端无能力即不发补全，绝不退成新复制。逐块检查 sender sync/peer/pinned-key/mode；receiver 再查认证授权，只有事务保存成功通知。补全不调用网络下载、native publisher 或剪贴板激活。
- 同一认证 source/原事件/digest 关联；原始身份和正文不变，追加原编码与 PNG 派生。缺失不创建、删除不复活，重复/倒序/碰撞、来源 proof、配额/取消/SQL 与文件失败有回归；live 映射保留，总 metadata cap 仍 4096。
- 三项真实 RED→GREEN 二修：tracker-only 身份/CAS 缺口、后台错误误计剪贴板丢失、重复 receipt 对损坏原图误报成功。清理代理新增 doc comments、未使用的共享测试工厂，拒绝远端历史误绑为本机原创。没有放宽预算或增加 allow/ignored。
- Codec 24、DB 18、Core archive 收发 5、Runtime 本轮集成 3 项通过。自动/手动下载、部分失败重试、真实二进制补全帧、同一条历史、原字节与重开离线预览/恢复载荷贯通；base native 效果与网络 source 是 fixture 模拟，不冒称两设备/native GUI 验收。
- 最终同指纹 16 个命令加格式/diff，共 18 项门禁通过：Core all-features 887＋4 ignored、Core integration 14、Runtime all-features 119、Protocol 63、Windows UI 397、Swift 333＋4 skipped、canonical tests 27；workspace all-targets/all-features Clippy、两 adapter/test 编译、生成/契约/canonical/features、Windows build/lint 通过。macOS 编译零警告，Windows 宿主 4 条既有 unique 警告、UI lint 22 条既有警告，无新增。
- 早期三份自然接口 Iroh 超时日志保留；最终两次完整复跑通过，但根因未证明，不宣称偶发网络问题已根治。代理的临时目录命令、DB cd 前缀代替 workdir、codec Cargo file-lock、父代理测试编译错误和 draft diff 损坏也在证据中如实保留，不把计划要求当已认证事实。

最终源码 854 文件，指纹 `c4a0e2ee989f55b61e91296fec9c7b90f15598fedefdcb50076d4db99cc4848d`。本轮定向变化 34 个产品/测试/工具文件，其中新增 7 个、零删除；1193 个非 artifacts baseline 的 1165 个未改，28 个定向改动，HEAD/暂存区保持。原封存/基线、旧 App/候选包保护，未读用户 DB/general 剪贴板、停止日常 App、stage/commit/reset/clean 或复制新缓存。

证据根 `artifacts/rich-text-repair/2026-10-08-r2c-archive-sync/`，最终索引 `parent-validation-final-04.json`，源码清单指针 `parent-source-final-04.json`，封存 `parent-evidence-seal-final-04.json`；有效回退补丁只用 `final-delta-v2/` 对本轮 baseline 定向恢复，不能 reset 到 HEAD。

R2-C 交付时下一项 **R3 阅读语义**尚未启动；后续状态见下节。当前安装 App 未更新，整体方案 active，不标 complete。

## R3 启动：常用阅读语义（2026-10-08）

按总体方案继续推进 R3，执行方案见 [本轮计划](RICH-TEXT-R3-EXECUTION-PLAN-2026-10-08.md)。两名内置 cline-pass/deepseek-v4.1-flash/high，fork_context=false、完整明文 ACK 后 START：Dirac（01a117e6-0149-77b3-a9d9-605dd9fb2561）负责 Core 阅读模型与 HTML，Popper（01a117e6-01ca-7470-a69c-17b3ca2cc550）负责两端展示与安全链接。父代理负责有限 RTF 位置证明、生产契约与独立审查二修；不重叠写集，门禁由父代理串行运行。

本轮 baseline 保存 1203 个非 artifacts 文件；R2-C 的 239 项封存摘要及源码指纹已只读复核一致。父代理真实回归确认原先 RTF-only 文稿正文为空，随后实现有界逐位置六种基础强调，保留原 RTA1 编码与原件；原始字节、正文或来源不能证明对应时不推测原生样式。新链接须计入文字/模型预算，两端契约由生产 Rust 生成，不手改生成文件。实施与交接还在进行中；不把计划或子代理报告当最终通过证据。

证据根 `artifacts/rich-text-repair/2026-10-08-r3-reading-semantics/`；旧 seal、App、用户数据及系统剪贴板不变。R3/R4 与整个目标均未完成，Release NO-GO。

## R3 交付：常用阅读语义（2026-10-09）

**R3 LOCAL_COMPLETE；真实 GUI/目标应用/设备 UNKNOWN；Release NO-GO。** 本节与执行清单是当前状态，上节保留启动时点的历史记录。详细行为、独立二修、测试与保护见 [R3 验收](RICH-TEXT-R3-ACCEPTANCE-2026-10-09.md)。

两名内置 DeepSeek/high 已完整交付并关闭，没有重派、第三名或 CLI 后台 agent。父代理完成独立审查，补齐有限 RTF 逐位置强调及生产契约，修复收集阶段的累计预算、安全链接/base、表格作用域、嵌套列表计数、显式颜色对比度及链接打开失败的默认重试问题。

预览新增三级标题、用户点击才打开的安全 HTTP(S) 链接、引用、保留空白的代码、有界列表层级与高亮；保留已有强调、图片位置和简单表格。RTF 只在正文逐字匹配、位置可证明时应用六种基础强调。不是完整网页或 Word 引擎，不从预览重建原件，不扩大原始采集/恢复格式范围。

最终候选指纹 `b1435a68d4663afe1dd2a6609d72601b3d79026916f8762430087b5457300595`，856 个产品/测试/工具/配置文件；`parent-*-final-05` 的 20 项门禁均成功且执行前后源码不变。Core 914 PASS / 4 既有 ignored，integration 14 PASS，Runtime 121 PASS，Protocol 66 PASS，Swift 344 PASS / 4 既有 skipped，Windows UI 410 PASS，canonical 27 PASS，本地契约 3 PASS。Windows 全量 lint 回到与 R2-C 相同的 22 条既有警告，限定文稿及生成契约文件 deny-warnings 通过；Windows adapter 的 4 条既有编译诊断逐项一致，不称零警告或原生 Windows 通过。

代理审计明确保留一次被拒绝的 Core patch、一次 grep 无匹配和一次 diff 发现变化；不把缺失数值退出码记成成功。展示代理在 CSS 路径正式修订前修改实际 `windows/src/styles/preview.css` 的执行顺序偏差保留。父代理的编译、断言、契约、Clippy、lint 与格式失败尝试也不覆盖。

本轮 25 个产品/测试/工具文件新增或修改，其中 2 个新增，零删除。1203 个基线非 artifacts 文件、HEAD/暂存区及 R2-A/R2-B/R2-C 旧封存另行校验；最终索引、保护审计及定向回退补丁封存在本轮证据根。复用既有 Rust/Swift 缓存，未更新已安装 App、替换旧包、读取真实 DB/general 剪贴板、停止日常 App 或 stage/commit/reset/clean。

下一项为 **R4 真实场景验收**：同一目标应用直接粘贴与历史恢复成对对照，真实帖子、来源关闭、重启、离线、辅助功能、Windows 原生和真实两设备补全分别取证。真实 HTTP/DNS、托管 CI及新候选安装包仍未验收。整个方案继续 active，不因 R3 本地通过而标 complete。

## R4 准备交付：真实场景样本与候选（2026-10-09）

**R4 PREPARED；真实 GUI/目标应用/公开网络/Windows 原生/双设备 UNKNOWN；Release NO-GO。** 本节补充准备结果，不把 R4 实机标为完成。详细步骤与可填写矩阵见 [R4 计划](RICH-TEXT-R4-EXECUTION-PLAN-2026-10-09.md) 与 [手工清单](acceptance/rich-text-r4-manual-checklist-2026-10-09.md)。

两名内置 cline-pass/deepseek-v4.1-flash/high，完整明文 ACK 后 START：Lorentz（01a11f03-039d-7c80-b40d-c34a36ed16f7）负责样本工具与回归，Gauss（01a11f03-041f-7d31-a383-5d9b7e87c4e4）负责方案与清单。两者已完整交付、关闭，父代理独立审查二修与隔离门禁；没有第三名、重派或后台 CLI agent。

父代理在初版发现多播 IP 被误允许，并通过新增回归修复 URL/IP/路径边界、IPv6 地址重建、错误部署可验收状态和 RTF 非 BMP 转义；补齐安全链接、非 HTTP 链接、简单表格及显式颜色样本。初版 25 项失败日志与二修前 29 项的 16 failures / 1 error 均保留，最终 29 项通过。临时 shell cat 编辑偏差、被拒绝的 rm -f 和只读无匹配退出如实审计，不记为测试成功。

工具实际生成八个静态文件：reading/styles/partial HTML、editor.rtf、原始 PNG/JPEG/静态 WebP 三图和 manifest。manifest 覆盖其余七文件，父清单额外记录 manifest 本身。样本不联网、不启动服务、不碰用户 DB/系统剪贴板，默认 example.com 仅文档占位；即使传公共地址也保持部署 UNKNOWN，不冒称实际发布或真实浏览器/编辑器复制。

新的 arm64 debug/ad-hoc 候选 TailSync-R4-20261009.app 已实际编译，主 UI、daemon、clipboard-helper 的静态签名与 macOS 13.0 部署检查通过。整包清单摘要 b1f98889e0fbd94c0627baf1ecda7b831088930095b5b8f3e4d08cf39848de0e；未启动、未替换日常 App。构建时绑定 R3 b1435a68 指纹；后续工具与 CI 修改使当前 858 文件指纹为 b5dc8000de811b55c944da9e69bbbb2d8bcdd233bf867ba854abf88ac1cfcf50，二者明确分开。

当前源码的 22 项完整本地门禁通过且执行前后源码不变：Core 914＋4 ignored、integration 14、Runtime 121、Protocol 66、Swift 344＋4 skipped、Windows UI 410、canonical 27、本地契约 3、fixture 29、CI scope 18，另含生产生成检查、两 adapter 编译、Windows 构建/lint、Clippy、格式/diff。Windows 22 条既有 lint 警告及 4 条宿主编译诊断与 R3 核验一致，不宣称零警告、原生 Windows 或托管 CI 通过。

隔离审查发现单实例锁、daemon 日志和生产钥匙串仍按当前系统用户共享；仅换数据目录、socket 或 bundle ID 不够。不得在日常账户启动候选，也不得运行会读写 general 剪贴板的 verify_macos_bundle.sh。实机验收须人类提供并授权独立 macOS 系统测试账户或 VM；VM 禁止宿主共享剪贴板、共享个人数据与钥匙串。还须指定一款浏览器、两款实际编辑器及真实公共部署地址，Windows 与双设备分别提供。

证据根 artifacts/rich-text-repair/2026-10-09-r4-acceptance/；本轮基线为 1206 个非 artifacts 文件，当前源码、最终门禁、代理审计、样本/包摘要、要求矩阵、保护核验与定向回退补丁分别封存。保留 HEAD/暂存区与 R3/R2 旧封存，不读取真实历史/剪贴板、不停止日常 App、不覆盖旧包、不 stage/commit/reset/clean、不复制或清理既有构建缓存。

下一项是满足隔离环境与授权前置条件后的 **R4 实机成对对照**，不是继续扩大格式支持。当前整个方案仍 active；尚无真实观察的格子保持 UNKNOWN，不能把准备封存当作整体完成。
