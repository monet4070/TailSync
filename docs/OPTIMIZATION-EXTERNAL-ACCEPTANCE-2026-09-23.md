# TailSync 外部验收与未闭环修复报告（2026-09-23）

## 结论与边界

**公开发布：NO-GO。** 被验收的产品源码提交 `ba7bbd5701b176c3179bbffbd0fcbb9e061e4edf` 的隔离旧版历史迁移缺陷已红/绿复现并通过完整本机回归；该源码提交的 Windows 无签名 development 包构建、隔离 portable/deep-link smoke、运行中与退出后的 TCP 19889 空端口，以及系统级忙碌文件剪贴板读取三档已通过。后续提交本报告只会使仓库 Git HEAD 前进，**不会改变安装包清单中的 `sourceCommit=ba7bbd5`，也不代表重新构建**。签名、干净安装/升级/卸载、真实双/三设备、产品剪贴板监视器端到端广播、产品进程崩溃矩阵和 WebView2 长时内存验收仍无证据。用户确认本轮没有干净 Windows 环境或第二台设备；这些项保持 `BLOCKED`，不得以本机测试替代。

本报告仅给实际运行的检查标记 `PASS`。早前回归日志目录为 `audit/external-acceptance-2026-09-23/run-20260923-100129/`（下文 `R/`）；更早旧 HEAD 的打包目录为 `audit/external-acceptance-2026-09-23/run-20260923-101547/`（下文 `P/`）。身份修复 HEAD 的打包目录为 `audit/frozen-spec-identity-race-2026-09-23/run-20260923-113636/`（下文 `Q/`），该 HEAD 的合成基线目录为 `audit/frozen-spec-local-baseline-2026-09-23/run-20260923-114158/`（下文 `B/`）。剪贴板测试目录为 `audit/frozen-spec-clipboard-2026-09-23/run-20260923-120000/`（下文 `T/`），上一 HEAD 打包目录为 `audit/frozen-spec-clipboard-2026-09-23/run-20260923-121000/`（下文 `C/`）；迁移隔离修复的证据目录为 `audit/frozen-spec-legacy-isolation-2026-09-23/run-20260923-122000/`（下文 `L/`），**当前 HEAD 打包目录**为 `audit/frozen-spec-legacy-isolation-2026-09-23/run-20260923-123000/`（下文 `D/`）。执行时间均为 2026-09-23，Asia/Pyongyang（UTC+09:00）；逐命令起止时间见各 `*-validation.json`。无本轮产品 PDF 预览截图；旧的合成 PDF 渲染截图单独标注，不充作 WebView2 证据。

## 构建身份、起始状态与隔离数据

| 项目 | 记录 |
|---|---|
| 仓库/分支 | `F:\TailSync\TailSync-2.3.0\TailSync` / `codex/external-acceptance-2026-09-22` |
| 开始时 HEAD | `06c86720008bb1f5a83d50647f74e88322877c11`，`feat(ipc): add stable error contract foundation`；开始时已运行 `git status --short`、`git branch --show-current`、`git log -1` |
| 本轮修复/当前 HEAD | `ba7bbd5701b176c3179bbffbd0fcbb9e061e4edf`，`fix(storage): isolate implicit legacy migration from data overrides`；此前剪贴板夹具 `23bb3c9`、身份修复 `61b6221`、基线/IPC 提交 `a5f5dcc`、`4310992`，早前修复 `d1d262a`、`a7791f0` |
| 产品/协议/数据库 | `2.2.2` / wire `v4` / DB schema `v11` |
| 当前构建清单 | `D/packages/TailSync-2.2.2-Windows-x64-build.json`；`sourceCommit=ba7bbd5...`；`sourceDirty=true`；`builtAtUtc=2026-09-23T03:31:50.8104386Z`。打包前占用中的其他 TailSync 进程已由设备所有者正常退出 |
| 工具版本 | Rust/Cargo `1.91.0`；Node `v24.18.0`；npm `11.16.0`；Tauri CLI `2.11.4`；PowerShell `7.6.5`；Git `2.47.0.windows.2`；详见 `P/environment.json` 与构建清单 |
| 签名环境 | `TAURI_SIGNING_PRIVATE_KEY`、`TAILSYNC_SIGNING_PRIVATE_KEY`、Windows 证书 thumbprint 均未设置；仅记录布尔值，未打印秘密 |
| 源码工作区 | 既有 `windows/src-tauri/gen/schemas/windows-schema.json` 修改保持原样；`audit/`、`windows/audit/` 和旧报告/计划为未跟踪审计文件。没有使用 `git reset --hard`、`git clean` 或 `git checkout --` |

