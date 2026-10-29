# TailSync WPF 备选界面

独立的 **C# / WPF / .NET 10** 工程，不依赖 WebView2、WinUI 或 Windows App SDK。保留并复用 `TailSync.Native.Core` 的示例客户端、视图状态、收藏保护和 Mac 界面清单。现有 WinUI 工程保持独立。

当前是 UI 候选，尚未接入真实 Rust 后台。全部内容为内置示例，恢复、配对、开机启动、更新和存储操作均不会修改真实系统数据。

## 包含的窗口

- 历史、收藏：400×600；搜索、分类、七种日期选项、自定义范围、置顶、上下键、Enter 恢复演示、空格预览、右键菜单、条件格式菜单与锚定详情浮层。
- 设置：540×500；通用、更新、历史记录、存储、连接与设备、外观六组；快捷键键帽与本窗口录制、重置/清除、10–500 历史上限、1–16384 GiB 配额、第三方图片折叠项、主题与语言入口。
- 连接：520×640；连接方式分段、本机身份、示例设备、配对验证码、按连接方式显示的远程配对折叠项。
- 独立预览：640×560；64px 内容头部、文字工具栏、文字选择与搜索、字号和换行；图片缩放和适应窗口。

视觉继续以用户提供的 Mac 截图及 SwiftUI 源码为参考。WPF 控件模板覆盖按钮、开关、搜索、下拉菜单、分段、滑块、列表选中边框和弹窗，不依赖 WPF 的默认系统皮肤。字体使用已安装字体回退，不附带 macOS 字体或 SF Symbols。没有声称已达到逐像素一致。

五种内置主题的浅色/深色共 240 个颜色值从现有 `shared/tailsync-themes/src/builtins.rs` 提取；修改主题时请重新生成：

```sh
node windows/native-ui/Design/export-wpf-themes.mjs
node windows/native-ui/Design/export-wpf-themes.mjs --check
```

当前已实现颜色资源和展示字体切换；完整 Theme V2 组件/主题包安装、所有主题密度与形状尚未完整移植。

## Windows 运行

便携包 `artifacts/wpf-ui-preview/TailSync-WPF-preview-win-x64.zip` 解压后直接运行 `TailSync.Wpf.exe`，内含 .NET 运行时。不要只复制 exe，其他 DLL 和运行时文件需保留。

也可以从源码在 Windows 安装 .NET 10 SDK 后运行：

```powershell
# 从 windows/native-ui 执行
.\build-wpf.ps1 -Run
.\build-wpf.ps1 -Run -Page settings
.\build-wpf.ps1 -Run -Page connections
.\build-wpf.ps1 -Run -Page favorites
# ARM64 Windows 构建
.\build-wpf.ps1 -Architecture ARM64 -Run
# 真正运行 WPF 控件和模板的冒烟检查（会打开并关闭测试窗口）
.\build-wpf.ps1 -SmokeTest
# 发布包含 .NET 的便携文件夹
.\build-wpf.ps1 -Publish
```

窗口标题栏右键可打开其他窗口；Ctrl+, 打开设置；历史/收藏窗口的 Ctrl+F 聚焦搜索。命令行也支持 `TailSync.Wpf.exe --window=settings`。快捷键录制只影响当前原型窗口，没有注册全局快捷键。

`build-wpf.ps1 -SmokeTest` 生成 `wpf-smoke-results.json`，覆盖实际 WPF 绑定、模板、过滤、空结果、收藏、设置组、主题资源和独立预览。该检查必须在 Windows 执行；macOS 上的编译不能代替它。

## 验证与边界

已在当前 macOS 主机完成 WPF Release 构建，包括 XAML→BAML 的标记编译和 C# 编译；可跨平台执行的共用 Core 检查另行运行。没有借助 WinUI 的临时 XAML 字段桩来代替 WPF 编译。

Windows 控件实例化冒烟检查、中文输入、窗口 DPI/圆角、Narrator、实际内存与唤起时延仍待 Windows 实测。浏览器中的“WPF 备选”是共享设计参考，明确标记为目标外观，不能当作 WPF 的运行截图或性能证据。

托盘、单实例、真实 Rust 通信、系统剪贴板恢复、全局快捷键、生产同步、PDF/DOCX 等专用渲染与引导高亮尚未接入。数据只在本进程内演示，退出后还原；不会迁移或覆盖正式客户端。

## 资源策略

历史 ListBox 使用 `VirtualizingStackPanel`、`Recycling` 和有界滚动区，避免为了显示历史而构建所有条目控件。窗口与定时器、事件订阅在关闭时释放。共享业务逻辑只保留一份，没有额外复制数据库或业务规则。

这些是实现选择，**不是已测得的性能提升**。与 WinUI/WebView2 比较时应接入等价用例，用相同数据量测量整个进程树的私有提交、驻留内存及快捷键唤起延迟；不要比较本示例与正式客户端后宣称改善百分比。
