# Windows 轻量化 UI 方案评估

日期：2026-10-26。范围由用户确认：保留 macOS 原生界面，重点优化 Windows，外观尽量与 mac 端一致，低内存和响应速度为硬要求。

**建议优先验证 Rust + Slint，以现有 WPF 原型作等价对照；通过资源和交互门槛后再确定生产选型。** WinUI 3 保留为候选，不再因 Windows Fluent 外观而优先。此前 `PROJECT-REVIEW-2026-10-25.md` 的排序侧重 Windows 原生体验，本报告按本次明确的轻量化与 mac 外观目标重新排序。

本报告完成源码检查和官方资料核对，没有运行 Windows 性能测试。以下性能数字均为建议验收预算，不是框架实测值或效果承诺。本轮只新增评估文档。

## 1. 项目已有基础与真实边界

| 源码事实 | 对选型的影响 |
|---|---|
| macOS 使用 SwiftUI/AppKit；共享 Rust Core 和 `tailsync-runtime` 承载业务 | 保留 mac 实现；Windows 重做展示与平台接线，复用业务规则 |
| Windows 使用 React 19 + Tauri 2；配置 `app.windows` 为空 | 不存在配置层预建全部窗口的问题；启动时实际创建行为仍需运行追踪 |
| `window_lifecycle.rs` 已在关闭后隐藏，并在 5 秒空闲后销毁窗口 | 不能把“关窗销毁”当作全新优化；须核实关联 WebView2 进程何时退出 |
| 历史按 30 条分页；缩略图缓存默认字节预算 4 MiB，条数上限为一页 | 当前并非把全部历史渲染到 DOM；仅添加虚拟列表不一定有显著收益 |
| `PreviewContent.tsx` 已按类型懒加载 PDF/DOCX/文本等渲染器 | 不能把全部预览依赖体积直接当作历史窗口常驻内存 |
| `useVisiblePolling.ts` 已按可见性停止安排轮询 | 新方案应继承可见性和订阅生命周期，而非重复宣称解决同一问题 |
| Rust Windows 宿主仍使用 Tauri 的 AppHandle、任务、托盘、窗口和插件 | 替换 TSX 不等于脱离 Tauri；后台宿主解耦是实质工作 |
| 已有 WinUI 3 和 WPF 原型，均接 `DemoTailSyncClient` | 可复用设计和对照工程，但不能用示例原型与完整客户端比较内存并宣称收益 |

WPF 原型已经使用 `VirtualizingStackPanel` 和 `Recycling`，关闭时释放定时器和订阅。它仍缺生产 IPC、托盘、全局快捷键、完整 Theme V2 和部分预览；README 明确未完成 Windows 实机资源与交互验收。

## 2. 候选路线

表中评价是架构与项目适配判断，不是资源排行榜。

| 路线 | 轻量化机会 | 复现 mac 外观 | 本项目主要代价 | 结论 |
|---|---|---|---|---|
| **Rust + Slint** | 主窗口无需浏览器或 CLR；可选择渲染器与构建特性 | 自定义声明式控件适合复用颜色、尺寸、形状和状态 | 中文输入、读屏、多屏 DPI、焦点返回及复杂预览需验证；许可证需按固定版本确认 | **资源优先的第一验证对象** |
| **C# + WPF + Rust** | 主窗口无需 WebView2；成熟的控件复用与虚拟化 | 现有 Mac 控件模板可继续用，默认 WPF 皮肤不决定最终外观 | CLR、GC、模板和图形资源的开销仍需测；需要生产 IPC | **最快得到可比较结果的对照，也是落地备选** |
| C# + WinUI 3 + Rust | 主窗口可以不用 WebView2 | 能定制，但 Fluent 默认视觉对本次目标没有直接优势 | .NET 与 Windows App SDK 部署、窗口互操作、模板覆盖 | 保留现有原型，优先级低于前两者；不预断其内存必然更高 |
| 保留 Tauri + React | 可继续减少缓存、布局和窗口恢复开销 | CSS 很容易贴近现有 mac 设计 | WebView2 浏览器运行时成本仍在 | 保留作为生产基线和迁移回退 |
| Tauri + Svelte/Solid/Preact | 可减少部分 JS 和框架执行开销 | 可以实现 | 无法消除 WebView2 进程组；重写会消耗迁移预算 | 不作为本次核心路线 |
| Rust + Win32/Direct2D/DirectWrite | 可精细控制依赖、分配和绘制 | 自绘自由度高 | 输入法、文字选择、无障碍、布局、控件体系需大量实现 | 仅当前两者都无法满足预算时重评 |
| Iced/egui、Avalonia、Flutter、Qt | 各有非浏览器实现能力 | 自绘或主题定制可实现 | 引入新 UI 生态，暂无优于已有候选的项目证据 | 本轮不扩展原型数量 |