已阅读 `CONTEXT.md`、`docs/OPTIMIZATION-IMPLEMENTATION-STATUS-2026-09-06.md` 和用户提供的冻结规范附件。仓库中不存在指定的 `docs/OPTIMIZATION-PROPOSALS-2026-09.md`，该输入记为 `BLOCKED`，没有臆测其内容。独立合成素材在 `audit/external-acceptance-2026-09-22/run-20260922-212714/test-data/`；当前包 portable 运行使用 `D/runtime-port-test-data/` 的独立测试身份，未读取或打印其私钥。此目录仍留在本机作审计，含仅用于测试的身份文件，不应上传公开制品。此前 PDF 产品尝试在修复前虽然设置了隔离数据目录，仍触发了对账户旧历史路径的探测；没有证据表明导入了真实历史，但该隔离缺陷已按下文修复，不能把修复前的会话称为完全隔离。

## 本轮确定性缺陷与最小修复

最小复现先使 Rust 和 TypeScript 测试失败（命令与断言记录于 `R/minimal-reproduction.md`；初始红测输出只在任务终端，未伪称有独立原始日志）：未知未来错误码虽然被解码为 `internal_error`，但来自未知 envelope 的 `retryable=true` 和 `message_key=future.error` 被保留。旧客户端因此可能错误重试或显示未定义文案。提交 `a7791f0` 仅修改稳定错误 envelope 的 Rust 反序列化器、TypeScript/JavaScript 与 Swift 生成器和相应回归测试，未知码现在采用完整的 `internal_error` 固定策略（`retryable=false`、`message_key=error.internal`、`detail_class=internal`）。Rust/TypeScript 回归通过；Swift 测试因本机没有 Swift 工具链保持 `BLOCKED`。

另一个最小复现为 `cargo clippy --manifest-path macos/src-tauri/Cargo.toml --all-targets -- -D warnings` 在 Windows host 失败，指出 `ClipboardRuntime::Headless` 未构造（`R/macos-clippy.log`）。提交 `d1d262a` 只给该变体增加非 macOS 条件下的 dead-code 允许属性，不改变运行时逻辑。修复后 macOS Rust clippy、76 项 macOS Rust 测试、Windows 95 项测试和跨平台契约均通过（`R/*-after-fix.log`）。

| 命令（工作目录为仓库根，另注明者除外） | 本轮时间 | 结果 | 日志 |
|---|---|---|---|
| `node shared/schema/generate-local-contracts.mjs --check` | 10:02:04–10:02:10 | `PASS`，27 个 DTO | `R/schema-check.log` |
| `cargo test -p tailsync-runtime` | 10:01:45–10:01:46 | `PASS`，20 项 | `R/runtime-test.log` |
| `npm test`（`windows/`） | 10:01:45–10:01:52 | `PASS`，41 文件、222 项 | `R/frontend-test.log` |
| `cargo fmt --all -- --check`；`git diff --check` | 10:03 前 | `PASS`；Git 仅有换行符提示 | 终端结果；无独立日志 |

## 本机门禁与打包

以下 `R/` 日志保存了原始输出；这些结果证明本机自动化检查，不证明外部设备或安装生命周期。

