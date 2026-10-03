# 键盘取用历史流程 — 执行计划（整理版）

这份计划由用户粘贴的历史流程执行方案整理并落盘。下面按「目标 → 状态机 → 阶段任务 → 测试验收 → 文件清单 → 技术决策 → 验收矩阵 → 失败复盘」组织，保留原始交互契约。

---

## 0. 当前状态

- 本次实现位于 `fix/phase4b-storage-feedback` 工作分支；应用版本号已从 2.3.1 提升到 **2.3.2**（下一版 Community Release 的候选），实现变更正在提交。
- macOS 已实现原生 `NSTextField.doCommandBy:`、marked-text 门禁、首次 ↓ 焦点移交、稳定 ID 选中、跨页、Enter / ⌘Enter、防重复恢复与原应用焦点返回。
- macOS 独立预览支持不夺焦浏览、选择防抖跟随、Tab 深读、预览 Enter / ⌘Enter、Escape 回宿主、关窗清理解密内容；预览按钮、历史键盘及文件批量恢复共享恢复锁与完成路径。
- **macOS 真机验收（2026-10-03）**：热键唤起 → 首次 ↓ → 第 0 条出现可辨识选中框，已由项目所有者在真实桌面会话确认通过。记录见 [`docs/acceptance/keyboard-macos-native-2026-10-03.md`](../acceptance/keyboard-macos-native-2026-10-03.md)。T-01 及其余条目仍未确认。
- Windows 按同一交互契约实现；真实 Windows 的前台 HWND 返回及输入法行为仍需目标机器验收。
- 取消反驳子代理与反驳环节，仅保留实现、测试与验收。
- 自动测试使用注入数据，不读写用户剪贴板；原生 key-window 的系统激活边界、真实中文输入法和跨应用粘贴不能仅靠单元测试确认。
- 本次不自动替换或重启正在运行的旧应用。

---

## 1. 目标与交互闭环

要做成一条**键盘优先的取用链路**，而不是给现有按钮零散补快捷键。核心闭环：

```text
1. 按历史全局快捷键
   → 唯一历史窗口激活
   → 光标进搜索框，第一项视觉预选
2. （可选）输入关键词
   → 输入法组词期间 ↑↓/空格/回车/Esc 完全交给输入法
3. 按 ↓
   → 第一次：控制权交给列表，仍选第一项
   → 之后：↑↓ 上下移动，跨页自动续接
4. （可选）按空格
   → 独立预览窗口轻量展示，不夺焦
   → 继续 ↑↓，预览内容实时跟随
5. 按 Enter（或 ⌘Enter / Ctrl+Enter）
   → Enter：恢复 → 关历史与预览 → 激活原应用 → 自己按 ⌘V
   → ⌘Enter：恢复 → 提示已恢复 → 历史窗口保留
6. 回到原应用按 ⌘V 粘贴
```

---

## 2. 焦点状态机

```text
                     ┌───────────────┐
                     │     Idle      │
                     └───────┬───────┘
                             │ 全局热键（快照原前台应用）
                             ▼
                ┌─────────────────────────┐
                │   Active: SearchFocus   │◄────────┐
                └────────────┬────────────┘         │
             第一次 ↓        │                      │ 在第 0 项按 ↑
                             ▼                      │ 或 ⌘F / Ctrl+F
                ┌─────────────────────────┐         │
                │    Active: ListFocus    ├─────────┘
                └──────┬────────────┬─────┘
              空格     │            │ Tab / 点击预览
                       ▼            ▼
      ┌────────────────────┐    ┌──────────────────────┐
      │ Previewing: Browse │    │   Previewing: Deep   │
      │ (焦点仍在历史窗口) │    │  (焦点转移至预览正文)│
      └────────┬───────────┘    └──────────┬───────────┘
               └─────────────┬─────────────┘
                             │ Enter / ⌘Enter
                             ▼
                    ┌─────────────────┐
                    │    Restoring    │  （防连击锁）
                    └────────┬────────┘
                    成功 │            │ 失败
                         ▼            ▼
          关窗 + 交还原应用    报错、保留选择与搜索词，可重试
```

**焦点归属总原则**：按键所属权跟随用户当下意图，而不是跟随窗口弹出动作。

---

## 3. 阶段划分与每阶段任务

### Phase 1 — 搜索框 IME 保护 + 单窗基础键盘流
- 输入法最高优先级：中文组词时 ↑↓/空格/Enter/Esc 一律放行给输入法。
- 搜索框 → 列表焦点移交：第一次 ↓ 不跳过第 0 项。
- ↑↓ 上下移动，支持长按连发。
- 跨分页：末端 ↓ 加载下一页并选首项；顶端 ↑ 回上一页并选末项；page 0 顶端 ↑ 或 ⌘F 回搜索框并全选。
- 搜索词变更、翻页、失败时保持选择按**稳定记录 ID**，不按行号。