Electron 不符合本轮资源优先目标。框架名、安装包大小、空白窗口截图、进程数量都不足以证明轻量化。

### Slint 验证方式

- 固定正式版本和 lockfile；先确认目标 Rust 工具链与 Windows 支持，再裁剪默认特性。不能把访问时主分支的 feature 清单当作已选版本配置。
- 同一历史窗口分别构建 software 与一种 GPU renderer，测试中文文字、图片、滚动、DPI 和多窗口。软件渲染可能减少部分 GPU 资源，也可能增加 CPU 和帧耗时，不能预先选胜者。
- 使用事件驱动更新，不设置常驻逐帧定时器；模型更新只作用于变化行，图片解码与历史查询离开 UI 线程。
- 验证实际模型和视图是否只保留有界数据及控件。自绘或 Rust 并不会自动提供数据虚拟化，也不会自动消除多份图片缓存。
- 保留 accessibility 能力并实际检查 Narrator、Tab 焦点、组合输入、候选框位置、Enter/Escape、快捷键恢复后返回目标应用。
- Slint 框架并非单纯 MIT；官方列出 GPLv3、Royalty-free、Commercial 路径。Royalty-free 涉及使用披露，采用时核对固定版本条款，不能由 TailSync 自有源码的 MIT 推导整个分发包许可。

### WPF 验证方式

复用现有工程，接入与 Slint 相同的数据契约和场景。保持列表容器回收，并补充后端分页；UI 虚拟化不代表数据集合按需加载。避免每行复杂阴影、深层模板和整页重建；位图按显示尺寸解码，缓存按字节限制。窗口关闭与 UI 进程退出分开测量：隐藏窗口不会卸载 CLR，self-contained 发布也不等于原生 AOT。不要把未来的 AOT 支持作为当前预算成立的前提。

## 3. 与 mac 一致的实现方法

应共享设计数据和行为规范，分别实现渲染。当前 mac 风格已由项目自己的主题和布局定义，无须以 Windows 默认控件外观为目标。

1. **统一令牌来源。** 继续使用 `shared/tailsync-themes` 的解析结果，涵盖颜色、字体、密度、圆角、组件六态、动效和高对比。现有 WPF 的 240 个内置颜色值只是起点，不能替代完整 Theme V2。自定义主题包也应走同一 Rust 解析用例。
2. **统一几何。** 复用 `windows/native-ui/Design/mac-ui-manifest.generated.json` 和生成脚本。历史/收藏为 400×600，设置 540×500，连接 520×640；搜索高 36、筛选高 28、间距 8，均为逻辑尺寸。对齐前运行来源摘要检查，避免引用过期清单。
3. **统一可见状态。** 覆盖浅/深色、选中/焦点/悬停/禁用、长按收藏、菜单、加载、错误和空列表。来源、格式与不完整内容提示的位置及语义应保留。
4. **字体和图标明确适配。** mac 当前使用 Songti SC、Avenir Next 等；Windows 字体替代会影响字宽和换行。优先保持语义、字号、字重与基线，图标采用合法可分发的统一矢量资产；不直接打包 Apple 字体或 SF Symbols。若要求更严格字体一致，需选定两端均可合法分发的字体并评估包体和字形缓存。
5. **窗口效果控制成本。** 保留重要的圆角、边界与轻阴影，优先使用系统合成能力；大面积实时模糊、重复离屏效果需单独测量。窗口控制区可定制，但必须保留拖动、缩放、DPI、焦点和辅助功能语义。

