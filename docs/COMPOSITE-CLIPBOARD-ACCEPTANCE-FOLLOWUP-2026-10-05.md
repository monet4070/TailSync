# Composite clipboard 验收复核与证据工具修复 — 2026-10-05

本文记录本日最初的证据工具验收。用户后来释放端口并授权继续后，已执行真实 App 验收及产品修复。
**当前结论和源码入口见 [单 Mac 当前报告](COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md)**；下文未启动 App / 未触碰剪贴板的声明仅限前期工具审计。

## 前期工具验收结论

R1–R6 的既有实现与 RR-12 关闭依据得到核对；R7 的原“PASS 2 / UNKNOWN 8”口径过宽。
本机故障/回退证据是支持项，完整设备阶段应为 **PASS 0 / UNKNOWN 10 / Release NO-GO**。
本轮补齐验收工具缺陷、逐场景证据绑定和当前 run 的文档入口，没有提交、推送或发布。

最终 R6 **44/44 PASS，0 unexpected，1 个唯一 Windows host 编译警告**。
故障矩阵 17 项观察、repair 30/30、四个 App 包与 debug、包内探针全部通过。封存结果见
`artifacts/composite-repair/2026-10-05-acceptance-hardened-r6-final/status.json` 为准。
旧 run 与中途变更源码的 run 不继承为当前候选证据。

## 初始工作完成情况

实际工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`；
HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。用户当前目录是同 HEAD 的基线 checkout，
本轮没有修改基线 checkout，也没有覆盖已有产品实现。

- 复核初始源码聚合为 `53dcfeb3106c2e8aa7c88cd81439aec018ce531ae64842a80098b4ea207332cb`，751 文件。
- 原 `d2937a6b…` 的 evidence-index 逐文件核验 0 不匹配；源码差异恰是 CI 与两个 R7 文件。
- 替代 run `2026-10-05-r6-after-verify` 确有 42 条记录、0 unexpected，源码逐文件与初始工作树一致。
  但该 run 没有 `status.json`/`evidence-index.json`；R6 文档与 R7 默认目录仍指向旧 run。
- RR-12 的 helper probe 均先于 DB 打开；release helper resolver 只查 executable parent，
  manifest/binaries 分支受 debug cfg 限制。账本为 30 closed / 0 open；保留原关闭内容。
- 原 R7 九项规则测试通过。新增四项负向测试随后全部失败，直接复现了工具的误判，未依赖用户记录的结论。

## 发现与修复

| 问题 | 原行为与风险 | 本轮修复 |
|---|---|---|
| P1 当前候选未绑定 | 回退只比较 proof 与同一旧 run 的 source 文件；源码变了仍可 PASS | 比较当前逐文件指纹，验证完整封存 R6、当前矩阵、命令/过滤器/测试数、原始日志和实际包文件；R7 保存候选与封存 hash |
| P1 故障误通过 | 探针退出 0，即使 `satisfied=false` 仍记 PASS | R7 同时检查退出码和五组真实断言；native probe 自身失败也返回 1，JSON 与 stderr 分开保留 |
| P1 支持证据越界 | 私有 board 被当作包含真实 storage/revoke 的完整故障门禁；合成 authority 被当作实机安装切换 | 单列 local-support，两个支持项通过仍保留完整设备门禁 UNKNOWN |
| P1 场景可遗漏 | 一个非空文本即可关闭 Windows 的 MSVC/dev/portable/NSIS 整组 | 观察 JSON 要求逐场景 PASS、预期/实测、设备/应用版本、源码/包 hash 和原始证据；少一项即拒绝 |
| P1 非法状态假验收 | 十项非法结果既不计 FAIL 也不计 UNKNOWN，报告可输出 acceptable | 仅接受合法状态；INVALID 阻断；只有十项完整设备 PASS 才可进入候选评审；报告状态也重新校验观察 manifest |
| P2 缺少正式封存与执行门禁 | 新 R6 run 无封存；R7 测试只进入 CI，未进入 R6 矩阵；旧默认 run/文档仍在使用 | 矩阵增加 R7 与 R6 有效性回归共 44 项；explicit R6 run；当前文档指向最终 run；封存后拒绝重跑/重封存 |
| P2 可变测试预期未指纹 | canonical expected-failures ledger 在 artifacts 中，未参与源码聚合 | 将该配置纳入指纹，修改 green/red 预期会使旧证明过期 |
| P2 文件/重跑操作不稳健 | 目录证据可能异常；相对路径可能重复拼接；自动日志覆盖旧尝试 | regular-file 校验、规范路径、原始文件 hash、独立输出名、观察历史保留、状态原子替换；自动支持不覆盖人工记录 |

修复只改变证据工具、故障探针 driver、规则测试、CI 和文档，没有改动产品协议、数据库、GUI 或 native helper 实现。
人的观察是操作者声明：此工具能检查覆盖与变更，不能从任意文件内容证明真实粘贴行为或包的编译来源。
独立设备验收仍必须核对实际候选、构建日志与原始观察。

## 验证与证据

- 初始四项负向复现日志：`artifacts/composite-repair/2026-10-05-acceptance-review/r7-negative-before.log`。
- Python 规则测试 **65/65**：R7 28、R6 有效性 18、日志解析 12、repair gate 7。
- R6 校验的 18 项测试在 Python `-O` 下也通过；关键判定使用显式错误，不依赖 assert。
- `exit 0 + false`、旧候选/旧 seal、缺场景、Mac 伪 Windows、缺第二设备、包/原始证据改动、
  非法状态、目录证据、路径越界、改命令/降低 min_tests/缩减 matrix、零测试、skip 和旧失败均有拒绝测试。
- package inventory 校验按 producer 的顺序复算聚合；debug 的 daemon/helper 清单顺序有正向回归。
- 中途 run `2026-10-05-acceptance-hardened-r6` 在校验器源码修改后主动拒绝封存，标记 INCOMPLETE。
  所有命令与原始日志保留；最终 run 从新 hash 完整执行。
- 最终 R6 44 条命令均执行通过，source hash 在每条命令前后保持一致；本机范围 LOCAL_COMPLETE。
- 当前源码聚合：`0d9d381078fbd613f9b5b616ece136d07a67b8a64beffec25730e28d7c5ad80a`，753 文件。
- 复核上下文与规则日志：`artifacts/composite-repair/2026-10-05-acceptance-review/`。

## 剩余完成条件

完整 Mac App、Windows MSVC/dev/portable/installer、来源/目标应用、双向同步、意图竞争、
真实网络、真实故障/撤销授权、实际安装版本回退、两端可访问性仍须设备与人执行。
使用 `COMPOSITE-CLIPBOARD-R7-ACCEPTANCE.md` 的逐场景模板取证；不能用本机支持项关闭设备门禁。

hosted CI 未运行，状态 UNKNOWN；本机完整矩阵也不替代 hosted CI 或原生设备验收。
发布 **NO-GO**。未启动完整 App、未触碰 general clipboard、未终止用户 daemon。
