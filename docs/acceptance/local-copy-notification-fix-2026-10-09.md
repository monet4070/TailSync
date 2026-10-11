# 本机复制重复弹出到达通知修复（2026-10-09）

## 问题与修复位置

上一轮仅诊断和记录通知问题，`2026-10-09-qq-rich-text-01/TailSync.app` 未包含通知修复。用户正在运行此包，本机复合记录来源为空，Swift 到达通知仅过滤 `self`，因而复制文字、图片和 QQ 图文都可能被当成远端新内容提示。

本轮已修改产品：

- macOS `macos/src-tauri/src/clipboard.rs`：正常复合采集与 native-only 采集两条持久化路径均使用 `self` 来源。
- Windows `windows/src-tauri/src/clipboard.rs`：对应便携/原生采集同样使用 `self`，避免两端本机来源漂移。
- macOS `macos/swift-ui/Sources/TailSync/TailSyncApp.swift`：到达通知仅接受明确的非空远端来源；`self`、空白与旧的空来源均保持安静。运行时错误、失败提示与远端到达通知路径保留。
- `AppBehaviorTests.swift`：回归验证本机及旧来源静默，同时正常远端来源仍提示。

首次 macOS 格式检查发现此前 `commands/history.rs` 有一行未按 rustfmt 换行；仅修正该表达式排版，行为不变，原失败日志保留。

## 验证

新回归修复前失败：空来源、空白来源及带空格的本机来源均被误判为应通知。修复后完整 AppBehaviorTests 28/28 通过，包含远端仍可提醒的回归。

macOS 和 Windows 格式检查、canonical include 与采集失败提示检查、diff-check、macOS composite-enabled all-targets cargo check 均通过。Windows 在 macOS 上的 host all-targets check 通过，保留因 Windows-only 路径被裁掉而产生的 dead-code 警告，不替代 Windows 原生验收。

本地证据目录：`artifacts/rich-text-repair/2026-10-09-local-copy-notifications/`。改动前四个文件与本轮独立 diff 均保存，旧 App 保留，没有提交或发布。

## 交付

修复包：`/Users/monet/Downloads/TailSync构建/2026-10-09-qq-rich-text-notifications-01/TailSync.app`。包含本轮通知修复以及此前 QQ 识别和网络图片下载反馈修复。包身份和切换结果见该目录 package-manifest.json、results.json、switch-result.json。

只读备份切换前生产历史库、配置和剪贴板；通过 App 正常退出流程切换版本。没有关闭用户通知开关，也不修改 macOS 剪贴板隐私提示或 QQ 缓存权限。QQ 第二张图的受保护缓存访问仍是前一轮记录的未完成项。

本轮静默与远端通知语义通过生产通知决策接口的回归验证；不声称完成真实双设备到达通知、Windows 原生或所有 macOS 系统提示验收。

已通过旧 App 的 Quit 菜单正常退出，启动新包后确认 UI PID 25953 与 daemon PID 25955 均来自 notifications-01 包。切换前的 5 条历史记录、剪贴板 snapshot 和通知设置保持一致。新 App 启动后没有可供自动化读取的窗口，Computer Use 返回超时；未将进程启动或本地决策回归算作新 App 实际通知横幅的 GUI 验收。