验收以真实 mac 和 Windows 截图为依据，覆盖 100%/150%/200% DPI、长中文、英文、五种内置主题及深色模式。结构位置、尺寸和状态可以严格对齐；字体光栅化、系统菜单和窗口管理差异单独记录。HTML 评审预览不能代替 Windows 原生截图或性能结果。

## 4. 建议架构与资源生命周期

```text
Windows 登录用户会话
  Rust ApplicationHost（同步、托盘、快捷键、剪贴板、单实例）
    ├─ 共享 tailsync-runtime / Core / themes
    ├─ 类型化本地 Named Pipe → Slint UI 或 WPF UI（同一候选进程承载各窗口）
    └─ 按需预览进程（仅复杂格式需要时启动）

macOS：继续 SwiftUI/AppKit → 现有 Unix socket → Rust 后台
```

采用用户会话宿主，避免把剪贴板和交互窗口迁入 Windows Session 0 服务。UI 可重启或退出，后台同步继续。即使 Slint 与后台同为 Rust，独立 UI 进程仍有按需释放和崩溃隔离价值；同进程可作为后续测量分支，但不能据此假设更省总内存。

后台迁移按明确边界进行：先复用用例，再从 Tauri 接线层抽出 ApplicationHost。托盘、全局快捷键、通知、更新及单实例必须有唯一所有者，不能在两套宿主中重复监听。旧 Tauri 界面在过渡期仍接同一用例。

本地 IPC 沿用生成契约的业务语义，补充 Windows transport：同用户 SID/DACL、仅本机、对端/会话校验、握手版本、请求 ID、取消、有界帧和有界事件队列。大图和预览资产按二进制读取，避免整份内容经 JSON/base64 多次复制；新 UI 不直接读数据库或密钥。具体协议须与现有 local contract 版本机制协调。

窗口生命周期比较三档：关闭立即退出 UI、短暂保温、保留小型历史窗口。实验先用 0/5/30 秒，测冷开和热开后确定策略；5 秒是现有实现值，30 秒只是试验值。保温期间停止不可见渲染和订阅，释放大图、预览和过期查询。不强制 trim working set，也不通过主动 GC 掩盖长期分配增长。

## 5. 复杂预览决定轻量化能否持续成立

常用路径（历史、收藏、设置、连接、文本、普通图片）以无浏览器为目标。复杂预览另列预算和能力清单：

| 内容 | 建议方向 | 必须保留或明确处理的边界 |
|---|---|---|
| 文本/代码 | 有长度预算的文字视图，异步高亮 | 中文选择、搜索、换行和字号；不得阻塞主线程 |
| 图片 | 按视口/DPI 解码，缩略图与原图分开，字节上限缓存 | 快速切换取消旧请求，释放像素缓冲和 GPU 纹理 |
| 复合图文 | 复用既有 DisplayDocument 和恢复规则 | 图片绑定、表格、来源与不完整提示不丢失；不在 UI 重新判定可恢复性 |
| PDF | 评估 Windows PDF API 或 PDFium | 按页位图不等于保留 PDF.js 的文字选择、搜索等能力 |
| DOCX/复杂 HTML/SVG | 先核对现有能力；必要时用隔离、按需启动的浏览器 renderer | 关闭后释放；限制网络和资源访问；兼容性不能因主 UI 迁移而静默下降 |

