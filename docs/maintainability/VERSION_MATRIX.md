# TailSync Maintainability Refactor — VERSION_MATRIX & 统一版本工具设计 (T003/T355)

> 生成时间：2026-08-15
> 依据：BASELINE_REPORT §3（T000 实测）+ 本机 Cargo.lock / package-lock.json 核查（T003 补充）。
> T003 只调查与设计；**实现 = T355（scripts/bump-version.mjs，见 §5）**。

---

## 1. VERSION_MATRIX（全部 FACT，实测于 2026-08-15）

| # | 文件 | 字段 | 当前值 | 被校验 | 备注 |
|---|---|---|---|---|---|
| 1 | `windows/src-tauri/tauri.conf.json` | `version` | 2.1.0 | ✅ validator | 打包版本来源 |
| 2 | `macos/src-tauri/tauri.conf.json` | `version` | 2.1.0 | ✅ validator | 打包版本来源 |
| 3 | `windows/src-tauri/Cargo.toml` | `version` | 2.1.0 | ✅ validator（cargo metadata --locked） | |
| 4 | `macos/src-tauri/Cargo.toml` | `version` | 2.1.0 | ✅ validator | |
| 5 | `shared/rust-core/Cargo.toml` | `version` | 2.1.0 | ✅ validator | |
| 6 | `windows/package.json` | `version` | 2.1.0 | ❌ | 前端包版本 |
| 7 | `site/package.json` | `version` | 2.1.0 | ❌ | 营销站点，独立发布物 |
| 8 | `windows/src-tauri/Cargo.lock` | `tailsync-core` 条目 | 2.1.0 | ⚠️ 间接 | `--locked` 构建要求 lock 与 manifest 一致 |
| 9 | `macos/src-tauri/Cargo.lock` | `tailsync-core` 条目 | 2.1.0 | ⚠️ 间接 | 同上 |
| 10 | `shared/rust-core/Cargo.lock` | `tailsync-core` 条目 | 2.1.0 | ⚠️ 间接 | 同上 |
| 11 | `windows/package-lock.json` | root + `packages[""]` | 2.1.0 | ❌ | npm ci 一致性 |
| 12 | `site/package-lock.json` | root + `packages[""]` | 2.1.0 | ❌ | 同上 |
| 13 | `README.md` 徽章/正文 | 文本 | v2.1.0 | ❌ | 文档性，非机械校验 |

- FACT: `scripts/validate-release-version.mjs` 校验 #1-#5（`validateRepositoryVersions`，经 `cargo metadata --locked --no-deps`）；tag 必须 `vX.Y.Z` 且 5 处全等；同时输出 stable/prerelease 通道判定。
- FACT: CI（ci.yml `scripts` job + release.yml `validate` job）执行该校验；release 流程在打包前强制一致。
- FACT: Cargo.lock 的三个 `tailsync-core` 版本条目与 package-lock 的 `version` 字段均实测为 2.1.0（本机核查）。
- FACT: 三个 Cargo.lock 为独立文件（非 workspace），每个 crate 各自锁定。
- INFERENCE: "被校验"缺口 = #6/#7（package.json）与 #11/#12（package-lock）——CI 不检查它们；#13 为文档。

## 2. 现状成本

- FACT: 一次版本升级需手工同步 12 个文件（#1-#12），其中 4 个 lock 文件若不同步会导致 `--locked` 构建或 `npm ci` 失败——失败模式是"构建期才暴露"而非"提交前暴露"。
- FACT: 现有 validator 只检测不一致、不提供写入入口；无任何脚本提供 bump 功能。
- INFERENCE: 统一工具的价值：单一入口 + 写入后立即自校验 + lock 同步，把 12 文件的手工操作压缩为 1 条命令。

## 3. 统一版本工具设计（仅设计，不实现）

### 3.1 形态选择

- 候选 A：`cargo xtask`（新增 xtask crate）。
- 候选 B：Node 脚本 `scripts/bump-version.mjs`（复用现有 node 脚本基础设施、`node --test` 测试模式、与 validator 同栈）。
- INFERENCE: 选 B。理由：仓库现有版本工具全部为 node（validator/generate-manifest 均为 .mjs 且带 .test.mjs）；xtask 会新增一个 crate 与 toolchain 面，收益低于成本（§22 Shared Code Decision Test 精神：不为形态增加架构）。`cargo xtask version X.Y.Z` 目标可通过 npm/脚本等价满足。

