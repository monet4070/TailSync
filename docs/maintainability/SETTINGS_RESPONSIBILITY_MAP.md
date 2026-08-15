# TailSync Maintainability Refactor — SETTINGS RESPONSIBILITY MAP (T200/T247)

> 生成时间：2026-08-15（T246 typed client 迁移后更新）
> 依据：T000 全量阅读（1774 行时点）+ T241-T246 迁移后复核（当前 1708 行）。
> 只读交付。本图是 R006（T248+ 增量提取）的唯一事实来源。

---

## 0. 提取完成状态（T259 收官，2026-08-15）

R006 的 6 项提取建议中 5 项已完成，第 6 项（update()/写回）按 §7 建议保持不动：

| # | 提取 | Task | 文件 | 单测 |
|---|---|---|---|---|
| 1 | updater | T248 | `hooks/useUpdater.ts` | 集成用例保护 |
| 2 | connectionTests | T255 | `hooks/useConnectionTests.ts` | 集成用例保护 |
| 3 | shortcut 录制 | T256 | `hooks/useShortcutRecorder.ts` | T257：6 用例 |
| 4 | pairing | T258 | `hooks/usePairing.ts` + `utils/pairingAddress.ts` | T258：13 用例 |
| 5 | devices | T259 | `hooks/useDevices.ts` | T259：13 用例 |
| 6 | update()/写回 | —（保持） | `Settings.tsx` 内 | LatestRequest+SerialTaskQueue 双回滚编排保持原状 |

**收官实测（T260）：**
- Settings.tsx：1774 → 1202 行（-572）；useState 29 → 8；useEffect 10 → 3；useRef 16 → 4；直接 invoke **0**（R008）。
- 页面仅剩编排 JSX + settings 枢纽（update()/settingsRef）+ 跨 feature 编排（handleConnectionMode/handleForget/changeStorage 等）。
- 前端 vitest 111/111 全绿；tsc + vite build 通过；oxlint 0 error（1 pre-existing warning：routeSupportsLatencyTest only-export-components）。
- 漂移检查 PASS（"Cross-platform contract passed…"，T260 复核）。

---

## 1. UI 分区（FACT，行号=当前文件）

| Section | 行号 | 内容 |
|---|---|---|
| 标题栏（含 loading 态） | ~910-920 区域 | ThemeLogo、版本 badge、`getCurrentWindow().hide()` |
| 连接与设备 | L924-1196 | 连接模式单选、配对开关、本机设备行、每 peer 设备行+路由列表、连接测试按钮、sync 开关、forget/pair 动作 |
| 通用 | L1199-1327 | 全局 sync 开关、快捷键录制+重置/清除、通知开关、进度条开关 |
| 历史 | L1330-1361 | history-limit 滑块 |
| 存储 | L1364-1417 | 存储位置+变更按钮、配额输入、旧存储清理行（注释曾误标 Appearance，K009） |
| 外观 | L1420+ | 主题 卡片、配色、语言选择 |
| 更新 | 后续 | 版本串+检查/安装按钮 |
| 快捷键录制对话框（条件） | 后续 | 键盘捕获、keycap 预览、错误/成功、取消/保存 |
| 配对对话框（条件） | 后续 | 验证码、指纹、确认、错误 |
| Toast | 尾部 | 错误/保存成功 |

## 2. 状态（FACT，T000 实测 29 useState + 16 useRef；0 useMemo/0 useCallback；迁移后未变）

- 29 useState（L197-224 时点）：settings（规范源）、historyLimitDraft、storageQuotaDraft、storageStatus、storageBusy、oldStorage、updateStatus、availableUpdate、updatePhase、updateError、saved、errorMessage、devices、devicesLoading、devicesError、pairingTarget、pairingStatus、pairingOpen、pairingError、pairingBusy、connectionTests、shortcutDraft、shortcutBusy、shortcutRecording、shortcutCandidate、shortcutPreviewKeys、shortcutCaptureActive、shortcutDialogError + hooks（theme×4 经 useTheme、t/setLocale 经 useI18n）。
- 16 useRef：toastTimer、shortcut 系 ×8（trigger/dialog/capture/previousFocus/cancelHandler/captureHandler/recordingRef）、previousPairingPhase、pairingBusyRef、settingsRef、saveQueue（SerialTaskQueue）、settingsUpdates（LatestRequest）、pairDialogRef、previousFocus。
- 特征：单一 `settings` state + 单一 `update()` 写回通道（乐观写 + 双重回滚）；`devices` state 被设备/配对/连接测试共享。