主 UI 无 WebView2，不代表打开复杂文档时整个产品也没有浏览器。若后续要求全场景完全移除 WebView2，需要额外原生渲染实现与格式验收，不能算作免费的框架替换收益。

## 6. 可量化的验收建议

以下是待实测校准的工程目标，尚未批准为发布门禁，也没有已达标结论。基准固定同一 Windows 机器、Release 构建、DPI、网络/配对状态、数据集和功能集合；保留所有进程与 GPU 指标，后台占用单独列出但不得从整机产品结果中隐去。

| 指标 | 第一轮建议目标与口径 |
|---|---|
| 无窗口、保温结束 | 不存在非必需 UI/预览进程；报告后台总占用、CPU 和唤醒 |
| 单历史窗口资源增量 | 相对等状态的后台基线：Private Working Set ≤50 MiB，Private Bytes ≤80 MiB；作为初始挑战目标，非框架承诺 |
| 整体收益 | 同功能场景整个产品进程树 Private Bytes 相对现有 Tauri 目标降低 ≥40%；同时报告绝对值，避免基线小时误导 |
| 热唤起 | 快捷键接收到历史首屏可输入的 p95 ≤100 ms，且不劣于当前基线 |
| 冷唤起 | 后台已启动、UI 进程不存在时，快捷键到可输入的 p95 ≤350 ms；另测真正系统冷启动 |
| 搜索 | 中文组合态提交后到结果稳定的 p95 ≤100 ms；输入防抖计入总耗时，组合态不触发恢复 |
| 滚动 | 60 Hz 下帧时间 p95 ≤16.7 ms，同时记录长帧和输入延迟 |
| 稳态与泄漏 | 100 次开关/切换后，在相同静置时间采样；内存、句柄、订阅、任务数无持续增长，关闭后回归稳定区间 |

数据先用 500 条混合历史，当前每页 30 条；包括长中文、普通文本、文件批次和图片。如加入 1k/10k 压测，用隔离夹具并注明超出当前设置 UI 的历史上限，不能假定生产配置支持。图片记录分辨率而不只记录压缩文件大小。

测试场景包括：首次启动仅托盘；首次/热开历史；同时开历史、收藏、设置和连接；大图、PDF、DOCX 预览；关闭后 5 秒/30 秒/5 分钟；同步过程中搜索；休眠恢复；多屏和远程桌面。至少采集足够的重复样本报告 p50/p95 与波动，不能把一次最快结果当结论。

使用 Process Explorer/PerfMon 快速取数，WPR/WPA 归因。统计产品关联的全部进程（包括通过共享 WebView 环境创建的进程），不要仅按简单父子关系遗漏 renderer。Private Bytes 与 Private Working Set 分开；共享 Working Set 不直接求和当独占内存；GPU 使用单列。

## 7. 最小验证顺序与选择规则

1. 记录现有 Tauri 的真实基线，并核验设计清单是否与当前 mac 源码一致。
2. 在隔离数据上，让 WPF 原型和一个 Slint 历史窗口消费同一生产历史用例，覆盖搜索、分页、缩略图、收藏保护、键盘、文本/图片预览与焦点恢复。静态示例只用于初步 UI 排查。
3. 对比 Slint 两种 renderer 和 WPF 的同功能结果；首先排除中文输入、读屏、快捷键和数据语义失败的候选，再比较内存和速度。
4. Slint 通过交互门槛且达到资源预算时优先推进；若它在关键桌面交互上仍有阻碍，而 WPF 达标，则选择 WPF。两者均不达标时先定位后台、图片、IPC 或渲染成本，再决定是否投入 Win32/Direct2D；不以换框架次数代替分析。
5. 确定候选后再完整迁移设置、连接、引导、主题包和各类预览；维持旧客户端回退，完成 Windows 安装、更新及双设备验收后才替换生产 UI。

## 8. 证据与资料