### 3.2 命令契约（设计）

```text
node scripts/bump-version.mjs X.Y.Z            # 写入 12 个文件 + 自校验
node scripts/bump-version.mjs X.Y.Z --check    # 只校验不写入（CI 可复用）
node scripts/bump-version.mjs --dry-run        # 预览将修改的文件与 diff
```

步骤（实现任务时细化）：

1. 语义版本校验（复用 validator 的 tag 正则逻辑，`^v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$`）。
2. 写 #1-#5（JSON/Cargo.toml 精确字段替换，保留格式与注释）。
3. 同步 3 个 Cargo.lock：对每个 crate 运行 `cargo generate-lockfile --manifest-path <…>`（不触碰依赖解析，仅刷新自身条目）。
4. 写 #6/#7 与 #11/#12：package.json `version` + package-lock 的 root 与 `packages[""]` 两处。
5. 可选 `--readme`：更新 README 徽章 vX.Y.Z（默认跳过，文档性）。
6. 自校验：调用 `validate-release-version.mjs --root . --tag vX.Y.Z`（复用现有检查，不发明新规则）。
7. `--check` 模式：只跑步骤 6 + lock 一致性 grep，CI 用。

### 3.3 验收锚点（实现任务的 AC 草案）

- AC1: 三个 crate 与两个前端项目在 bump 后 `cargo check --locked` / `npm ci` 均成功（lock 同步验证）。
- AC2: `validate-release-version.mjs --tag v<新版本>` PASS；旧 tag FAIL（负向）。
- AC3: `--check` 在不一致时非零退出；`--dry-run` 不写文件。
- AC4: 配套 `bump-version.test.mjs`（临时目录 fixture，不碰真实仓库）。
- AC5: CI 的 scripts job 加入 `--check` 调用（属 R015 增强，需独立 Task 确认）。

### 3.4 边界与风险

- 不触碰：`macos/release/*.dmg`、`windows/release/*`（构建产物）、`tailsync-update.json`（生成物）、site 独立发布节奏（#7/#12 可一次同 bump，但 site 可独立发版——设计上不强制）。
- 风险：Cargo.lock 同步若引入依赖变更（理论上 generate-lockfile 只刷新根包版本），需 diff 检查确认无依赖漂移。
- UNKNOWN: 未来是否将三个 crate 收敛为 workspace（会改变 lock 结构，本设计不依赖此变化）。

## 4. 实现归属

- T003 仅设计。实现 = 独立 Task（建议挂在 Phase 0 之后、Phase 1 之前作为 T004 或并入 Phase 3 末尾），需满足 §18 完成标准与 R015（CI 增强需单独确认）。

## 5. 实现状态（T355，2026-08-15）

- **`scripts/bump-version.mjs` 已实现**（候选 B，Node 脚本，与 validator 同栈）：
  - `node scripts/bump-version.mjs --target X.Y.Z [--root PATH]` —— 写 12 个文件（JSON 解析重写保序 + Cargo.toml [package] 行级替换 + Cargo.lock [[package]] 块级定向替换，不触碰依赖解析）+ 自校验（复用 validate-release-version.mjs）。
  - `--check` —— 只校验（validator + 锁文件一致性 grep），CI 可复用；不一致非零退出。
  - `--dry-run` —— 只报告将写入的文件数，零写入。
  - `--target` 语义版本校验复用 validator 的 tag 正则（`^v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$`）。
- **验收实测（全部 PASS）**：
  - `bump-version.test.mjs` 4 用例（12 文件全写 + 幂等、dry-run 零写、verify 新旧版本正负、锁文件失同步检测）→ `node --test` 9/9（含既有 5 个）。
  - 真实仓库：`--check --target 2.1.0` PASS（stable channel）；`--check --target 2.2.0` FAIL（负向，exit 1）；`--dry-run --target 2.2.0` 报告 12 文件、`git status` 12 个版本文件零改动。
- **R015 边界**：CI scripts job 增加 `--check` 调用属 CI 增强，需独立 Task 确认（本任务未动 CI）。
