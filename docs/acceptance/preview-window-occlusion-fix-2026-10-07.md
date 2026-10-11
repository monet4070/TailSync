# 预览窗口打开后立即关闭：修复与验证（2026-10-07）

## 根因与修改

预览窗口居中打开，尺寸通常大于历史窗口。它完全遮住历史窗口时，AppKit 会发送遮挡状态通知；历史窗口虽然仍然打开，`occlusionState` 中的 `.visible` 会消失。

`TailSyncTransientWindowController.windowDidChangeOcclusionState` 原先把这个状态解释为窗口已隐藏，再通过历史或收藏窗口的可见性通知传给 `HistoryView`。`HistoryView` 收到 `visible=false` 后会立即关闭该集合的预览。因此预览刚打开就会关闭。此问题同时影响历史和收藏。

修复将判断改为 `window.isVisible && !window.isMiniaturized`：窗口仍然打开但被遮挡时保持预览；窗口真正隐藏或最小化时仍报告不可见。历史列表在打开但被遮挡时继续更新，使被动预览仍能跟随上下键选择。

产品代码只修改 `macos/swift-ui/Sources/TailSync/Services/TailSyncTransientWindowController.swift` 的这一处判断。

## 回归证据

新增 `HistoryKeyboardIntegrationTests.testCoveredHostKeepsPreviewOpenAndFollowingSelectionForBothCollections`，覆盖真实 `HistoryView`、AppKit 历史窗口、窗口控制器、可见性通知和预览控制器的调用链；历史数据由测试注入，API socket 指向不存在的测试路径。

测试重放遮挡通知，分别检查历史和收藏的预览保持打开、上下键选择仍使预览跟随，以及真正隐藏宿主窗口仍关闭预览。遮挡状态由测试窗口显式提供，因此这是可重复的通知链回归，不能代替用户桌面上的实际窗口遮挡和焦点验收。

| 验证 | 结果 |
|---|---|
| 修复前运行新增测试 | 失败：两个集合均关闭预览，预览跟随也失败，共 6 个断言失败 |
| 修复后窗口、键盘、预览专项 | 33 项：32 通过，1 项既有原生焦点测试跳过，0 失败 |
| 全量 Swift 测试，开启 WebKit SVG 验证 | 312 项：311 通过，1 项既有原生焦点测试跳过，0 失败 |
| Swift release 编译 | 通过 |
| 新包 arm64、ad-hoc 签名及严格校验 | 通过 |
| 当前候选包上的人工操作 | 待用户复查 |

跳过的是 `testNativeKeyOwnershipAcrossPassivePresentationFocusAndEscape`，需要可以获得前台焦点的 GUI 测试宿主；本次未新增跳过条件。原生 Windows、双设备、目标应用和发布门禁未由本次修复关闭。

## 修复包与证据位置

新包：`artifacts/clipboard-improvement/2026-10-07-preview-window-fix-r1/TailSync-PreviewFix.app`。

同目录保留修复前后日志、全量测试日志、release 编译日志、源码指纹和 `candidate-manifest.json`。指纹为 `2223c488cd9131fc429417e2bb5a206692d71982176587758d2164ec530597e6`。

与 S2-07 r2 封存源码相比，仅窗口控制器及上述测试文件发生变化。新包复用 S2-07 候选包中未变更的 Rust daemon 和 clipboard helper，替换本次 release 编译的 Swift 主程序后重新签名。既有封存记录仍只证明其原始源码，本次不修改或重新封存它们。

## 人工复查步骤

1. 从菜单退出当前旧版 TailSync，然后打开 `TailSync-PreviewFix.app`。
2. 在历史窗口选中一条记录，按空格预览，等待至少 3 秒；预览应保持打开。
3. 将预览移动到完全盖住历史窗口的位置，按上下键；被动预览应继续跟随选择。
4. 按 Escape 关闭预览；按 Tab 打开可进入阅读的预览，确认不会立即关闭。
5. 在收藏窗口重复上述预览操作；关闭另一个集合窗口不应关闭当前集合的预览。
6. 再检查关闭历史窗口、隐藏历史窗口和最小化/还原的既有行为。

本次构建使用独立 scratch 目录，打包及保存日志后清理该目录，保留修复包和证据。
