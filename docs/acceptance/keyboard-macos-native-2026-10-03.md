# 键盘取用历史的 macOS 真机验收记录（2026-10-03）

> 目的：记录键盘取用历史功能在**真实 macOS 桌面会话**中的首次人工验收结果。
> 为什么需要这份记录：`docs/features/keyboard-history-retrieval.md` §8 复盘了上一版失败的原因——**当时所有自动化测试都是绿的，但真机上按 ↑↓ 毫无反应**。因此本功能的验收门禁不能由自动化测试单独满足，必须有一次真人操作。此前该结论只存在于会话记录中，本文件把它固化为可引用的证据。

## 背景：自动化测试无法覆盖这一层

本次同时确认了一个工具链限制，它使"用自动化测试替代真机"这条路走不通：

- `TAILSYNC_GUI_FOCUS_TESTS=1` 门控的 `testNativeKeyOwnershipAcrossPassivePresentationFocusAndEscape` **在 `swift test` 下必然失败**，且失败在它自己的前置断言（测试文件第 35 行 `The native test host must acquire key status`），从未进入产品代码。
- 实测原因：`swift test` 进程不是 `.app` bundle，macOS 的 WindowServer 拒绝让它成为前台应用。`NSApp.setActivationPolicy(.regular)` 返回成功，但 `NSApp.isActive` 恒为 `false`，窗口因此永远拿不到 key 状态。
- 结论：该 opt-in 测试需要**由真实 `.app` 承载的 GUI 测试宿主**才能运行；在当前仓库的 `swift test` 流程下它只能报红。相关说明已写入测试文件 `macos/swift-ui/Tests/TailSyncTests/HistoryPreviewKeyboardWindowTests.swift` 的 `showHost` 注释。

因此，"热键唤起后窗口是否真的拿到键盘"这一问题**只能由真人操作回答**，下列结果即为该回答。

## 本次确认的范围

| 项 | 内容 | 结果 |
|---|---|---|
| 唤起 | 全局热键唤起历史窗口 | ✅ 通过 |
| 首次 ↓ | 按一次 ↓，焦点从搜索框移交列表 | ✅ 通过 |
| 选中可见性 | 第 0 条出现可辨识的选中框，**未被跳过** | ✅ 通过 |

操作者：项目所有者。环境：真实 macOS 桌面会话，非自动化测试宿主。

## 明确**不在**本次确认范围内的项

以下条目仍为 `待验`，不得由本记录推断为通过：

- **T-01** Enter 恢复 → 关窗 → 焦点回原应用 → ⌘V 粘贴
- **T-02** 中文输入法组词中按回车不误触发恢复
- **T-03** 搜索框内空格作为文本输入，不弹出预览
- **T-05** 预览打开后连续 ↑↓ 时预览实时跟随且不夺焦
- **T-06** 预览打开时按 Enter 同时关闭历史与预览
- **T-07** 页底继续 ↓ 自动翻页并选中新页首项
- **T-08** 文件历史恢复后跨应用粘贴完好
- **T-09** 恢复失败时窗口保留、选择与搜索词不丢
- **T-10** ⌘Enter 恢复并保持窗口打开
- **K-1/K-2/K-3** Tab 深读、预览 Esc 回宿主、Esc 分层
- VoiceOver 无障碍朗读（见 `voiceover-connections-checklist.md`，需人工逐项核对）

## 与自动化证据的关系

| 证据层 | 覆盖 | 不能替代什么 |
|---|---|---|
| L2 既有单测（37 项键盘测试，36 通过 + 1 跳过） | 键位映射、焦点状态机、真实 responder chain（`HistoryKeyboardIntegrationTests` 驱动真实 field editor） | 系统是否把键盘交给该窗口 |
| L4 本记录 | 热键唤起 → ↓ → 选中可见 | 上表所有未列项 |

## 重新打开条件

任一情况出现时本记录失效，需重新人工验收：

- 历史窗口的唤起路径改变（`GlobalShortcutController` / `HistoryWindowController.present` / `onKeyboardActivation`）；
- 搜索框控件替换（`HistoryNativeSearchField` / `HistorySearchNSTextField`）或其 `doCommandBy:` 委派改变；
- 历史行选中样式的可见性改变（`HistoryRowFocusStyle`）；
- 出现任何"按键无反应"的用户报告。