| 命令 | 本轮时间 | 状态/摘要 | 日志 |
|---|---|---|---|
| `./windows/scripts/check_cross_platform_sync.ps1 -WinRoot (Resolve-Path windows).Path -MacRoot (Resolve-Path macos).Path` | 10:01:45 | `PASS`，60 个 Swift API 命令等静态契约 | `R/cross-platform.log` |
| `node scripts/check-local-contracts.mjs --root .` | 10:06:25–10:06:28 | `PASS`，schema v1 / wire v4 | `R/local-contracts.log` |
| `node scripts/check-shared-resolution.mjs --root .` | 10:06:25–10:06:29 | `PASS`，非阻断 feature 差异保留在日志 | `R/shared-resolution.log` |
| `node --test scripts/*.test.mjs` | 10:06:25–10:06:28 | `PASS`，52 passed、1 skipped（不声称跳过项通过） | `R/script-tests.log` |
| `cargo test --manifest-path shared/rust-core/Cargo.toml -- --test-threads=1` | 10:04:33–10:05:12 | `PASS`，389 passed、1 ignored；未启用注入 feature 的崩溃矩阵不会在这里运行 | `R/core-test.log` |
| `cargo test --manifest-path shared/rust-core/Cargo.toml --features acceptance-injection --test received_batch_crash_matrix -- --test-threads=1` | 10:05:36–10:05:55 | `PASS`，六阶段独立子进程退出码 70 后恢复；1 个矩阵测试通过、worker 测试按设计 ignored | `R/crash-matrix.log` |
| `cargo check --manifest-path windows/src-tauri/Cargo.toml --all-targets` | 10:02:04–10:03:03 | `PASS` | `R/windows-check.log` |
| `cargo test --manifest-path windows/src-tauri/Cargo.toml --lib -- --test-threads=1` | 10:02:04–10:03:00 | `PASS`，95 项 | `R/windows-test.log` |
| `cargo clippy --manifest-path windows/src-tauri/Cargo.toml --all-targets -- -D warnings` | 10:02:04–10:02:24 | `PASS` | `R/windows-clippy.log` |
| `npm run build`（`windows/`） | 10:02:04–10:02:10 | `PASS` | `R/frontend-build.log` |
| `npm run lint`（`windows/`） | 10:02:04–10:02:05 | `PASS`，有既有 React warnings，并非零警告 | `R/frontend-lint.log` |
| `cargo test --manifest-path macos/src-tauri/Cargo.toml --lib -- --test-threads=1` | 10:06:07–10:06:47 | `PASS`，Windows host 上的 76 项 Rust 测试；不代表 macOS 产品运行 | `R/macos-rust-test.log` |
| `cargo clippy --manifest-path macos/src-tauri/Cargo.toml --all-targets -- -D warnings` | 首次 10:08:19–10:08:31；修复后 10:15 | 首次 `FAIL` 为最小复现；修复后 `PASS` | `R/macos-clippy.log`、`R/macos-clippy-after-fix.log` |
| `cargo test --manifest-path macos/src-tauri/Cargo.toml --lib -- --test-threads=1`；同命令重跑 Windows Rust 95 项及跨平台检查 | 10:15 | `PASS`，macOS Rust 76、Windows Rust 95、跨平台静态契约 | `R/macos-rust-test-after-fix.log`、`R/windows-test-after-fix.log`、`R/cross-platform-after-fix.log` |
| `Get-Command swift,swiftc` | 10:22 | `BLOCKED`，本机没有 Swift/Swiftc；Swift 生成解码器的测试必须在 macOS CI 运行 | `P/environment.json` |

已先阅读 `windows/package.json` 与 `windows/scripts/package-windows.ps1` 的参数。无签名密钥时，使用仓库支持的 `--no-sign` development 路径；**没有**传入 `SkipChecks` 或 `SkipSmokeTest`。当前 HEAD `ba7bbd5` 的打包命令（12:27:38–12:31:55，退出码 0，见 `D/package-validation.json`）为：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File windows/scripts/package-windows.ps1 `
  -OutputDirectory 'F:\TailSync\TailSync-2.3.0\TailSync\audit\frozen-spec-legacy-isolation-2026-09-23\run-20260923-123000\packages' `
  -BuildDirectory 'target-package-frozen-20260923'
