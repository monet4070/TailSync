# S6-P0-1 运行时门禁的故障注入证据（P0）

> 目的：证明 Windows 成品端口的**运行时**断言确实能抓住“重新启用旧 JSON TCP API”这一回归。
> 执行方式：临时分支 + `workflow_dispatch` 完整 CI，事后删除分支；变异**不进入日常 CI**。

## 变异

从 `origin/main`（`b61a0a0`）建临时分支 `ci/batch-p2-s6p0-1-mutation`，在保留 Phase 2 成品断言的同时移除三处门禁，让 Windows 生产路径真的启动 `127.0.0.1:19889` 的 JSON TCP API：

1. `windows/src-tauri/src/lib.rs`：移除 `api::start` 块前的 `#[cfg(not(target_os = "windows"))]`
2. `windows/src-tauri/src/api.rs`：移除 `pub use transport::start;` 前的同一 cfg
3. `windows/src-tauri/src/lib.rs`：移除 `let api_token = ...` 前的同一 cfg

变异提交：`3624168`（分支已从远端删除；该未引用提交仍可由 GitHub API 取得）。

**前两次尝试不作为证据**：它们分别在 Clippy 阶段以 `cannot find function start in module api`、`cannot find value api_token` 失败——那是编译失败，不是运行时断言失败。三处齐改后 Clippy 通过，才真正到达成品断言。

## 结果

- 运行：`36603236594`（`workflow_dispatch`，`conclusion=failure`）
- 失败 job：`Packaged application (Windows)`
- 失败步骤：**`Build NSIS and run packaged executable/deep-link smoke tests`**（不是 Clippy、不是单元测试）
- 输出原文：

  > Packaged TailSync is listening on the legacy local API port 127.0.0.1:19889; the Windows app must use Tauri invoke/event IPC instead.

## 结论

该断言是**运行时门禁**：重新启用旧监听会让 Windows 打包 job 报红，`Required verification` 随之失败。日志同时显示 Clippy、单元测试与 `Check cross-platform contracts` 均已通过，所以失败可归因于成品断言本身。

## 复现方式

1. 从当时的 `origin/main` 建临时分支；
2. 应用上述三处移除（保留 `windows/scripts/package-windows.ps1` 中的成品断言）；
3. `gh workflow run CI --ref <branch>`；
4. 确认 `Build NSIS and run packaged executable/deep-link smoke tests` 失败并输出上面的消息；
5. 删除临时分支。
