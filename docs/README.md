# TailSync 文档索引

本文档集最近一次按 `codex/reliability-hardening@d6c4b24` 的实现复核。版本、协议和数据库 schema
以仓库根目录 `README.md` 与代码常量为准。

该实现基线的 [CI run 33343172090](https://github.com/monet4070/TailSync/actions/runs/33343172090)
已完成并通过全部 5 个 job，包括 Windows NSIS 构建/烟测、macOS bundle/daemon 验证、双向互操作、
Windows/site 前端和发布恢复脚本。此后的纯文档改动另由版本矩阵、跨平台契约与本地链接检查验证。

## 当前规格

| 文档 | 用途 |
|---|---|
| [`../README.md`](../README.md) | 产品能力、平台支持、架构概览、构建入口与当前限制 |
| [`../CONTEXT.md`](../CONTEXT.md) | 领域词汇、模块边界、跨平台契约与开发门禁 |
| [`USER_GUIDE.zh-CN.md`](USER_GUIDE.zh-CN.md) | 面向使用者的安装、配对、同步、历史、预览与排障说明 |
| [`features/resumable-file-transfer.md`](features/resumable-file-transfer.md) | 文件批次跨重启续传、持久状态、提交边界与验收矩阵 |
| [`features/remote-iroh-pairing.md`](features/remote-iroh-pairing.md) | 无自建服务器的 `tailsync://` Iroh 远程配对、链接安全边界与验收矩阵 |
| [`features/favorites-long-press.md`](features/favorites-long-press.md) | 长按收藏动画、选中态和删除权限 |
| [`features/history-preview.md`](features/history-preview.md) | 独立预览窗口的交互、格式支持与安全边界 |
| [`THEMING.md`](THEMING.md) | Theme V2 包格式、安装、迁移和安全要求 |
| [`DEPENDENCIES.md`](DEPENDENCIES.md) | 安全更新、季度依赖维护和三份 lockfile 策略 |
| [`RELEASE.md`](RELEASE.md) | Community / Trusted 发布、签名和迁移操作手册 |

架构决策位于 [`adr/`](adr/)；已接受且已经落地的 ADR 仍是当前约束，不是待办计划。

## 审计与历史记录

以下文档保留问题发现、证据和决策过程。它们带有日期，正文中的旧行号、旧风险和“待修”描述
只代表当时快照；判断当前状态时先看文件顶部的状态说明和当前规格：

- [`CODE-REVIEW-2026-08-28.md`](CODE-REVIEW-2026-08-28.md)
- [`CODE-REVIEW-2026-08-30.md`](CODE-REVIEW-2026-08-30.md)
- [`SECURITY-AUDIT-2026-08.md`](SECURITY-AUDIT-2026-08.md)
- [`HANDOFF-2026-08-28.md`](HANDOFF-2026-08-28.md)

## 维护规则

- 用户可见行为变化：同步更新 `README.md`、用户指南和对应 `features/` 规格。
- 跨平台业务不变量变化：同步更新 `CONTEXT.md` 和对应 ADR。
- 发布、依赖或安全策略变化：更新对应手册，并保留验证命令和 Go/No-Go 条件。
- 产品版本变化：使用 `node scripts/bump-version.mjs <version>`，不要手工只改一个文件。
- 历史报告不删除旧证据；在顶部追加当前裁决或指向取代它的规格。