### Phase 2 — 恢复闭环 + 原应用焦点归还
- 唤起前快照原前台应用（macOS `NSWorkspace.frontmostApplication`；Windows `GetForegroundWindow()`）。
- Enter / ⌘Enter（Ctrl+Enter）分流：关窗返回 vs. 保留窗口。
- 防连击锁；恢复失败不关窗、保留状态。
- Escape 分层：先关预览/弹窗，再退历史并交还焦点。

### Phase 3 — 独立预览轻量联动 + 深度阅读
- 预览以「不夺焦」方式展示（macOS `order(.above,)`；Windows `takeFocus: false`），历史窗口保持键盘控制。
- ↑↓ 切换记录时预览实时跟随（防抖，丢弃迟到响应）。
- Tab 进入深度阅读（焦点显式移交预览）；Esc 回历史窗口。
- 预览内 Enter 一并恢复并关闭。

### Phase 4 — 全场景验收
- 双端全量测试 + 真实流程走查（见第 7 节矩阵）。

---

## 4. 实现、测试与验收

按用户最新要求，取消反驳子代理与反驳环节。

- 实现：按阶段交互契约修改代码，保留已有功能。
- 测试：补充输入法、真实 responder、异步过期请求、防重复恢复与预览生命周期回归测试。
- 验收：运行构建与测试，并按第 7 节逐项进行真机验收；自动测试不能替代真实输入法与跨应用粘贴验证。

---

## 5. 双端文件改造清单

### macOS（SwiftUI / AppKit）

| 文件 | 职责 |
|---|---|
| `Services/GlobalShortcutController.swift` | 热键回调中、激活前快照前台应用 |
| `Services/HistoryWindowController.swift` | 会话生命周期、`dismissAndReturnToOriginatingApp()` |
| `Services/HistoryPreviewWindowController.swift` | `present(_:takeFocus:)` 非激活展示 |
| `Views/HistoryFilterBar.swift` | 搜索框 IME 保护、搜索→列表按键移交 |
| `Views/HistoryView.swift` | 列表键盘交互、跨页、Enter/⌘Enter、防抖 |
| `Views/HistoryRow.swift` | 选中高亮视觉 |
| `Tests/.../HistoryRowInteractionTests.swift` | 策略与行交互单测 |

### Windows（Tauri v2 / React / Rust）

| 文件 | 职责 |
|---|---|
| `src-tauri/src/commands.rs` | 热键回调捕获原前台 `HWND` |
| `src-tauri/src/window_lifecycle.rs` | 交还焦点（先交还再 hide，处理 `IsIconic`） |
| `src-tauri/src/preview_window.rs` | 非激活展示、关闭时归还焦点 |
| `src/pages/history/HistoryHeader.tsx` | 搜索框 IME 守卫、↓ 焦点移交 |
| `src/pages/History.tsx` | 键盘导航、跨页、Enter/Ctrl+Enter |
| `src/pages/Preview.tsx` | Enter 恢复并关闭、Esc 关闭 |
| `src/tailsyncClient.ts` | `takeFocus` 参数透传 |

---

## 6. 关键技术决策

**macOS**
- 输入法保护以 `hasMarkedText()` 为硬门禁。
- 滚动跟随用 `ScrollViewReader` + `.id(entry.id)`。
- 恢复前先 `orderOut` 再 `activate` 原应用；原应用已退出则不强行激活。
- ⚠️ **本次实机失败的关键点**：最初用全局 `NSEvent.addLocalMonitorForEvents` 承接方向键，同时又靠窗口标题/`isKeyWindow` 做窗口判定。这条路径在真实运行中未能可靠地把 ↓↑ 从搜索框夺走——表现为「焦点始终停在搜索框、按键像被搜索框吃掉」。**如果重做，应改为在搜索框自身的 `control(_:textView:doCommandBy:)` 里处理 `moveDown:/moveUp:/insertNewline:`（Spotlight 同款机制），而不是依赖全局监听器。**

**Windows**
- IME 保护：`isComposing || keyCode === 229`，并加 `compositionend` 后的短宽限窗。
- 前台锁：**先 `SetForegroundWindow` 再 `hide()`**；目标最小化时先 `ShowWindow(SW_RESTORE)`。
- 预览非激活：`takeFocus:false` 只 `show()` 不 `set_focus()`；窗口层级同步宿主 always-on-top。
- 僵尸预览：`History.tsx` 监听 `tailsync://preview-close` 清 `isPreviewOpen`；空选择时自动关闭预览。

