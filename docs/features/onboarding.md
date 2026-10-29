# 新手引导

适用：macOS SwiftUI 与 Windows React/Tauri。内容版本 3。

## 用户流程

引导直接叠加在应用的真实窗口上：深色蒙版留出一个可操作的高亮区域，底部显示当前操作说明、进度、上一步、下一步和结束按钮。它不新增独立介绍窗口，也不复制连接表单、历史列表或设置界面。

每个安装只自动展示一次；用户完成或主动结束后，后续启动不再弹出。待处理配对链接优先进入配对流程，显式测试启动参数保留原行为。各步骤依次定位：

| 步骤 | 实际窗口 | 高亮目标与操作 |
|---|---|---|
| 1–4 | 连接与设备 | 自动连接模式、单端允许配对、虚拟待配对设备、远程邀请面板 |
| 5–10 | 历史 | 演示记录概览、双击回溯与右键删除、长按收藏、搜索筛选、空格预览、收藏入口 |
| 11–15 | 设置 | 同步与快捷键、历史保留、存储、外观与主题、更新 |

「下一步」会滚动到目标控件，必要时打开对应的真实窗口。高亮区域可以点击、输入和滚动。展开面板、调整窗口尺寸或滚动时，高亮跟随控件；较长设置分区只突出当前可操作的可见部分，可以继续滚动。说明卡在底部占用独立布局空间，高亮只裁剪到真实可见窗口或滚动区域；小窗口内说明文字可滚动，导航按钮始终可见。配对等模态对话框优先交互，关闭后恢复引导。引导期间，历史窗口使用内置演示记录，连接窗口加入一台标明的虚拟待配对设备；演示覆盖文字、网站、代码、路径、图片、Markdown、SVG、PDF、Word、PowerPoint、富文档、未知格式和文件批次。演示内容不写入数据库、剪贴板、网络或信任状态。没有记录、目标控件尚未加载或当前连接模式不支持远程邀请时，仍可继续下一步；无高亮目标时保留底层重试操作。

配对只需一台设备开启「允许配对」，另一台直接在设备列表点击「配对」，之后双方核对并确认验证码。设置顶部可在永久关闭前重新启动；永久关闭后不保留该入口。

卡片中的「不再显示（永久）」会永久隐藏引导，并同时移除设置页入口；只有重新安装应用后才可能重新出现。关闭按钮「结束引导」与最后一步「完成」只结束本轮，设置顶部仍可从第一步重新开始。菜单栏和系统托盘不提供引导入口。文案跟随应用语言，支持中文和英文。

## 模块与状态

- `shared/onboarding/guide.json` 是窗口、控件标识和中英文内容的共同来源；`generate-swift.mjs --check` 已加入漂移检查。
- Windows `GuidedTour.tsx` 观察实际 DOM 锚点，绘制可点击穿透的 SVG 镂空与提示卡；`tourState.ts` 处理进度、跨窗口通知、操作失败回退。
- Windows 命令通过 `tailsyncClient.ts` 进入现有 Tauri 窗口 API；`onboarding.rs` 只负责首启入口和本机完成标记，不增加生产 HTML 入口。
- macOS `OnboardingGuide.swift` 管理共享内容和本机进度；`OnboardingView.swift` 用 SwiftUI anchor preferences 定位控件，保留子级锚点的 transformAnchorPreference 定位收藏等嵌套控件，四块遮罩保留中间真实控件的交互，通过现有 AppDelegate 打开窗口。
- Windows 进度保存在 `tailsync-guided-tour-v2`，完成标记为数据目录中的 `onboarding-v2.dismissed`；macOS 为隔离数据目录中的 `onboarding-v2.json`。状态不随剪贴板同步，不扩展共享 settings 或网络协议。

损坏、缺失或越界进度回到第一步。跨窗口打开失败时恢复原步骤并显示重试提示；完成状态写入失败时保留引导。macOS 使用原子 JSON 写入，`suppressed` 状态独立于普通完成状态；Windows 使用数据目录中的 started/suppressed 标记。

永久关闭状态绑定本次安装：macOS 使用应用包目录的文件系统身份，内置更新成功后将偏好绑定到更新后的应用包；手动重新安装的新应用包从第一步开始。Windows NSIS 手动安装生成新的 `tailsync-install-id`，`/UPDATE` 保留原 ID，普通卸载移除 ID；便携版以所在目录的创建时间和路径识别安装。Windows 原生标记优先于可能残留的 WebView 缓存，重新安装后的启动入口重置旧进度。首次升级时接纳旧格式的关闭状态；重装不清除配对、历史或其他设置。

## 边界与验证

引导不会自动配对、复制示例文本、创建邀请或申请目录权限。演示设备的模拟配对、回溯、删除和收藏只改变引导内存；只有用户结束引导并操作真实控件时，应用才执行原有业务。完成引导不代表配对或同步成功；原件恢复、图片归档和来源操作仍受条目能力限制。

开发用 `windows/tour-preview.html` 复用真实 React 页面，以标明的示例数据替代 IPC，支持逐步查看全部引导。它不在 Vite 生产入口中，不连接设备，不读写真实剪贴板；实际配对、预览窗口等能力需在原生客户端验证。

验证命令：

```sh
node shared/onboarding/generate-swift.mjs --check
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
cd windows && npm test && npm run build && npm run lint
# 仓库根目录，使用可用 Apple 测试工具链：
node scripts/run-isolated-tests.mjs -- swift test --package-path macos/swift-ui --filter 'OnboardingTests|AppBehaviorTests'
```

自动化覆盖真实锚点、同窗口推进、跨窗口失败回退、新安装启动时清理旧缓存并从第一步开始、窗口间进度通知、模态让位与恢复、缺失目标、完成失败重试和中英文内容。Swift 另验证进度持久化、真实收藏按钮的嵌套锚点、可见边界裁剪与真实连接控件的蒙版渲染。浏览器验证点击原配对按钮、搜索输入、跨页面步骤和窗口尺寸布局；Windows WebView2 事件权限、完整客户端首启、VoiceOver 与双设备同步仍需目标系统验收。