本地源码：

本轮只读校验通过：`node windows/native-ui/Design/sync-mac-design.mjs --check`（6 组、461 条 Mac 文案，设计来源一致）；`node windows/native-ui/Design/export-wpf-themes.mjs --check`（5 套主题 × 浅深色 × 24 色）。这只证明生成数据与来源一致，不证明原生渲染效果或性能。

- `windows/src-tauri/src/window_lifecycle.rs`：5 秒释放和透明窗口配置。
- `windows/src/utils/historyPagination.ts`、`windows/src/pages/history/HistoryConstants.ts`：每页 30 条与缓存条数。
- `windows/src/hooks/useThumbnailCache.ts`、`useVisiblePolling.ts`：缓存预算和可见性生命周期。
- `windows/src/preview/PreviewContent.tsx`：按类型懒加载。
- `windows/src-tauri/src/lib.rs`、`Cargo.toml`：Tauri 宿主接线和依赖。
- `windows/native-ui/README.md`、`TailSync.Wpf/README.md`、`TailSync.Wpf/MainWindow.xaml`：原型范围、运行边界与虚拟化。
- `windows/native-ui/Design/mac-ui-manifest.generated.json`、`macos/swift-ui/Sources/TailSync/TailSyncApp.swift`：布局尺寸。
- `docs/THEMING.md`、`shared/tailsync-themes/`：主题契约。

本次访问的官方资料：