## 3. Effects（FACT，10 个 useEffect）

| 行号（时点） | 目的 |
|---|---|
| 249-272 | 初始加载：getSettings + getStorageStatus + getUpdateStatus（seed drafts/theme/locale） |
| 274 | unmount 清理 toast timer |
| 275-277 | unmount 时若录制中 resumeSyncShortcut |
| 278-296 | listen("sync-state-changed") → 修补 sync_enabled |
| 297-299 | pairingBusy → ref 镜像 |
| 302-336 | 设备轮询：getPeers 初始 + 5s interval（可见时）+ listen("peer-health-changed") |
| 438-486 | 配对对话框焦点陷阱/Escape/Tab |
| 530-556 | 配对状态轮询（1s）+ 自动开关对话框 |
| 814-858 | 快捷键对话框焦点/Escape（经 handler ref） |
| 860-876 | 捕获键 keydown/keyup |

## 4. 命令面（T246 后现状：全部经 tailsyncClient，31 调用点 → wrapper）

- 直接 invoke：**0**（R008 收官，T246）。
- wrapper 依赖（按 feature）：updater=getUpdateStatus/checkForUpdate/installUpdate；storage=getStorageStatus/changeStorageLocation/deleteOldStorage；devices=getPeers/refreshPeers/togglePeer/testConnection/forgetPeer；pairing=getPairingStatus/enablePairing/startPairing/cancelPairing/confirmPairing；shortcut/sync=setSyncShortcut/suspendSyncShortcut/resumeSyncShortcut/setSyncEnabled；写回=updateSettings/getSettings。
- 事件订阅：listen("sync-state-changed")、listen("peer-health-changed")。

## 5. Feature 依赖图（FACT，T000 实测 + 迁移后复核）

- **耦合枢纽**：`settings`/`settingsRef` + `update()`（L338-374 时点）——update/连接模式/历史/存储配额/主题/语言 全部读写它；shortcut 成功后手工修补 settings（绕过 update）。
- **devices 枢纽**：设备列表/配对（配对后刷新）/连接测试（finally 重取）三 feature 共享。
- **隔离良好**：updater（updateStatus/availableUpdate/updatePhase/updateError 独立 state，零共享）；connectionTests（独立 Map）。
- 快捷键捕获状态机横跨 6 函数 + 2 组键盘 effect；配对流程为轮询驱动自开对话框。

## 6. 复杂度热点（T248+ 拆分输入）

1. `update()` 双回滚编排（LatestRequest+SerialTaskQueue）——settings 写侧核心，拆分时保持不动或整体提取。
2. 设备行 JSX 5-6 层嵌套三元（L1104-1244 时点）。
3. shortcut 录制状态机（start/cancel/restart/confirm/handleCapture + 2 effect 对 + 模块级 handler refs）。
4. 配对流程（openPairing guard + enable/start 嵌套 + catch 回退）。
5. 两套近似相同的焦点陷阱（配对 L443-450 与快捷键 L818-823 时点）。
6. toast 双实现（update 保存 + shortcut 保存）。
7. changeStorage 重跑 init 水合（重复初始加载逻辑）。

## 7. T248+ 提取建议（按隔离度排序，BEHAVIOR_CHANGE_ALLOWED=FALSE）

1. **updater → useUpdater hook**（updateStatus/availableUpdate/updatePhase/updateError + getUpdateStatus/checkForUpdate/installUpdate + 更新区块渲染 props 回调）——零共享 state，先行试点。
2. **connectionTests → useConnectionTests hook**（connectionTests Map + testConnection + devices 重取 finally）。
3. **shortcut 录制 → useShortcutRecorder hook**（录制状态机 + 键盘 effect + 焦点陷阱 + suspend/resume 账目）。
4. **配对 → usePairing hook**（轮询 + 对话框状态 + enable/start/cancel/confirm + 自动开关）。
5. **设备列表 → useDevices hook**（轮询 + refresh + peer-health-changed 订阅）。
6. **update()/写回 → 保持**（结构稳定后单独评估）。

每个提取：核心测试（现有 vitest mock 契约）+ build/lint/test 全绿 + 页面仅剩编排 JSX。
