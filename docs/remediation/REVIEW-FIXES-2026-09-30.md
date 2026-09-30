# 2026-09-30 复核缺陷的修复

原提交基线：`f35baf6be0e2aadab677a75d22b14223f20fd24f`；对照的 `origin/main` 为 `b61a0a04b88f5828707f7ef64781389944471be8`。修复位于原 `codex/remediation-2026-09-29` 工作树，未涉及其他检出的未提交内容。本文件记录本次修复，不能替代原生 Windows、托管 CI 或双机验收。

## 已改动的问题

| 问题 | 修复与保护 |
|---|---|
| 三处 `unnecessary_to_owned` 阻断 Clippy | BLAKE3 十六进制摘要直接借用字符串，去掉冗余分配；Core 全目标严格 Clippy 本机通过 |
| GC 按路径拼写对账会删除仍被引用的大小写别名 | 以卷与文件 ID 对账（Unix dev/inode、Windows FILE_ID_INFO）；先完整解析存活引用，错误时中止清扫；候选身份读取失败时保留文件 |
| 台账把编译、空作业或普通函数当成门禁 | 校验测试属性、非忽略状态、实际执行命令与非空作业；跟随 include! 时匹配完整解析路径 |
| 台账不验证测试真的执行 | 原有 Cargo/Swift/打包步骤只运行一次，包装器记录退出码、通过/失败/跳过、源码 SHA/内容摘要和日志；每个平台作业收尾核验实际通过结果，失败/跳过/缺失/旧源码不能认证；打包 P0 必须有运行时断言成功标记 |
| Windows 发现门禁只证明全局广播常量存在 | 生产过滤和发送路径可注入；用明确网卡夹具和本机 UDP 覆盖 down/loopback/点对点过滤、成功发送但零设备、全部发送失败，保持现有返回接口 |
| v9 所谓短重试实际继承 5s SQLite 超时 | 临时设置每次忙等待 50ms，加四次 50ms 间隔，成功/失败后恢复原超时；完整迁移测试覆盖 pending 状态与释放读者后重开恢复 |
| 可观测性只停留在计划与过期文档 | 每日/手动工作流采集 Core/runtime 的 LCOV、JSON 和源码 SHA；不增加 PR 插桩成本。耗时采样限定 ci.yml 已完成运行；同步执行报告、关键路径清单与验收环境分类 |

执行器/台账源文件或台账条目改变时，scope planner 会选中两端原生作业，避免门禁配置变了却只执行文档检查。纯文档仍保留轻量路由。

## 先红后绿证据

- `orphan_sweep_preserves_reused_payloads_with_case_aliases`：旧实现删除仍可读取的载荷，测试失败；修复后通过。在大小写敏感文件系统上同时保留正常孤儿删除行为，不将所有旧文件豁免。
- `v9_busy_migration_keeps_pending_state_and_completes_on_reopen`：旧实现等待约 **26.2s**，超过 2s 回归上限；修复后完整 v9 相关三项测试合计约 **0.61s**。约 450ms 是重试锁等待预算，不能解读成整个迁移的实时承诺。
- Windows 发现两项测试已在 macOS 主机编译运行。新 Windows 文件身份代码和 Windows 专属网卡字段另用 `x86_64-pc-windows-msvc` 最小探针成功类型检查；这不等同 Windows 成品运行。
- 脚本回归覆盖：仅编译/列举、普通函数、忽略测试、无 CI 作业、未执行/跳过/失败、旧源码记录，以及没有端口断言成功标记的打包，均被拒绝。
- 插桩首次并行运行有两项 Iroh 夹具因 5s 配对窗口关闭而失败；未过滤或忽略测试。覆盖率任务改为串行后，Core 453 项和 runtime 23 项通过并导出 LCOV。PR 常规并行 Core 测试也已通过；串行设置仅用于额外的覆盖率任务。

**本地核对注意**：在 macOS 上对 Windows crate 跑 `cargo clippy --all-targets -- -D warnings` 会因 `#[cfg(windows)]` 分支不存在而报 `clipboard_file.rs` 的 dead-code 错误（6 个），这是交叉编译产物、不是缺陷；该步骤必须在 `windows-latest` 上跑（CI 的 `Run Windows application Clippy` 于 `e955128` 通过）。

本机临时日志位于 `/tmp/tailsync-fix-*.log`，属于本次调试证据；持久的具名测试和 CI 产物才是后续回归依据。历史 P0 故障注入记录仍见 `docs/acceptance/s6-p0-1-mutation-proof.md`，不能冒充本次改动后的 CI 结果。

## 本机复现与 CI 证据

先安装 Windows 前端锁定依赖并构建真实资源，再运行 Windows crate 的主机测试：

```bash
npm ci --prefix windows
npm run build --prefix windows
cargo test --locked --manifest-path windows/src-tauri/Cargo.toml --lib
cargo clippy --locked --manifest-path shared/rust-core/Cargo.toml --all-targets -- -D warnings
node --test scripts/*.test.mjs
node scripts/check-remediation-ledger.mjs
```

