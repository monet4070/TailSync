# TailSync WPF Windows 前端

**C# / WPF / .NET 10 + 原有 Rust 同步后台**。已合入 `codex/clipboard-onboarding-fixes-2026-10-31@35c7234`，保留此前按 main 迁移的原生页面与界面优化。生产模式通过仅当前用户可访问、会话认证的 Windows 命名管道调用真实后台，后台不创建 WebView。

默认启动为生产模式。`--demo`、showcase、smoke 和 quality 是独立示例模式；`--migration-check-dir` 与 `--migration-showcase-dir` 使用真实 Rust 加密数据库及专用测试目录，禁用系统剪贴板写入、网络、通知、注册表和快捷键注册，不会冒充正式同步成功。

## 2026-10-31 分支集成

- 共享核心保留已完成的图片下载，执行累计字节与像素预算，并支持系统代理；WPF 提供「自动保存网络图片」开关。
- 历史窗口默认聚焦条目：`1–6` 取用并在完整恢复后关闭，`←/→` 翻页，`F` 搜索，搜索内 `↓` 返回条目。编号按实际展开后的行计算；查询、翻页和写入期间拦截旧操作，失败保留原页与选择。部分恢复显示缺图反馈；复合内容恢复携带原始记录与内容身份。
- 原生 15 步中英文引导使用 `shared/onboarding` 的内容与样本。历史、收藏、预览和模拟配对不写入真实数据、剪贴板或网络；切换前后清除旧查询与预览。引导结束后可从设置重开，永久关闭使用本次安装的 Rust 标记并移除入口。初次普通启动自动引导，配对链接和测试参数优先。
- 样本图片、SVG、Markdown、PDF、Word 走原生预览，PowerPoint 与富文档样本使用共享结构化文档。引导中主题选择是内存预览，结束后恢复已保存主题。
- 最小窗口中压缩引导卡片，保留历史条目与导航；完整步骤说明可从标题提示查看，永久关闭放入 `⋯` 菜单。
- 生产构建默认开启复合剪贴板，必须携带匹配架构的 helper，校验协议并验证发布目录中的实际解析路径。ARM64 交叉构建检查 PE 架构，运行检查需在 ARM64 Windows 完成。

构建和验证细节见 [`../../../docs/WPF-CLIPBOARD-ONBOARDING-2026-10-31.md`](../../../docs/WPF-CLIPBOARD-ONBOARDING-2026-10-31.md)。

## 已迁移

- 历史/收藏：30 条分页、250ms 搜索去抖与请求取消、按后台能力显示的 6 类内容筛选、日期范围、日期/文件批次分组、展开折叠、缩略图有界缓存、收藏保护、长按收藏、右键删除、清空时保留收藏；上下键、Enter 恢复、空格预览、Ctrl+F、Ctrl+,；恢复操作全进程互斥，错误与同步警告按实际状态展示。
- 独立原生预览：文本选择/搜索/字号/换行、虚拟化代码高亮、Markdown 与表格、图片缩放旋转、SVG 源码及 HTTPS 图片逐条目授权、PDF 分页画面与独立可选择文本、DOCX 段落/表格/图片/链接；批次导航与窗口所有者生命周期。
- 设置：同步、开机启动、全局快捷键录制/冲突回滚/重置/清除、通知、进度条、历史上限、存储配额、真实存储迁移及旧数据清理、语言切换、更新检查。
- 连接：真实本机/设备身份与路由、自动/LAN/Iroh/Tailscale 模式、路由测试、设备启停与撤销配对、双端验证码状态机、远程邀请创建/检查/复制/取消、冷启动与重复启动邀请链接。
- Theme V2：五个内置主题、浅色/深色/跟随系统/高对比、按主题字体角色及 Windows 回退链渲染；主题包四模式验证、预览、安装、更新、降级确认、回滚、删除和三类图片槽。
- Windows 外观：右上角最小化/最大化/关闭、4px 可见滑块及 10px 命中区域、像素平滑滚动及虚拟化、最小窗口宽度适配。主题字体使用系统已安装字体，不分发 macOS 字体。

历史列表正文和搜索分别使用主题的 history/search 排版。内置主题通过 `typography.history.useDisplayFont: false` 使用 reading 字体，标题保留 display 字体；Canvas 正文为 Noto Serif SC / Source Han Serif SC 等宋体候选、16px，其他主题正文 15px（Mono 为 14px）；搜索 16px（Mono 为 15px），界面文字 14px。字体由系统安装情况按主题回退，不分发字体文件。自定义主题可显式选择正文角色；旧主题缺少该字段时保留历史行为。文字及滚动内容表面显式启用 ClearType 提示，静止文字使用固定 hinting；滚动偏移在结束、键盘跳转及容器重排后对齐物理像素。日期标题不包含在行选中边框中。缩略图依据 Core `display` 的资格与缓存键加载，支持普通图片、复合图片和图文记录。

Canvas 界面字体优先 Noto Sans SC，正文和设置标题消费主题 history/panel 组件的 500 字重；说明、分类和历史元数据默认 13px，派生自 UI 字号。文件大小使用 secondary 色以保持小字对比度。详见 `docs/WPF-TEXT-CLARITY-2026-10-30.md`。这些变化不修改系统字体平滑、显示缩放或其他内置主题的字体族。