---

## 7. 验收测试矩阵

| 编号 | 场景 | 预期 |
|---|---|---|
| T-01 | 原应用输入到一半，唤起历史按 Enter | 关窗、回原应用、⌘V 正确粘贴 |
| T-02 | 中文拼音组词中按回车选词 | 不误触发恢复、不关窗 |
| T-03 | 搜索含空格内容 | 空格正常输入，不误开预览 |
| T-04 | 第一次按 ↓ | 焦点进列表且**仍选第 0 项**；第二次 ↓ 才到第 1 项 |
| T-05 | 开预览后连续 ↑↓ | 预览不关、内容实时跟随、历史窗口保持控制 |
| T-06 | 预览打开时按 Enter | 恢复并同时关闭历史与预览 |
| T-07 | 当前页底部继续 ↓ | 自动翻页并选中新页首项 |
| T-08 | 恢复大文件/文件历史 | 关窗后原应用 ⌘V 粘贴文件完好 |
| T-09 | 模拟恢复失败 | 报错、窗口保留、选择与搜索词不丢 |
| T-10 | ⌘Enter（Ctrl+Enter） | 恢复并提示，窗口保持打开 |

---

## 8. 上次实机失败的复盘（重做时务必先解决）

**现象**：在 macOS 上按 ↑↓ 无任何可见反应；搜索框始终高亮、光标不动，感觉按键进了搜索框。

**当时未收敛的怀疑点**（三版迭代均未实测证实哪一条是真因）：
1. 全局 `NSEvent` 监听器可能根本没触发，或在 `event.window / isKeyWindow / 窗口标题匹配` 处提前 return。
2. 监听器触发了，但状态（`displayedIds` / `focusedId`）是陈旧或空的，策略直接返回 `.ignore`。
3. 选择其实变了，但**行高亮在主题配色下不可见**，用户看不出移动。
4. 搜索框的 `@FocusState` 从未被解除，焦点环一直留在搜索框，且后续字符继续进搜索框。

**教训与重做建议**：
- 不要用全局事件监听器去"抢"正在被 `NSTextField` 编辑的方向键；直接用 `doCommandBy:` 在搜索框内拦截，这是 macOS 的规范做法。
- 先做一个**可验证的最小闭环**（唤起 → ↓ 高亮明显移动 → Enter 恢复），确认这条链路在真机可用后，再往上叠预览、跨页等能力。
- 选择高亮必须与主题无关地**足够显眼**，避免"其实动了但看不出来"。
- 把"窗口是否 key、第一响应者是谁"变成可观测/可断言的东西，而不是靠推理。

---

## 9. 分阶段验收顺序

1. 先只做 macOS 单端最小闭环：搜索框 `doCommandBy` 拦截 ↑↓ + 显眼高亮 + Enter 恢复关窗。
2. 对已实现的跨页、空格预览、Tab 深读逐项进行真机验收；未完成真机验收前不视为发布门禁通过。
3. 在 Windows 机器上按同一交互契约验收双端一致性。
4. 每一步按第 4 节执行实现、测试与验收，以**真机可验证性**为优先。

---

## 10. 本次自动验证与交付

验证日志时间为 2026-10-03；这些结果不等于第 7 节真机验收全部通过。

| 检查 | 结果 |
|---|---|
| macOS Swift 全量测试 | 244 项，240 通过、4 跳过、0 失败 |
| macOS 键盘集成测试 | 5 项通过（首次 ↓、跨页、恢复锁/失败重试、查询过期保护） |
| macOS 预览键盘窗口测试 | 14 项，13 通过、1 原生 GUI 焦点测试跳过 |
| macOS Swift release build | 通过 |
| macOS 本地 bundle 构建及签名/部署目标检查 | 通过，arm64，最低 macOS 13.0，ad-hoc 签名 |
| Windows 前端测试 | 51 个文件、266 项全部通过 |
| Windows 前端生产构建 | 通过 |
| Windows 前端 lint | 0 errors、17 warnings |
| Windows 工程 Rust 宿主 cargo check | 通过（macOS 宿主，不是 Windows native target） |
| Windows 工程 Rust 宿主单元测试 | 124 通过、2 ignored、0 失败 |

macOS 的 4 项跳过包括 1 项系统 key-window 焦点测试及 3 项真实 WKWebView 测试。

