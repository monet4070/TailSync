# TailSync Maintainability Refactor — PHASE 0 AUDIT（§19 周期性审计）

> 审计时间：2026-08-15（Phase 0 结束：T000-T003 完成）
> 审计不修改代码。

---

## 1. Requirement Coverage（T001 追溯 + 实测证据）

| R | 状态 | 证据 |
|---|---|---|
| R001 保持现有行为 | **VERIFIED**（基线层面） | T002 全绿（REGRESSION_BASELINE.md §1）；无代码修改 |
| R002 跨平台兼容 | **VERIFIED**（基线层面） | 漂移检查 PASS（38 Swift API commands、四端契约）；interop 本机 UNVERIFIED（CI-only） |
| R003 Core 平台无关 | **PARTIAL** | 现状记录（已存在 cfg 平台依赖，边界="不新增"）；尚无新代码可验证 |
| R004 消除 byte-identical 重复 | **PARTIAL** | T100 清单完成（6 C 类候选 + 2 D 类）；迁移未开始 |
| R005 保留 drift checker | **VERIFIED** | 检查器运行 PASS；职责未缩小未删除 |
| R006 Settings 拆分 | **PARTIAL** | 责任图数据已收集（BASELINE §7.1）；未执行拆分 |
| R007 History 拆分 | **PARTIAL** | 责任图数据已收集（BASELINE §7.2）；未执行拆分 |
| R008 typed client | **PARTIAL** | 45 调用点/26+14 命令清单已收集；未迁移 |
| R009 commands 变薄 | **PARTIAL** | 35 命令清单 + 5 厚命令已点名；未拆分 |
| R010 sync 拆分 | **PARTIAL** | 责任图已收集（BASELINE §6.1）；未拆分 |
| R011 不重构 DB | **VERIFIED**（约束层面） | 基线记录 db.rs 64.6% 测试；未动 DB |
| R012 typed error | **PARTIAL** | 错误面统计完成；未迁移 |
| R013 error boundary | **PARTIAL** | Swift 子串契约已记录（K004）；未动 |
| R014 测试优先保护高风险 | **PARTIAL** | 高风险路径测试分布已记录；续传 5 专项在列 |
| R015 CI 不得降低 | **VERIFIED** | ci.yml/release.yml 未动；本机复跑 CI 同款命令全 PASS |
| R016 coverage 可观察 | **NOT_STARTED** | 无 coverage 报告（设计在 T003 文档中未涉及，属 Phase 4） |
| R017 单一版本入口 | **PARTIAL** | T003 设计完成（VERSION_MATRIX.md）；未实现 |
| R018 可观察性 | **NOT_STARTED** | 日志清单已收集（BASELINE §11）；schema 设计未开始 |
| R019 网络测试分层 | **NOT_STARTED** | 现状已记录（interop probe + 1 ignored 测试） |
| R020 fuzz/property | **NOT_STARTED** | 现状 0 fuzz target；候选审计未做 |
| R021 不统一平台 UI | **VERIFIED** | 未做任何 UI 统一动作 |
| R022 避免过度架构 | **VERIFIED** | 本轮新增 4 份文档 0 行代码；无新 abstraction |

## 2. Scope Drift

- 本轮产出：4 份分析文档（BASELINE/REQUIREMENTS_TRACEABILITY/VERSION_MATRIX/IDENTICAL_SOURCE_INVENTORY/REGRESSION_BASELINE = 5 份）+ PROJECT_STATE 更新。
- 无代码修改、无 CI 修改、无新 abstraction、无无关 refactor。
- 结论：**无 drift**。唯一"顺手"项：T100 分类时对 6 个 C 类文件做了 tauri/cfg 依赖核查（属 T100 本身必要事实，非越界）。

## 3. Duplication Audit

- 本轮未移动任何代码 → 无重复转移问题。
- 记录层面：T000 已确认现有重复面（双命令面 K002、byte-identical 清单、db.rs 测试内联）未增加。

## 4. Complexity Audit

- 新增概念：0 个代码抽象；文档 5 份均为后续 Task 的直接输入（追溯/基线/版本/清单），不增加调用者认知成本。
- VERSION_MATRIX.md 的 bump 工具设计坚持 Node 脚本（候选 B）而非 xtask crate —— 避免为形态新增 crate（§22 精神）。

## 5. Regression Audit

- 过去 VERIFIED 项（R001/R002/R005/R011/R015/R021/R022）本轮无代码变动 → 不受影响。
- 基线数字已固化（REGRESSION_BASELINE.md），后续任何重构对照此表。

## 6. Assumption Audit

- 本轮 ASSUMPTION 数：0（T000 声明无假设）。
- 无待验证假设。
- 记录过的 INFERENCE（如"Settings 更新 feature 隔离度最高"）为分类/建议性质，已在对应文档标注，不影响验证。

## 7. 审计结论

- Phase 0 达标：基线真实、追溯完整、无 drift、无回归。
- 下一阶段入口：Phase 1（T101+ 共享 runtime 迁移，起点 = rate_limit.rs 试点）或先行 Phase 2 只读图（T200/T220/T240/T300/T320/T350/T400 数据已备）。
- 建议顺序：T101（rate_limit.rs 迁移试点，验证迁移模板）→ 后续按 IDENTICAL_SOURCE_INVENTORY §3 顺序。
