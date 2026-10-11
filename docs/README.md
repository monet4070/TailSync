# TailSync 文档索引

本文档集最近一次按 `codex/reliability-hardening@d6c4b24` 的实现复核。版本、协议和数据库 schema
以仓库根目录英文版 `README.md`、中文版 `README.zh-CN.md` 与代码常量为准。

该实现基线的 [CI run 33343172090](https://github.com/monet4070/TailSync/actions/runs/33343172090)
已完成并通过全部 5 个 job，包括 Windows NSIS 构建/烟测、macOS bundle/daemon 验证、双向互操作、
Windows/site 前端和发布恢复脚本。此后的纯文档改动另由版本矩阵、跨平台契约与本地链接检查验证。

## 当前规格

| 文档 | 用途 |
|---|---|
| [`../README.md`](../README.md) | 默认英文版：产品能力、平台支持、架构概览、构建入口与当前限制 |
| [`../README.zh-CN.md`](../README.zh-CN.md) | 简体中文版：与英文版对应的项目说明 |
| [`../CONTEXT.md`](../CONTEXT.md) | 领域词汇、模块边界、跨平台契约与开发门禁 |
| [`USER_GUIDE.zh-CN.md`](USER_GUIDE.zh-CN.md) | 面向使用者的安装、配对、同步、历史、预览与排障说明 |
| [`features/resumable-file-transfer.md`](features/resumable-file-transfer.md) | 文件批次跨重启续传、持久状态、提交边界与验收矩阵 |
| [`features/remote-iroh-pairing.md`](features/remote-iroh-pairing.md) | 无自建服务器的 `tailsync://` Iroh 远程配对、链接安全边界与验收矩阵 |
| [`features/favorites-long-press.md`](features/favorites-long-press.md) | 长按收藏动画、选中态和删除权限 |
| [`features/history-preview.md`](features/history-preview.md) | 独立预览窗口的交互、格式支持与安全边界 |
| [`features/composite-clipboard.md`](features/composite-clipboard.md) | 图文复合剪贴板的目标语义、支持范围、启用条件、结果状态、来源去重、迁移回退与验证要求 |
| [`THEMING.md`](THEMING.md) | Theme V2 包格式、安装、迁移和安全要求 |
| [`DEPENDENCIES.md`](DEPENDENCIES.md) | 安全更新、季度依赖维护和三份 lockfile 策略 |
| [`RELEASE.md`](RELEASE.md) | Community / Trusted 发布、签名和迁移操作手册 |

架构决策位于 [`adr/`](adr/)；已接受且已经落地的 ADR 仍是当前约束，不是待办计划。

## 优化计划

- [COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md](COMPOSITE-CLIPBOARD-IMPROVEMENT-EXECUTION-2026-10-06.md)：逐项实施与暂停汇报台账；S0 基线、五类真实接缝复现、现场待验证条件及后续任务。
- [COMPOSITE-CLIPBOARD-IMPROVEMENT-CODE-REVIEW-2026-10-06.md](COMPOSITE-CLIPBOARD-IMPROVEMENT-CODE-REVIEW-2026-10-06.md)：已完成阶段（O11/O01/O02/O06）的只读实现复核：优雅性、测试冗余、死代码与修改效果；仅提建议，不改代码。
- [COMPOSITE-CLIPBOARD-REVIEW-VERIFICATION-FIXES-2026-10-06.md](COMPOSITE-CLIPBOARD-REVIEW-VERIFICATION-FIXES-2026-10-06.md)：核实最新审查发现，记录误判更正、DeepSeek/high 修复范围与主代理复验。
- [COMPOSITE-CLIPBOARD-O04-RESOURCE-ARCHIVE-2026-10-06.md](COMPOSITE-CLIPBOARD-O04-RESOURCE-ARCHIVE-2026-10-06.md)：DeepSeek/high 与主代理二次补修的公开 PNG 归档、Manifest 3、完整 CAS、网络安全与分层验收。
- [COMPOSITE-CLIPBOARD-O05-SOURCE-CONTEXT-2026-10-06.md](COMPOSITE-CLIPBOARD-O05-SOURCE-CONTEXT-2026-10-06.md)：DeepSeek/high 交接、六项独立 RED 与二次补修；可选同源静态 PNG 文件桥、Manifest 4、授权生命周期及 UNKNOWN 边界。
- [ADR-005：原件身份与版本兼容](adr/ADR-005-composite-content-identity.md)：O11 的内容域、去重资格、旧 hash/重试保全、schema/IPC/契约及回退；含 O11-D2、O01/O02/O06/O10 的生产接线与证据边界。
- [COMPOSITE-CLIPBOARD-FEEDBACK-AND-RESEARCH-2026-10-06.md](COMPOSITE-CLIPBOARD-FEEDBACK-AND-RESEARCH-2026-10-06.md)：六项用户反馈、两份调研、当前代码差距与十二项优化方向；区分待复现现象、代码事实和产品偏好。
- [COMPOSITE-CLIPBOARD-IMPROVEMENT-PLAN-2026-10-06.md](COMPOSITE-CLIPBOARD-IMPROVEMENT-PLAN-2026-10-06.md)：对应十八项反馈/优化方向的方案、依赖和验收用例；O04／O05 可选文件桥的本机部分完成；真实浏览器／GUI／设备门禁仍开放。
- [COMPOSITE-CLIPBOARD-USER-FEEDBACK.md](COMPOSITE-CLIPBOARD-USER-FEEDBACK.md)：用户间断反馈的原始记录及合理性评估。
- [`OPTIMIZATION-PLAN-2026-09-05.md`](OPTIMIZATION-PLAN-2026-09-05.md)：系统设计与工程流程优化方案，包含目标架构、工作包、依赖、验收和回滚边界。
- [`OPTIMIZATION-IMPLEMENTATION-STATUS-2026-09-06.md`](OPTIMIZATION-IMPLEMENTATION-STATUS-2026-09-06.md)：按工作包记录当前实现、已执行门禁和仍需真机/发布环境完成的验收；不能用本地 probe 结果代替真实双系统证据。

## 审计与历史记录

以下文档保留问题发现、证据和决策过程。它们带有日期，正文中的旧行号、旧风险和“待修”描述
只代表当时快照；判断当前状态时先看文件顶部的状态说明和当前规格：

- [`CODE-REVIEW-2026-08-28.md`](CODE-REVIEW-2026-08-28.md)
- [`CODE-REVIEW-2026-08-30.md`](CODE-REVIEW-2026-08-30.md)
- [`SECURITY-AUDIT-2026-08.md`](SECURITY-AUDIT-2026-08.md)
- [`HANDOFF-2026-08-28.md`](HANDOFF-2026-08-28.md)
- [`COMPOSITE-CLIPBOARD-ACCEPTANCE-REVIEW-2026-10-04.md`](COMPOSITE-CLIPBOARD-ACCEPTANCE-REVIEW-2026-10-04.md)：
  图文复合剪贴板候选的验收评审（未提交工作树，基线 `f719d58`）。结论为**当前不可发布**，含 1 个已复现的
  默认构建阻断项、不可逆 v12 迁移的范围问题、一族静默丢失路径，以及三条架构接缝的根因分析。该审查保留当时状态；当前修复与证据见下方 R1–R5 补修记录。
- [`COMPOSITE-CLIPBOARD-REPAIR-EXECUTION-PLAN-2026-10-04.md`](COMPOSITE-CLIPBOARD-REPAIR-EXECUTION-PLAN-2026-10-04.md)：
  修复执行方案（R0—R7 阶段门槛、K1—K10 契约、T01—T30 故障回归、命令门禁与停止条件）。
  R1—R6 已完成本地验证并封存；R7 本机子场景已部分执行，候选仍为 NO-GO。
  当前结果见下方单 Mac 验收报告，不能从本地通过推导完整设备验收通过。

## 架构决策

- [`adr/ADR-004-composite-clipboard-contracts.md`](adr/ADR-004-composite-clipboard-contracts.md)：
  冻结复合剪贴板的十条契约（K1—K10）、确立 ClipboardCoordinator/异步 ClipboardPort、
  EffectJournal 与 MigrationPolicy 三个深边界，并冻结支持范围。ADR-001—003 仍有效。

## 维护规则

- 用户可见行为变化：同步更新 `README.md`、`README.zh-CN.md`、用户指南和对应 `features/` 规格。
- 跨平台业务不变量变化：同步更新 `CONTEXT.md` 和对应 ADR。
- 发布、依赖或安全策略变化：更新对应手册，并保留验证命令和 Go/No-Go 条件。
- 产品版本变化：使用 `node scripts/bump-version.mjs <version>`，不要手工只改一个文件。
- 历史报告不删除旧证据；在顶部追加当前裁决或指向取代它的规格。

- [`COMPOSITE-CLIPBOARD-R1-R5-FOLLOWUP.md`](COMPOSITE-CLIPBOARD-R1-R5-FOLLOWUP.md)：
  2026-10-05 补修清单、当前源码指纹、完整本地门禁和 R6 设备验收边界。
- [`COMPOSITE-CLIPBOARD-R6-VERIFICATION.md`](COMPOSITE-CLIPBOARD-R6-VERIFICATION.md)：
  R6 本地验收（当前 44 个必需命令、六项工作、台账映射、独立版本表）；结论 LOCAL_COMPLETE，
  Windows 原生/完整 App/目标 App/双设备仍 UNKNOWN，发布 NO-GO。
- [`COMPOSITE-CLIPBOARD-R7-ACCEPTANCE.md`](COMPOSITE-CLIPBOARD-R7-ACCEPTANCE.md)：
  R7 实机与双设备验收的执行方式、门禁清单与"无证据不得判 PASS"的规则。

- [COMPOSITE-CLIPBOARD-ACCEPTANCE-FOLLOWUP-2026-10-05.md](COMPOSITE-CLIPBOARD-ACCEPTANCE-FOLLOWUP-2026-10-05.md)：验收复核与证据工具修复。
- [COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md](COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md)：当前单 Mac 原生验收、产品修复、证据范围与未完成条件。

- [COMPOSITE-CLIPBOARD-NATIVE-FOLLOWUP-2026-10-05.md](COMPOSITE-CLIPBOARD-NATIVE-FOLLOWUP-2026-10-05.md)：真实 delayed provider、通知测量、旧 GUI 与签名身份限制。
- [COMPOSITE-CLIPBOARD-PERF-FOLLOWUP-2026-10-05.md](COMPOSITE-CLIPBOARD-PERF-FOLLOWUP-2026-10-05.md)：实际 cache、50/250 ms timer、PUBLISH 串行等待与主代理独立复算。
