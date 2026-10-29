# TailSync · Windows 原生界面原型

另有独立 **WPF 备选工程**：[`TailSync.Wpf/README.md`](TailSync.Wpf/README.md)，使用 `build-wpf.ps1` 构建运行。下文仍描述原有 WinUI 工程；两版保留作同功能对照。

独立 **C# / XAML / WinUI 3** 工程，保留现有 Rust 后台的接入边界。当前修订以 Mac SwiftUI 源码和用户提供的两张 2× 截图为视觉参照，采用相同的独立窗口结构；原型没有接入 WebView2。

本工程使用内置示例数据。恢复、配对、更新、目录选择等均为演示；不会操作真实历史、系统剪贴板、开机启动、密钥或网络服务。上方的历史/收藏/设置/连接选项仅用于浏览器评审，不属于产品窗口。

## 本次修订

- 历史/收藏 **400×600**、设置 **540×500**、连接 **520×640** 逻辑像素；恢复左侧三个窗口控制按钮。
- 历史恢复 36px 搜索、28px 筛选、8px 间距、原有透明填色叠加；收藏隐藏置顶/收藏入口，收藏标记移回来源旁边。
- 内容详情使用锚定浮层；格式菜单只对符合条件的复合内容显示。删除、收藏和预览移入右键菜单，恢复完整日期选项和自定义范围。
- 设置恢复通用、更新、历史记录、存储、连接与设备、外观六组。快捷键有独立键帽/编辑/重置/清除，历史上限为 10–500 的滑块，存储配额为 1–16384 GiB；补齐第三方图片折叠项、外观分段、五张主题卡片和语言选择。
- 连接恢复单张分组卡片中的连接方式、本机身份、设备、配对入口、默认折叠的远程邀请。预览恢复 64px 内容头部及 56px 工具栏；配对使用盾牌、设备名、验证码和核对操作。
- 删除此前添加的计数、示例标签和产品页脚窗口菜单。

## 查看和运行

浏览器预览可在 macOS 上检查布局和示例交互；**它不是 WinUI 运行截图**。

Windows 安装 .NET 10 SDK、Windows SDK 10.0.26100 和 Visual Studio Windows 构建工具后执行：

```powershell
.\build.ps1 -Run
.\build.ps1 -Run -Page settings
.\build.ps1 -Run -Page connections
# ARM64:
.\build.ps1 -Architecture ARM64 -Run
```

固定依赖为 WindowsAppSDK `1.8.260921001`、SDK BuildTools `10.0.26100.6584` 和 CsWinRT `2.3.1`。unpackaged、WindowsAppSDKSelfContained；最低目标系统 10.0.19041。输出不要覆盖日常使用的 TailSync 安装。

## 工程结构

```text
Design/sync-mac-design.mjs              提取 Mac 文案/主题色并记录参考源码摘要
Design/mac-ui-manifest.generated.json   原生工程与评审预览共同消费的界面清单
TailSync.Native/                        App、历史 XAML、MacPages 设置/连接构建器
TailSync.Native.Core/                   示例客户端、查询、条目、视图状态、设计清单
TailSync.Native.Checks/                 跨平台原生状态检查
DemoData/history.json                  共用的安全示例数据
preview/template.html                  交互评审预览源文件
preview/prepare-preview.mjs             将共用文案、主题、示例数据注入预览
tests/preview.test.mjs                  日期、收藏、菜单、设置、预览等交互检查
```

设计清单属于展示层，未新增生产 IPC 契约。`ITailSyncClient` 仍仅由 `DemoTailSyncClient` 实现。后续后台适配应消费现有生成契约和 Rust 用例，保留收藏删除保护、取消和错误语义。

## 验证与尚未完成的对齐

- 19 项跨平台原生状态检查、8 组预览交互检查通过；设计来源摘要和 XML 结构另行检查。
- 使用实际 WinUI/Windows SDK 引用编译 C# 源码和事件签名；临时 XAML 命名字段桩不证明 XAML 能编译或渲染。
- macOS 无法运行 Windows 的 `XamlCompiler.exe`，尚未生成并运行本次修订的 Windows 包。窗口拖动区、DPI、边框、最小尺寸、字体、键盘、辅助功能及内存仍需 Windows 实机检查。
- **未达到经验证的逐像素一致。** Windows 字体回退、Segoe 图标与 SF Symbols、系统选择菜单/日期控件/ContentDialog 仍有平台差异；WinUI 主题卡片和连接子行还需实机对照。主题卡片选择是展示状态，尚未实现完整 Theme V2 渲染器。
- 图片/PDF/DOCX/SVG 各类专用预览工具、批次展开、托盘、引导高亮、生产错误/加载状态和真实后台能力尚未完整移植。当前“新手引导”只提供入口演示。

不要把源码几何/文案对齐当成全部界面已经一比一验收。两张参考截图之外的界面，当前以 SwiftUI 结构作为对照，仍需原版实际截图逐屏验收。

```sh
node windows/native-ui/Design/sync-mac-design.mjs --check
dotnet run --project windows/native-ui/TailSync.Native.Checks -c Release
node windows/native-ui/preview/prepare-preview.mjs
node --test windows/native-ui/tests/preview.test.mjs
```