```

原始日志 `D/package-windows.log`；脚本重复执行了跨平台契约、lint、前端测试/构建、Windows Rust 测试，生成 NSIS，并通过独立 portable 启动及 remote-pairing deep-link 单实例 smoke。`C/packages/`、`Q/packages/`、`P/packages/`、`R/packages/` 及 `audit/frozen-spec-local-baseline-2026-09-23/run-20260923-111709/packages/` 均是旧提交的历史产物。当前 HEAD 的产物（`Get-FileHash -Algorithm SHA256` 与清单及 `.sha256` 一致；`Get-AuthenticodeSignature` 独立核对见 `D/artifact-validation.json`）：

| 当前 HEAD 产物（均位于 `D/packages/`） | 字节 | SHA-256 | 状态 |
|---|---:|---|---|
| `TailSync-2.2.2-Windows-x64-portable.exe` | 27,681,792 | `bf0a6d7caaca96555c6f39afc7db01032d6e17a7e5bb61a9448182a372048f04` | `NotSigned`；构建/smoke `PASS` |
| `TailSync-2.2.2-Windows-x64-setup.exe` | 7,759,319 | `ec3dc397e5adf33bd95391272fbe6236b8a4326ab7bc291a9ecf591e54f31014` | `NotSigned`；生成 `PASS`，安装未执行 |
| `TailSync-2.2.2-Windows-x64-build.json` | 969 | `167a7a532f207863fee2f0fc36bba76aecef305df9bda84c5868449d37547b4d` | 构建身份记录 |
| `TailSync-2.2.2-Windows-x64.sha256` | 211 | `d639f07b442faf37d9140e9036ef8850230da63672557710eea1e91704307e83` | 产物校验清单 |

正式 community updater 签名与 trusted Authenticode 打包均为 `BLOCKED`，没有伪造密钥。清单 `sourceDirty=true`，因为原有 schema 修改与审计目录仍在工作区；本包不可冒充干净签名发布制品。

当前 HEAD 安装包绝对路径：`F:\TailSync\TailSync-2.3.0\TailSync\audit\frozen-spec-legacy-isolation-2026-09-23\run-20260923-123000\packages\TailSync-2.2.2-Windows-x64-setup.exe`。上述 `D/artifact-validation.json` 同时记录了签名状态、smoke 后与 portable 存活期的空端口计数及隔离日志检查；**仅安装包生成通过，安装态仍 `BLOCKED`**。

## 端口、安装包和忙碌剪贴板

先前运行的 `M:\TailSync\tailsync.exe` 已由用户正常退出；本轮未终止该用户进程。当前 HEAD 打包 smoke 后执行 `Get-NetTCPConnection -LocalPort 19889 -ErrorAction SilentlyContinue` 返回空，见 `D/artifact-validation.json`（12:33:06）。同一 portable SHA 在独立测试身份下存活期间，进程 PID 58204 存活且该命令仍返回空；测试脚本随后仅强制停止自己启动的进程，不算“正常退出”验收。隔离 portable 日志中 `TailSync_History` 命中数为 0。当前**安装态**运行时端口仍待干净环境验证。

| 外部安装包项目 | 验证命令/方式 | 状态与理由 | 证据 |
|---|---|---|---|
| 干净账户/VM 安装 | 在可销毁环境运行上述 `setup.exe` 并确认安装目录/应用入口 | `BLOCKED`，用户确认本轮无此环境 | 无截图 |
| portable 启动 | `package-windows.ps1` 的 portable 启动与 deep-link smoke | `PASS`，**仅 portable smoke** | `D/package-windows.log` |
| 已安装应用启动 | 干净账户中从安装入口启动并记录进程/窗口 | `BLOCKED`，安装未执行 | 无截图 |
| 正常退出 | 安装态从 UI/托盘退出并确认进程消失 | `BLOCKED`；本轮只由脚本清理自己启动的进程 | 无截图 |
| 升级 | N-1 安装态运行当前 `setup.exe`，比较版本及保留的合成数据 | `BLOCKED`，无可追溯 N-1 安装态 | 无日志 |
| 卸载 | 测试账户通过系统卸载入口移除并核对残留 | `BLOCKED`，未在干净环境安装 | 无日志 |
| 重启恢复 | 安装后重启测试 VM，核对进程、数据和连接 | `BLOCKED`，无可重启环境 | 无日志 |
| 开机启动注册 | 安装态切换 autostart，读取该测试账户注册状态，重启后核对 | `BLOCKED`；未更改真实账户注册表 | 无日志 |
| 当前包 smoke 结束后 19889 TCP 为空 | `Get-NetTCPConnection -LocalPort 19889 -ErrorAction SilentlyContinue` | `PASS`，空结果；不是运行时测量 | `D/artifact-validation.json` |
| 当前包 portable 存活期间 19889 TCP 为空 | `pwsh -File D/verify-package.ps1` 启动同 SHA portable 后执行上述命令 | `PASS`，进程存活、空结果；仅 portable | `D/artifact-validation.json` |
| 已安装应用运行时 19889 TCP 为空 | 在干净账户安装并启动后执行同一命令 | `BLOCKED`，安装未执行 | 无日志 |
| 忙碌文件剪贴板读取 40ms | 独立隐藏窗口 owner 持有合成 `CF_HDROP`，运行下面的忽略测试 20 次 | `PASS`，20/20 文件类型，均共尝试 4 次，70–71ms | `T/clipboard-system-test-20x.log`、`T/clipboard-summary.json` |
| 忙碌文件剪贴板读取 200ms | 同上，20 次 | `PASS`，20/20 文件类型，均共尝试 6 次，270–271ms | 同上 |
| 忙碌文件剪贴板读取 500ms | 同上，20 次 | `PASS`，20/20 文件类型，均共尝试 8 次，631–633ms | 同上 |
| 剪贴板占用超过预算 900ms | 同一 owner 持有 900ms | `PASS`，第 9 次尝试后 831ms 返回类型化 `Busy`，未读取文件/文本 | 同上 |
| 产品监视器端到端不广播文本路径 | 在隔离产品会话中观察文件事件与文本广播 | `BLOCKED`；上述读取层测试与代码分支不能替代完整广播链路 | 无产品日志/截图 |

上述测试由用户明确授权覆盖当前账户剪贴板，未读取或保存原内容；夹具退出时清空合成 `CF_HDROP` 并删除合成文件。执行命令（`TAILSYNC_CLIPBOARD_TEST_DIR` 指向 `T/`，起止时间与退出码见 `T/run-20x-metadata.json`）：

```powershell
$env:TAILSYNC_ALLOW_CLIPBOARD_OVERWRITE = '1'
$env:TAILSYNC_CLIPBOARD_TEST_DIR = 'F:\TailSync\TailSync-2.3.0\TailSync\audit\frozen-spec-clipboard-2026-09-23\run-20260923-120000'
cargo test --locked --manifest-path windows/src-tauri/Cargo.toml --lib clipboard_file::tests::busy_file_clipboard_system_acceptance -- --ignored --exact --nocapture --test-threads=1
```

夹具开发的两次失败保留为 `T/clipboard-system-test.log` 与 `T/clipboard-system-test-isolated-owner.log`，均是未正确持锁的测试 owner 失败，不能写成产品缺陷或验收 PASS；修成独立隐藏窗口后先以每档 1 次调试通过，见 `T/clipboard-system-test-hidden-window-debug.log`，再执行上述 20 次。夹具提交 `23bb3c9` 只增测试专用计数器/忽略测试与 `windows/scripts/clipboard-owner-test.ps1`，不改变 release 的读取逻辑。完整回归命令、起止时间和日志见 `T/validation.json`：Windows Rust check/test/clippy、前端 225 测试/构建/lint、跨平台检查均 `PASS`；正常测试中的系统级项目为 ignored，只有显式 opt-in 的 20 次运行才算该项目证据。

## 本轮确认缺陷：隔离目录仍隐式探测账户旧历史

修复前的隔离 PDF 尝试日志显示，即使设置 `TAILSYNC_DATA_DIR`，旧版迁移仍检查账户的 `TailSync_History`。该会话因缺 Fernet key 警告，没有证据表明真实历史被导入；然而隐式访问本身违反独立测试数据边界。`L/` 的最小复现只构造合成 `HOME`/`USERPROFILE`、合成 Fernet key 和一行合成旧历史：修复前测试退出码 101，确实导入该行；修复后同命令退出码 0，不再隐式迁移。另一个同目录回归确认显式 `TAILSYNC_V1_DATA_DIR` 仍可迁移那一行合成数据。提交 `ba7bbd5` 只修改 `shared/rust-core/src/db/legacy_v1.rs` 与新增 `shared/rust-core/tests/legacy_data_isolation.rs`：有明确旧版源路径时优先使用它；没有明确源路径且覆写当前数据/存储目录时，禁止回退扫描账户旧历史。默认正式用户迁移路径未改。

| 命令/检查 | 时间（UTC+09:00） | 状态 | 证据 |
|---|---|---|---|
| `cargo test --locked --manifest-path shared/rust-core/Cargo.toml --test legacy_data_isolation overridden_data_dir_does_not_import_implicit_legacy_history -- --exact --nocapture`（修复前） | 12:21:38–12:21:43 | `FAIL`，预期红测，旧代码导入合成账户历史 | `L/reproduction-red.log`、`L/reproduction-red-metadata.json` |
| 同一命令（修复后） | 12:22:04–12:22:12 | `PASS`，隔离目录不隐式导入 | `L/reproduction-green.log`、`L/reproduction-green-metadata.json` |
| 同一命令（显式旧版迁移源回归） | 12:22:41–12:22:45 | `PASS`，合成显式源仍导入一行 | `L/explicit-source-regression.log`、`L/explicit-source-regression-metadata.json` |
| `cargo fmt --all -- --check`；Core test/clippy；Runtime、Themes test；Windows Rust check/test/clippy；macOS Rust test/clippy（Windows host）；跨平台契约；前端 test/build/lint | 12:23:40–12:25:46 | `PASS`，各命令退出码 0；Core 390 项、1 ignored，新集成测试 1 项、worker ignored；Windows 95 项、1 ignored；前端 225 项。Swift/真实 macOS 仍 `BLOCKED` | `L/validation.json` 与同目录各 `.log` |
| `pwsh -NoProfile -ExecutionPolicy Bypass -File D/verify-package.ps1`：校验当前包清单、SHA-256、签名、smoke 后及运行时端口、隔离日志 | 12:33:06–12:33:12 | `PASS`，同 SHA portable 存活，TCP 19889 空；隔离日志对 `TailSync_History` 匹配 0 | `D/verify-package.ps1`、`D/artifact-validation.json`、`D/runtime-port-test-data/tailsync.log` |

`D/runtime-port-test-data/` 含本机合成身份及密钥；审计归档时不得公开上传。脚本只停止自己启动的 portable；没有把该行为计为安装态正常退出。产物清单因既有 schema 修改和审计文件而标为 `sourceDirty=true`，所以这不是干净签名发布包。

## 真实双/三设备验收

用户确认本轮没有第二台设备；下表项目均**未执行**，统一为 `BLOCKED`。测试命令应在独立设备的同一 commit/安装包上记录实际连接日志、路径、时间与截图；不能把同 host Noise/网络单测写为真实路径 PASS。

| 必测场景/执行方式 | 状态 |
|---|---|
| 双向文本复制，两端核对文字与事件日志 | `BLOCKED` |
| 双向图片复制，两端核对像素/哈希与事件日志 | `BLOCKED` |
| 双向文件复制，两端核对内容哈希与文件数 | `BLOCKED` |
| 两端同时发起 TCP 连接至少 100 次，核对 glare 仲裁 | `BLOCKED`（本机单元模拟不替代） |
| 配对失败后读取两端封禁计数，确认不误增 | `BLOCKED` |
| 网络断开/恢复、睡眠/唤醒后核对续传和状态 | `BLOCKED` |
| 可追溯 N-1 与当前版本互通，核对 v4 fallback | `BLOCKED` |
| 物理剪贴板回音抑制及用户再次复制相同内容 | `BLOCKED` |
| LAN 直连的连接耗时、RTT、吞吐 | `BLOCKED` |
| Tailscale 路径的连接耗时、RTT、吞吐 | `BLOCKED` |
| Iroh direct 路径的连接耗时、RTT、吞吐 | `BLOCKED` |
| Iroh relay 路径的连接耗时、RTT、吞吐 | `BLOCKED` |
| 大文件传输，接收哈希/吞吐/内存 | `BLOCKED` |
| 重复哈希内容与多 Peer（至少第三设备） | `BLOCKED` |

## 崩溃、维护与 PDF

`ReceivedBatchCommitState` 显式阶段及 debug-only `acceptance-injection` 已在 core 实现。`R/crash-matrix.log` 证明 `hash_verified`、`clipboard_staged`、`history_persisted`、`receipt_persisted`、`acked`、`cleaned` 六个阶段各发生实际子进程退出（码 70）并重启恢复；测试断言 sidecar 状态、history/receipt、ACK 次数和清理。状态为 **core 注入矩阵 `PASS`**，但 Windows/macOS **产品进程**每阶段终止、真实持久化目录和重启恢复仍 `BLOCKED`。`.part` 偏移恢复、过期文件进入维护队列、重复提交语义有 core 单测支持（`R/core-test.log`），没有真实产品长期维护或终止矩阵证据，外部验收仍 `BLOCKED`。

| 项目/命令或执行方式 | 状态 | 证据 |
|---|---|---|
| 六阶段 core 子进程终止/恢复：`cargo test --manifest-path shared/rust-core/Cargo.toml --features acceptance-injection --test received_batch_crash_matrix -- --test-threads=1` | `PASS` | `R/crash-matrix.log` |
| Windows/macOS 产品进程每阶段终止、重启，验证 manifest/ACK/清理顺序 | `BLOCKED`，现有注入点只在 core debug feature；未建产品进程验收 harness | 无产品日志 |
| 产品进程重启后 `.part` 恢复 | `BLOCKED`，core 单测不能替代 | `R/core-test.log` 仅作单测证据 |
| 过期断点进入维护队列并最终清理 | `BLOCKED`，无产品长时测试/时间注入 | `R/core-test.log` 仅作单测证据 |
| 各阶段重复提交后的历史唯一性和 ACK 语义 | `BLOCKED`，仅 core 矩阵有幂等断言 | `R/crash-matrix.log` 仅作 core 证据 |

旧 `PDFPageProxy.cleanup()` 在页面替换/卸载时调用，前端测试 `PASS`。100/500 页合成 PDF 位于 `audit/external-acceptance-2026-09-22/run-20260922-212714/test-data/pdfs/`；首尾页离线渲染截图位于 `.../screenshots/pdf-fixtures/`。这些只证明素材页数/离线渲染。曾以隔离 portable 打开空 History 窗口并在文件资源管理器复制合成 100 页 PDF；随后测试进程消失、窗口归属变为用户的 `M:\TailSync\tailsync.exe`，因此立即停止 UI 操作，未读取该窗口历史。过程见 `C/pdf-product-attempt.md`、`C/pdf-product-session.json`、`C/pdf-product-session-restart.json`，没有产品截图或 RSS 样本，也无法判断用户进程为何出现或是否观察到合成 PDF。WebView2 打开 100/500 页、连续翻页/缩放/跳页、进程树 RSS/页面数量/稳定后曲线及“不线性增长”结论均为 `BLOCKED`；不得把 jsdom、Poppler 或代码调用推断成内存验收。

| PDF 项目/执行方式 | 状态 | 证据 |
|---|---|---|
| `PdfPreview` 旧页 `cleanup()` 调用回归：`npm test`（`windows/`） | `PASS`，代码/模拟层 | `R/frontend-test.log` |
| WebView2 打开 100 页和 500 页 PDF | `BLOCKED` | 仅有离线素材/首尾页截图，无产品截图 |
| 连续翻页、缩放、跳页 | `BLOCKED` | 无产品操作日志 |
| 每秒采样 WebView2 子进程 RSS、页面数和稳定后曲线 | `BLOCKED` | 无 RSS 曲线/截图 |
| 验证长时间翻页内存不持续线性增长 | `BLOCKED`，没有前项曲线不能作结论 | 无证据 |

## 冻结规范 Phase 0A 增量（11:02–12:10）

本轮以两个原子提交推进单机基线：`a5f5dcc` 让隔离合成历史测试输出 p50/p95、SQLite 候选行数、候选条目声明的明文字节量、合成写入量和操作频率；`4310992` 在 Windows 唯一的 Tauri 调用边界加入显式 `VITE_TAILSYNC_DIAGNOSTICS=1` 才启用的有界内存计数，只保存命令名、次数、失败数和耗时，不保存参数、结果、剪贴板正文、路径或密钥。普通构建默认关闭。诊断构建可从该模块的 `getLocalIpcMetrics()` 取得快照；由于本轮没有隔离产品会话，**没有**把它的零实测值写成 IPC 基线。

前一 HEAD `61b6221cfbbbc22227be0841777b2b002f10a88e` 的历史基线命令及开始/结束时间、版本、退出码分别见 `B/run-metadata.json`。它是该提交的合成基线，不冒充当前 HEAD 重新采样；后续 `ba7bbd5` 修改了旧历史迁移源选择。原始输出见 `B/history-benchmark.log`，结构化原始样本见 `B/history-benchmark.json`。命令：

```powershell
$env:TAILSYNC_HISTORY_BASELINE_ROWS = '1000,10000,50000'
$env:TAILSYNC_HISTORY_BASELINE_ROUNDS = '5'
$env:TAILSYNC_PERFORMANCE_BASELINE_OUTPUT = 'F:\TailSync\TailSync-2.3.0\TailSync\audit\frozen-spec-local-baseline-2026-09-23\run-20260923-114158\history-benchmark.json'
cargo test --locked --manifest-path shared/rust-core/Cargo.toml db::tests::synthetic_history_performance_baseline -- --ignored --exact --nocapture
```

| 合成条目 | 首屏 p50/p95 ms | 命中 50 条 p50/p95 ms | 无匹配全扫 p50/p95 ms | 首屏/命中/全扫候选行 | 合成逻辑写入 |
|---:|---:|---:|---:|---:|---:|
| 1,000 | 1.97 / 2.41 | 29.30 / 31.49 | 34.42 / 35.58 | 128 / 896 / 1,000 | 1,000 行，1,088,512 明文字节 |
| 10,000 | 3.15 / 3.28 | 34.03 / 34.40 | 404.31 / 413.38 | 128 / 896 / 10,000 | 10,000 行，10,885,120 明文字节 |
| 50,000 | 9.36 / 9.57 | 58.86 / 63.46 | 3,368.78 / 3,408.39 | 128 / 896 / 50,000 | 50,000 行，54,425,600 明文字节 |

每组仅 5 轮；p95 为该组最大值，不是长期分布。候选字节是条目声明的明文大小之和，**不是**物理磁盘读取量；原始 JSON 还保存各轮逻辑字节、chunk 数和操作次数/秒。测试专用目录每轮创建并清理，不读取真实历史。早期采样（`run-20260923-110249/`、`run-20260923-110754/`、`run-20260923-112333/`）保留为历史证据，不与同 HEAD 的 `B/` 混用。

| 本轮命令/检查 | 结果 | 原始日志和起止时间 |
|---|---|---|
| `cargo test --locked --manifest-path shared/rust-core/Cargo.toml -- --test-threads=1`；Core clippy；Runtime/ Themes tests；Windows `cargo check/test/clippy`；跨平台检查 | `PASS`；Core 390 项、Windows 95 项 | `audit/frozen-spec-local-baseline-2026-09-23/run-20260923-110754/validation.json` 及同目录各 `.log` |
| `npm test`（Windows，诊断包装首次接线） | `FAIL`，3 个 History 测试发现无参命令被多传 `undefined` | `audit/frozen-spec-local-baseline-2026-09-23/run-20260923-111442/frontend-test.log`；同目录 `validation.json` |
| 修复无参调用形态并增加回归断言后的 `npm test`、`npm run build`、`npm run lint`、Windows Rust test、跨平台检查 | `PASS`，前端 225 项、Windows Rust 95 项；lint 有既有 React warnings | `audit/frozen-spec-local-baseline-2026-09-23/run-20260923-111601/validation.json` 及同目录各 `.log` |
| 当前 HEAD `package-windows.ps1`，不使用 `SkipChecks`/`SkipSmokeTest` | `PASS`，仅无签名本地打包与 portable smoke | `D/package-validation.json`、`D/package-windows.log` |

## 本轮确认缺陷：Windows 同进程并发设备身份初始化

在身份修复前的 HEAD 复跑 Core 全量测试时，`identity::tests::concurrent_initialization_converges_on_one_identity` 因 Windows `SetFileSecurityW` 的 `AccessDenied` 失败（`audit/frozen-spec-local-baseline-2026-09-23/run-20260923-112820/core-test.log`，389 通过、1 失败、1 忽略）。定向复现同一命令 30 次，5 次失败：`audit/frozen-spec-identity-race-2026-09-23/run-20260923-113008/`。首次尝试仅串行化目录/文件 ACL 操作后仍有 1/30 失败（`run-20260923-113111/`），所以该尝试**不算修复**，其改动已撤回。最终提交 `61b6221` 只在 Windows 将同进程的完整身份 load/create/restrict 流程串行化，保留跨进程 create-only hard-link 语义；30/30 和扩大后的 100/100 定向测试均通过（`run-20260923-113226/`、`run-20260923-113316/`）。

修复后 Core 全量 390 项、Runtime、Themes、Windows check/test/clippy、跨平台契约通过，逐命令起止时间、退出码和日志见 `audit/frozen-spec-identity-race-2026-09-23/run-20260923-113434/validation.json`。macOS Rust 在 Windows host 的 test/clippy 通过，见 `Q/macos-rust-validation.json`；这不代表 Swift 或真实 macOS 产品验收。当前 HEAD 的完整无签名打包见 `D/package-validation.json`。**跨进程**同时初始化、干净账户权限策略仍未专门验收，不能由同进程 100 次测试推断通过。

Phase 0A **仍未通过 Exit Gate**：连接耗时/吞吐、DB 锁等待 p50/p95、真实本地 IPC 频率、产品进程树内存曲线、慢迁移与重复哈希/解包分配基线尚未以隔离运行留证。现有 Runtime `db_lock_wait_ms` 调试事件和本轮 Windows IPC 计数器只是采集入口，不等于已采集的指标。SQLite 双连接的触发条件仍无法判定，保持单连接；不得据此调优 5 秒窗口保留时间或声称 Phase 4B 就绪。

## 尚未闭环的代码与人工交接

1. O08 本地稳定错误契约仍只完成基础层。`LocalCapabilities.current().supports_stable_errors=false`，Tauri/Swift 命令未全量迁移，Swift 生成测试未在 macOS 执行；不能因本轮未知码修复而宣称 O08 完成。
2. v4 认证握手已有能力协商基础，但 `file_sliding_window` 与 `image_compressed_chunks` 默认关闭，滑窗与压缩分片实现/互通/吞吐尚未完成；不得写成 wire v5 发布就绪。
3. `ReceivedBatchCommit` core 六阶段矩阵与按 dispatch 共享源校验已有实现及单测；仍需产品进程级 crash/restart、真实 `.part` 和维护队列验收。
4. macOS Rust clippy 的 Windows-host 失败已由 `d1d262a` 修复并复测 `PASS`；真实 macOS Swift 构建、测试、签名和 bundle smoke 尚未执行。
5. 需人工提供可销毁 Windows VM/测试账户、签名密钥与证书（只在受控 CI 注入）、真实第二台设备（多 Peer 第三台）、N-1 安装包、产品剪贴板监视器端到端观察环境和 WebView2 长时采样环境。用户已确认本轮 VM/第二设备不可用；Win32 忙碌剪贴板读取层 owner 已在本机完成，不再列为缺失夹具。
6. `R/runtime-port-test-data/` 留有仅用于本机审计的合成身份/数据库，不含真实历史；处理/归档时需避免公开其私钥或与正式用户数据混用。
7. 冻结规范 Commit 17/18 的滑窗文件传输、压缩分片图片仍未实现；Commit 20 的 Windows 暂态窗口保留时间仍是未按内存曲线调优的 5 秒。Phase 0A 的连接/吞吐、DB 锁等待、真实 IPC 频率和内存曲线仍缺基线；新增诊断入口不能代替实测。
8. 隔离目录仍探测账户旧历史的确定性缺陷已由 `ba7bbd5` 修复，当前包日志零命中；**干净账户下默认旧版迁移、显式旧版迁移的完整安装态流程**仍需按安装验收执行。此前修复前的 PDF 尝试不能追认为完全隔离会话。

上述外部证据在同一 SHA、同一签名产物和可追溯哈希下补齐以前，结论继续是 **NO-GO**。此前的详细执行顺序见 `docs/OPTIMIZATION-EXTERNAL-REMEDIATION-PLAN-2026-09-23.md`，但该计划前言关于“尚无 core 阶段注入/共享源校验”的旧状态已被本报告及提交 `01330fa`、`69489aa` 更新，不应照旧判读。