- [Microsoft：WebView2 process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)：进程组、环境共享和 renderer 关系。
- [Microsoft：WPF 控件性能](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/advanced/optimizing-performance-controls)：UI 虚拟化、数据虚拟化与容器回收的区别。
- [Slint 官方 Rust feature 定义](https://github.com/slint-ui/slint/blob/master/api/rs/slint/Cargo.toml)：多种 renderer 与 accessibility；访问的是主分支，未据此指定生产依赖版本。
- [Slint 官方许可](https://github.com/slint-ui/slint/blob/master/LICENSE.md)：许可路径与使用披露。

Slint 在线 renderer 指南本次返回 HTTP 403，渲染特性核对改用官方仓库源码；未把未读取的网页内容当作核验结果。

## 9. 现有功能完整迁移可行性复核

用户追问：目前所有功能是否都能在推荐技术栈中实现。范围仍为 Windows 现有功能；macOS 独有的 Quick Look PPT/PPTX 预览不算成 Windows 已有能力。

**结论：没有发现必须依赖 React 才能成立的业务功能。采用 Slint 主界面 + Rust 后台 + Windows 平台适配 + 按需复杂预览的组合，现有功能有完整迁移路径；但纯 Slint 控件不足以直接替代所有现有渲染器。不能同时承诺“全部功能等价、完全去浏览器、全场景低内存”已经得到验证。**

以下“可实现”指架构可行性，不代表完成移植或通过 Windows 实机验收。

| 当前功能 | 迁移路径 | 难度与验收重点 |
|---|---|---|
| 文本、图片、文件及复合内容同步；加密、断点续传、去重 | 保留 Rust Core 和平台剪贴板适配，Slint 展示状态 | UI 框架不限制这些业务；后台脱离 Tauri 后仍须验证任务和生命周期 |
| LAN/Tailscale/Iroh 发现、连接测试、配对、邀请链接、设备管理 | Rust 用例 + Slint 表单/设备行；协议链接由 Windows 宿主接收 | 取消、验证码双方确认、冷启动链接投递和单实例行为不能丢失 |
| 历史搜索、分类/日期筛选、分页、批次展开、收藏、删除保护 | Slint 列表与弹层，复用历史用例 | 中；稳定 ID、跨页选择、长按与双击冲突、保护规则 |
| 原格式/可用格式恢复、图片归档、浏览器来源授权与导入 | Slint 操作入口，复用 Core 判定与平台适配；接文件对话框 | 中；预览成功不等于恢复完整；原件、缺失资源和错误语义必须保留 |
| 文件进度、取消、暂停同步、存储诊断和位置迁移 | 后台状态事件 + Slint 表单和提示 | 中；迁移进度、回滚、保留/删除旧目录与阻塞状态 |
| 常规设置、快捷键录制、中英文 | Slint 控件、共享文案、平台注册器 | 低至中；录制期间暂停旧快捷键与冲突反馈 |
| 内置/自定义主题、安装/更新/回滚、四种预览 | Rust Theme V2 解析 + Slint 令牌适配 | 中；不只迁移颜色，组件六态、字体、密度、形状和资产都要覆盖 |
| 托盘、通知、开机启动、单实例、系统文件/目录选择 | Rust Windows API 或适当库 | 中；原 Tauri 插件负责的能力需重新接线；通知身份与点击激活需保留 |
| 签名更新、安装、重启、便携版 | 复用更新验证规则，适配宿主/安装器 | 中高；不能把新 UI 更新后旧 host 仍驻留当作更新完成 |
| 独立多窗口、置顶、最小化联动、位置记忆、DPI | Slint 窗口 + Windows 窗口管理适配 | 中高；无激活显示、虚拟桌面、屏幕边界与回焦需目标机器验证 |
| 中文 IME、键盘取用、预览深读、Narrator | Slint 输入/无障碍接口 + 必要平台适配 | 高优先级风险；必须原型先验，不能只验证英文输入和按钮点击 |
| 15 步真实窗口引导与演示数据 | 复用共享内容/演示模型；用 Slint 控件坐标重写蒙版和命中测试 | 中高；DOM 锚点不可直接复用，滚动跟随、跨窗、模态让位和安装身份需保留 |
| 纯文本选择、搜索、复制、换行、字号记忆 | Slint 文字控件/专用文字视图 + Rust 搜索 | 中；大文本、中文选择与滚轮缩放 |
| 代码高亮、行号、文本/代码切换 | Rust 高亮库 + 可选择的文字布局，或暂用预览进程 | 中高；分段着色、连续选择、换行和行号对齐须共同成立，不能只画彩色文本 |
| 图片及图片文件预览、缩放、平移、旋转、透明背景 | Rust 解码器 + Slint 图像视图 | 中；格式覆盖与现有行为一致，首帧、像素预算、快速切换取消和纹理释放 |
| Markdown 标题、列表、任务、引用、代码块、表格 | 原生解析/布局需重做；第一阶段复用按需 Web renderer | 高；解析器输出 AST 不等于具备排版、选择、链接和资源限制 |
| DisplayDocument 复合图文、表格、样式与缺失提示 | 共享文档模型保留；Slint 重做文档布局或复用预览进程 | 高；表格跨度、混合文字、图像绑定与部分内容诊断，不能降为纯文本冒充等价 |
| PDF 分页、文字选择、异步搜索、缩放、缩略图 | PDFium 等库 + 自定义交互，或继续按需运行 PDF.js | 高；系统 PDF 位图输出不足以替代文字层与全部交互 |
| DOCX 本地预览 | 按需复用 docx-preview + WebView2；彻底原生需另选文档引擎 | 高；Slint 没有可直接替换当前 DOCX 排版的标准控件，不以外部 Office 打开代替应用内预览 |
| SVG 图形/源码切换、每条目外链信任 | 按需保留受限浏览器；原生 SVG 库需逐项验证覆盖 | 高；能显示 SVG 图标不等于兼容当前 SVG 文档、CSS、字体和明确批准的外部资源 |
| 不支持格式的元信息与恢复、加载/损坏/超限/重试提示 | Slint 元信息界面与现有错误契约 | 低至中；不扩大支持承诺，不把所有失败合并成“无法预览” |

复杂预览进程可复用现有 React 渲染器，但仍需适配 IPC、主题、窗口控制、键盘焦点和资源释放。它应仅在使用相关格式时创建，独立预览关闭后释放，常用主界面不因此预加载浏览器。保留该进程也意味着复杂预览场景的内存可能接近现有方案，不能把主界面的预算套到整份大型文档上。

若选择完全无 WebView2，优先验证四个难点再决定：PDF 的选择/搜索、DOCX 实际版式、SVG 现有受限兼容范围、Markdown/复合文档连续文字选择及表格。浏览器移除后 CSP/iframe sandbox 不再适用，需由新渲染器落实等效的联网限制、资源访问、大小/像素预算和取消机制；恢复 Core 中的正确性与权限边界继续保留。

建议投产判断顺序为：① IME/焦点/无障碍，② 代码/文档文字选择，③ 复杂预览隔离后跨窗口交互，④ 同功能资源测试。若 Slint 在前两项需要大量自研而 WPF 原型能较快达标，应选择 WPF；WPF 的成熟文字与桌面控件能降低部分风险，但也不会自动提供完整 PDF/DOCX 渲染器。

本轮复核依据：`shared/schema/local-commands.json` 的 Windows 命令面，`docs/USER_GUIDE.zh-CN.md`，`docs/features/history-preview.md`、`keyboard-history-retrieval.md`、`onboarding.md`，以及 `windows/src/preview/renderers/` 的文本、复合文档、PDF、DOCX、Markdown 实现。未执行 Slint 原型或 Windows 原生测试。

## 10. 完全不用 WebView 的 PDF / DOCX 预览候选

针对用户进一步提出的无 WebView 方向，补充以下方案。原生文档引擎可以与 Slint、WPF 分别组合；不需要为预览能力强行选择某一种主 UI。

| 引擎/路线 | 能力与适配 | 资源与兼容边界 |
|---|---|---|
| Windows.Data.Pdf | 系统 PDF 页面渲染，可输出图像交给 Slint | 增量部署依赖少；该 API 不提供完整文字提取/搜索/选择链路，不能等价替换当前 PDF.js；实际内存仍需测 |
| PDFium + Rust FFI / pdfium-render | 独立 PDF 引擎，不要求启动 Chromium/WebView；有页面渲染、文本提取、字符坐标、搜索等接口 | 推荐作为完整 PDF 预览候选；选择高亮、命中测试、缩略图和滚动仍需 UI 实现。绑定库不包含 PDFium 本体，需固定二进制/API 版本和第三方许可 |
| MuPDF | 原生 PDF 引擎；已核对 1.28.5 tag 中的 Office handler，包含 DOCX/XLSX/PPTX | Office 路径转换为内部 HTML 后由 MuPDF 自有排版器重排，不需要浏览器；不等于 Word/PowerPoint/Excel 原版式兼容。AGPL 或商业许可 |
| 自建受限 DOCX 阅读器 | ZIP/XML → 段落、文字样式、列表、图片、表格 → 原生阅读视图；可评估复用项目 DisplayDocument | 有控制功能范围与资源的机会，但解析、排版、选择都需要实现；不承诺分页、浮动对象、页眉页脚、域、公式和复杂图形一致。这是需明确接受的功能取舍 |
| Aspose.Words for C++ 等商业 Word 引擎 | 无需 Office 或 WebView，DOCX 可转换为 PDF/图片；之后复用 PDF 预览 | 更适合要求较高版式兼容的方向，仍受字体和具体 Word 特性影响；收费、包体、转换峰值和首屏时延需测，不能直接贴上“轻量”标签 |
| Windows IPreviewHandler | 宿主调用已注册的文档预览处理器 | 依赖用户安装的软件及处理器，资源、样式和内部实现不可控；不能保证任何设备都有 DOCX 处理器，也不能保证第三方处理器内部绝不使用 WebView，因此不作为严格无 WebView 的统一主方案 |

**进一步建议：PDF 优先 PDFium；希望同一引擎兼顾 PDF 与 DOCX 阅读时，新增 MuPDF 作为重点对照。** 是否选 MuPDF，先用真实 DOCX 夹具比较当前 docx-preview 的段落、表格、图片、中文字体、分页和文字选择表现，再评估许可与资源。发现 Office handler 不等于证明已满足 TailSync 当前全部预览能力。

如 DOCX 必须尽量保留 Word 版式，评估商业引擎或按需 LibreOffice 转 PDF；后者有办公套件依赖和启动/转换成本，不作为轻量化首选。将 DOCX 转为 PDF 不能消除排版引擎的成本。一般 XML 解析库或格式转换工具也不等同于 Word 排版器。

共同资源策略：预览 worker 按需启动，关闭释放；只渲染可见页和少量邻页，缩略图使用低分辨率与独立字节预算；搜索分批执行，切换文档取消旧工作。1200×1700 的 32 位位图单份约 7.8 MiB，宽高各翻倍后约 31.1 MiB，尚未计 GPU 纹理、解码缓冲与文档模型，因此仅限制缓存页数不够。DOCX 引擎仍可能需要整文档解析或分页，不能把按页显示说成整个引擎按页占用。

PDFium 官方 C API 明确不具备线程安全性，集成时使用专用串行 worker 或相应同步，不能随意并行调用；独立进程本身也不等于具有浏览器的完整沙箱。继续遵守项目的明文预览生命周期、资源限制和联网策略。裁剪 PDF JavaScript/XFA 必须先核对现有支持范围，不能为了降资源静默删除已支持功能。

官方依据（本轮读取）：

- [Windows.Data.Pdf](https://learn.microsoft.com/en-us/uwp/api/windows.data.pdf?view=winrt-26100)：页面转图像 API。
- [PDFium 文本 API](https://github.com/chromium/pdfium/blob/main/public/fpdf_text.h)、[渲染与线程约束](https://github.com/chromium/pdfium/blob/main/public/fpdfview.h)、[许可](https://github.com/chromium/pdfium/blob/main/LICENSE)。
- [pdfium-render](https://github.com/ajrcarey/pdfium-render)：Rust 封装及独立 PDFium 部署说明。
- [MuPDF 1.28.5 Office handler](https://github.com/ArtifexSoftware/mupdf/blob/1.28.5/source/html/office.c)、[重排文档实现](https://github.com/ArtifexSoftware/mupdf/blob/1.28.5/source/html/html-doc.c)、[该版本许可说明](https://github.com/ArtifexSoftware/mupdf/blob/1.28.5/README)。
- [Aspose.Words C++ 转 PDF](https://docs.aspose.com/words/cpp/convert-a-document-to-pdf/)、[授权](https://docs.aspose.com/words/cpp/licensing/)。
- [Windows Preview Handler 架构](https://learn.microsoft.com/en-us/windows/win32/shell/preview-handlers)。

本轮为接口与源码核对，尚未构建这些引擎，也未测量 TailSync 文档的渲染兼容性、包体或内存。

## 11. 预览 Demo 实现入口

后续已创建 `windows/preview-demos/`，包含 PDFium、MuPDF.NET 和 Windows.Data.Pdf 三个独立 Windows x64 demo，共用 WPF 测试窗口及自动对比流程。详见该目录 README 的能力、指标与验收状态。这一组用于分离文档引擎差异，不能当作 Slint 与 WPF 的性能比较。

实际包与此前 C 核心方案的区别：MuPDF demo 固定 `MuPDF.NET 3.28.2.4` / `MuPDF.NativeAssets 1.28.2`，其 NuGet 包携带 **Artifex Community License（非商业用途，商业另行授权）**，不能按 MuPDF C 核心的 AGPL/商业选择直接描述该封装。这也不是已决定采用的正式生产依赖。Windows 控件、DOCX 实际渲染和性能仍待实机验证。
