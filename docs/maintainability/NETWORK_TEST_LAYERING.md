# TailSync Maintainability Refactor — NETWORK TEST LAYERING (T420)

> 生成时间：2026-08-15（T420，R019 输入，T400 后实测）
> 依据：T000 BASELINE §10（interop 描述）+ 本文件计数为 **T420 实测**。
> 只读交付。本图是 R019（确定性测试下沉 core + 真实传输测试分层）的唯一事实来源。

---

## 1. 分层现状（FACT，测试计数实测）

| 层 | 载体 | 测试数 | 特征 |
|---|---|---|---|
| **确定性（in-memory）** | peer/delivery.rs `MemoryConnection`（15 处引用，in-memory Noise 握手） | delivery 26 + event_receiver 5 = **31** | 帧记账/ACK/竞速/事件处理全在内存通道 |
| 状态机（无网络） | peer/health.rs 12、peer/directory.rs 19 | **31** | 纯状态/解析（directory +6 于 T107 时代 resolve_candidates） |
| **真实传输（被忽略）** | iroh_transport.rs 7（**1 ignored**：`repeated_rtt_probes`，K005 "环境回归：本机 localhost QUIC 连接超时"） | 7 | 依赖 QUIC/iroh 运行时 |
| **真实传输（CI-only）** | interop_probe.rs（307 行，byte-identical W/M，漂移钉扎）+ test_cross_project_interop.ps1（双向：一侧 server 一侧 client，Noise 握手 + 文件/文本往返断言） | CI Windows job | pwsh 本机不可用（T002 §10） |

## 2. 分类表（T421+ 输入）

确定性可下沉：delivery/event_receiver 的 in-memory 测试已全在 core ✓（现状即目标层）。
真实传输候选审计：

| 候选 | 现状 | 建议 |
|---|---|---|
| repeated_rtt_probes（iroh_transport.rs，ignored K005） | 环境回归 ignored | 修复需 CI 网络环境验证；本机保持 ignored（不凑数） |
| interop_probe 双向烟测 | PS1 CI-only | 保持；PS1 已在漂移钉扎（两端同文） |
| iroh_transport 其余 6 测试 | 运行中 | 保持（iroh 依赖面，R012/R019 均不触及） |

## 3. R019 判定

- 确定性测试已全部位于 core（R019 目标状态达成，无需迁移）；R004 时代的 delivery 迁移已将其带入。
- 真实传输层测试维持现状：本机不可运行项（PS1/ignored）不强行本机化（避免环境回归造假证），CI 门禁保持（R015）。
- 下一步（T421+，可选）：为 delivery in-memory 测试补充连接竞速的确定性用例（已有 26 个，增量按 R014 高风险路径判定）。