> **更正（2026-10-03，实测）**：本表第 1 行的 `244 项` 是加入 §10.1 主题焦点回归测试之前的数字；加入后全量为 **248 项，244 通过、4 跳过、0 失败**，以 §10.1 为准。
>
> 关于被跳过的原生焦点测试：`TAILSYNC_GUI_FOCUS_TESTS=1` **在 `swift test` 下无法生效**。实测确认 `swift test` 进程不是 `.app` bundle，WindowServer 拒绝让其成为前台应用（`setActivationPolicy(.regular)` 返回成功但 `NSApp.isActive` 恒为 `false`），窗口永远拿不到 key 状态，测试在自身前置断言处失败、从未进入产品代码。该测试需要由真实 `.app` 承载的 GUI 测试宿主，当前仓库的 `swift test` 流程不具备此条件。因此"窗口是否真的拿到键盘"只能由真人验收回答，结果见 [`docs/acceptance/keyboard-macos-native-2026-10-03.md`](../acceptance/keyboard-macos-native-2026-10-03.md)。

Windows native target 尚未通过构建验证：此前交叉检查受缺少 Windows SDK/C 头文件 `assert.h` 限制。真实 WebView2 输入法、非激活预览、Win32 前台返回及文件剪贴板恢复仍需在 Windows 机器验证。双端真实中文组词、跨应用粘贴、大文件恢复仍保留为真机验收项。

本地 macOS 测试包：`/tmp/TailSync-Keyboard-2.3.1.app`（以及主题修正后的 `/tmp/TailSync-Keyboard-ThemeFix-2.3.1.app`）。两者保留 **2.3.1** 版本号，仅供验收，不是正式发布；没有自动打开、安装、替换或重启当时运行的旧应用。当前工作树已提升到 **2.3.2** 并准备提交为下一版 Community Release 候选，分支原有存储反馈改动保留。

本次日志：
- `/tmp/tailsync-keyboard-full-final.log`
- `/tmp/tailsync-keyboard-preview-final.log`
- `/tmp/tailsync-keyboard-release-final.log`
- `/tmp/tailsync-keyboard-bundle-final.log`
- `/tmp/tailsync-windows-tests-final.log`
- `/tmp/tailsync-windows-rust-final.log`

### 10.1 主题焦点适配修正（2026-10-03）

- 历史行键盘选中边框改用主题 `history.focus` 焦点色，缺省使用主题强调色；删除系统蓝色叠加框，圆角跟随主题，普通主题 2pt、高对比主题 3pt。
- 原生搜索 field editor 的插入光标、文字选中背景与前景显式跟随主题；编辑期间切换主题立即更新，不修改 marked text 或选区。失焦时恢复共享 field editor 的原始颜色，避免影响其他输入框。
- 新增 4 项回归测试，覆盖光标、文字选中/失焦恢复、组词期间主题切换及内置/自定义主题 token。键盘导航专项 18 项全部通过；macOS 全量 248 项，244 通过、4 跳过、0 失败。
- 新测试包：`/tmp/TailSync-Keyboard-ThemeFix-2.3.1.app`（仍为 2.3.1，arm64、最低 macOS 13.0、ad-hoc 签名）。构建及签名/部署目标检查通过，未打开、安装或覆盖旧测试包。实际 GUI 外观仍需真机验收。
- 日志：`/tmp/tailsync-theme-focus-red.log`、`/tmp/tailsync-theme-focus-targeted.log`、`/tmp/tailsync-theme-focus-full.log`、`/tmp/tailsync-theme-focus-bundle.log`。

### 10.2 预览联动与 Tab 深读的测试补齐（2026-10-03）

审查发现 `followPreviewSelection`（预览内容实时跟随）与 Tab 深读交接在自动化测试中**零覆盖**——这两条正是契约 §3 Phase 3 的核心。已补 2 项真视图集成测试（`HistoryKeyboardIntegrationTests`）：

| 新增测试 | 断言 |
|---|---|
| `testPreviewFollowsSelectionWithoutStealingFocusAndEscapeClosesItFirst` | 空格开预览后**历史列表仍持有第一响应者**（不夺焦）；改选后预览在防抖后跟随到新条目；Escape 先关预览、历史不动 |
| `testTabFromListEntersDeepReadPreviewForTheSelectedRow` | 列表焦点下 Tab 打开所选条目的深读预览 |

两项都做了**故障注入验证**：把 `followPreviewSelection` 的 `windowIsVisible` 守卫取反、把 Tab 的键位映射改成 `nil`，对应测试均按预期失败；随后已完整回退（工作树只余测试文件改动）。

计数更新：键盘集成测试 **7 项**（原 5 项）；macOS 全量 **250 项，246 通过、4 跳过、0 失败**。§10.1 的 248 项是本次补齐之前的数字。