`--thumbnail-check-dir=<目录>` 使用独立加密测试库验证缩略图的完整加载链路；同时传入 `--thumbnail-review` 可在检查后保留一个明确标注的样例窗口用于鼠标/键盘验收。测试目录需包含匹配的 Backend 与 `tailsync-clipboard-helper.exe`。

历史缩略图按实际图片边缘裁出主题圆角，未选中的筛选框也保留底色与边界。
取消收藏具有反向填充和星标淡出反馈，托盘导航在 Rust 与 WPF 之间移交前台权限。
验证入口与细节见 `docs/WPF-HISTORY-DETAILS-2026-10-30.md`。

调整窗口大小时使用原生客户区完整重绘，避免旧尺寸画面与新布局交叠；进入
拖动时停止尚未结束的滚动惯性和窗口入场位移。验证入口为
`--resize-check-dir=<目录>`，附加 `--resize-review` 可交互检查合成窗口。
详见 `docs/WPF-RESIZE-REDRAW-2026-10-30.md`。

## 构建与运行

需要 Windows、.NET 10 SDK、Rust MSVC 工具链。以下从 `windows/native-ui` 执行：

```powershell
# 同时构建 Rust 后台和 WPF，打开真实历史窗口
.\build-wpf.ps1 -Run
# 其他窗口
.\build-wpf.ps1 -Run -Page settings
# 完全独立的演示模式
.\build-wpf.ps1 -Run -Demo
# 仅运行示例控件检查，无生产后台
.\build-wpf.ps1 -SmokeTest
# 包含 .NET、Rust 后台、原生预览库及许可证的便携目录
.\build-wpf.ps1 -Publish
# 复用已构建的匹配架构 Rust 后台
.\build-wpf.ps1 -Publish -BackendExecutable F:\build\release\tailsync.exe -ClipboardHelperExecutable F:\build\release\tailsync-clipboard-helper.exe
# 指定本地 SDK、发布目录
.\build-wpf.ps1 -Publish -DotnetExecutable F:\dotnet\dotnet.exe -PublishDirectory F:\TailSync-WPF
```

便携目录运行 `TailSync.Wpf.exe`，保留旁边的 `TailSync.Backend.exe`、运行库和 DLL。源码模式可设置 `TAILSYNC_WPF_BACKEND_EXE` 指向真实后台。`--window=history|favorites|settings|connections` 选择页面；`--background` 保持后台与托盘。

已开启的旧版 TailSync 开机启动项会迁移为 WPF 宿主，`tailsync://` 用户协议也指向 WPF。一个会话只保留一个 WPF 宿主；Rust 原有单实例机制继续避免两个正式后台同时运行。

## 验证

2026-10-30 本机 Windows 11 x64：Release 与自包含发布无警告/错误；19 项 Core 检查、12 项基础 WPF 检查、147 项布局/滚动断言、127 项自包含 WPF→真实隔离 Rust 集成断言通过。后端另外通过 143 项隔离 IPC 检查、3 项生命周期检查和 6 项桥接单元测试。不同检查有重叠，不应相加为独立功能数量。

集成检查覆盖真实加密 DB 的 30/30/5 分页、收藏保护、清空保留收藏、TSPV 原始二进制预览、主题包安装/更新/回滚/删除、主题字体/图片、设置重启持久化、稳定错误本地化及标题栏语言。证据在 `windows/ui-benchmark/results/wpf-main-migration/`；详细功能对照见 `docs/WPF-MIGRATION-2026-10-30.md`。

```powershell
# 真实后台、全新测试目录；不会使用正式数据库或注册表
.\build-wpf.ps1 -BackendExecutable F:\build\release\tailsync.exe -MigrationCheckDirectory F:\wpf-checks
# 已发布程序可以直接执行
.\TailSync.Wpf.exe --migration-check-dir=F:\wpf-checks
# 留下可操作的真实后台示例窗口，三窗口全部关闭后退出
.\TailSync.Wpf.exe --migration-showcase-dir=F:\wpf-showcase
```

## 尚需验收与发行边界

- 自动更新保留检查。现有发行包仅包含 Tauri 前端，WPF 明确拒绝安装它；需配置包含 WPF 宿主与后台的签名更新包后启用自动安装。
- PDF 的可选择文本为独立视图，非图像上精确对齐文字层；DOCX 是原生重排，复杂分页/页眉脚注/浮动布局需实际文件验收。
- SVG 仅授权 HTTPS 栅格图片；外链 CSS、字体和 use 引用不加载。公网授权下载正路径尚未实网验收。
- 双设备配对与同步、真实全局快捷键/剪贴板恢复、存储迁移、系统通知、IME、触摸板、多屏 DPI 和长时运行仍需真实使用环境验收。隔离测试不替代这些验收。
- WPF 构建脚本已启用复合剪贴板并强制携带 helper；跨设备富文本与真实系统剪贴板发布仍需目标设备验收。
- 旧 WPF UI 原型的 85 MiB 内存基准不代表接入真实后台后的总占用。此次迁移没有重新跑五技术栈性能矩阵。

第三方组件与许可证见 `THIRD-PARTY-NOTICES.md`、`Licenses/`；依赖锁定在 `packages.lock.json`。