按 macOS 作业收集本版本的门禁结果，四个既有套件各执行一次。生成证据期间不要编辑源码，结果目录应使用默认忽略路径或仓库外目录：

```bash
node scripts/run-remediation-tests.mjs --job rust-macos -- cargo test --locked --manifest-path shared/rust-core/Cargo.toml
node scripts/run-remediation-tests.mjs --job rust-macos -- cargo test --locked --manifest-path shared/tailsync-runtime/Cargo.toml
node scripts/run-remediation-tests.mjs --job rust-macos -- cargo test --locked --manifest-path macos/src-tauri/Cargo.toml --all-targets
TAILSYNC_WEB_SVG_RENDER_TESTS=1 node scripts/run-remediation-tests.mjs --job rust-macos -- swift test --package-path macos/swift-ui
node scripts/check-remediation-ledger.mjs --job rust-macos --results-dir .remediation-test-results
```

CI 上传 `remediation-tests-macos` / `remediation-tests-windows`（原始日志与 JSON，保留 30 天）。覆盖率工作流 `coverage.yml` 每日/手动独立运行，固定 cargo-llvm-cov 0.6.21 和 Rust 1.91.0，上传 `shared-rust-coverage`。`report` 仅导出同一次采集的 JSON，不重复运行测试；尚未覆盖 Swift/前端与 Windows 条件分支，也未设全仓库百分比门槛。

## 保留的未完成项

- `S1-P1-5` 回退为 **partial**，取得本次原生 rust-windows 通过证据后才更新状态；断网/VPN 真机验收仍单独待验。
- `S5-P2-1` 仍为 **partial**：本次修的是 GC 安全性，生产周期调用未接线。
- 配额并发双批次准入、配对状态机、通知、性能预算与 UI/runtime 后续阶段未在本次扩大实施。
- Windows NSIS/成品 smoke、VoiceOver 和双机互通证据仍待取得。PR #68 仍是独立 PR，其成功检查不能认证本分支。

## 首次推送后的 CI 结果（run 36661666391，SHA 629b4ec）

推送后完整 CI 暴露了本文件未覆盖的两处缺陷，均已修复：

1. **执行对账在真实 CI 中永远无法通过。** 源码指纹取 `git ls-files --cached --others --exclude-standard`，**包含未跟踪且未忽略的文件**；Windows 打包步骤会创建这类文件，于是打包之前写下的记录全部被判过期。产物证据：核心记录指纹 `52967a8def`，打包后变为 `d0622ea9e3`，三个 rust-windows 记录全部报 `failed, stale or malformed`。修复：指纹只取**已跟踪**文件（仍能发现运行期修改受版本控制源文件）；`macos/build` 未进 `.gitignore` 也因此不再重要。回归测试 `sourceIdentity ignores untracked artifacts but tracks tracked content`。
2. **v9 迁移测试用墙钟断言，在 CI 上抖动。** `elapsed < 2s` 本地 0.54s 通过、GitHub 运行器上 2.48s 失败（两个平台都挂，Windows 因此级联到对账步骤）。修复：把每次忙等待与次数提为 `pub(crate)` 常量并**确定性地**断言其上界，端到端断言放宽到 15s（仍能抓住"五次各等 5s≈25s"的原始失效模式）。

本机按文档跑完整 macOS 对账流程（runtime / core / macOS 应用 / Swift 四个套件各一次 + 校验）已通过：`remediation ledger OK: 31 entries`。

### 后续三轮 CI 暴露的第三个缺陷与最终结果

修复上述两项后 Windows 仍失败，错误为「92 个门禁命令未配置」：`parseCiJobs` 只按 `\n` 切分，而 Windows 检出是 CRLF，残留的 `\r` 使每条 `run:` 正则都不匹配。已改为按 `/\r?\n/` 切分并补 CRLF 回归夹具。

最终 **完整双平台 CI 全绿**：run [36669221251](https://github.com/monet4070/TailSync/actions/runs/36669221251)（`bde8222`，`workflow_dispatch`，16m37s），11 个 job 全部 success，含两个打包 job 与 `Required verification`。

**补充（本分支后续提交）**：`S5-P2-1` 的周期调用已接线（`run_expired_transfer_maintenance` 接收数据库句柄，每个 tick 先清过期传输、再以 1 小时宽限清扫孤儿载荷），并有门禁 `transfer_maintenance_tick_sweeps_aged_orphaned_payloads`（变异验证：把宽限期改到远超夹具年龄即失败）。台账更新为 **fixed_gated 22 / partial 3 / unfixed 5 / needs_adjudication 1**。

台账状态：**fixed_gated 21 / fixed_ungated 0 / partial 4 / unfixed 5 / needs_adjudication 1**，共 31 项。原报告中过期的“workflow scope 阻塞”“阶段 5 未实施”和“11 个 job 全绿”已更正；覆盖率首份托管产物仍待工作流上线验证。
