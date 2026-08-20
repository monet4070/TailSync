# TailSync Maintainability Refactor — REGRESSION_BASELINE (T002)

> 生成时间：2026-08-15（本机）
> 基线提交：`1849419 fix(peer): preserve connection session contracts`（HEAD，验证后未变）
> 本文件是 R001/R009/D009 的回归锚点：此后任何重构必须对比本基线。

---

## 1. 实测结果（全部为本机实际运行输出）

| # | 检查项 | 命令 | 结果 |
|---|---|---|---|
| 1 | core fmt | `cargo fmt --manifest-path shared/rust-core/Cargo.toml --all -- --check` | **PASS** |
| 2 | core clippy | `cargo clippy --locked --manifest-path shared/rust-core/Cargo.toml --all-targets -- -D warnings` | **PASS** |
| 3 | core 单元测试 | `cargo test --locked --manifest-path shared/rust-core/Cargo.toml` | **202 passed / 0 failed / 1 ignored**（ignored = `iroh_transport::repeated_rtt_probes`，K005 已知） |
| 4 | macos crate fmt | `cargo fmt --manifest-path macos/src-tauri/Cargo.toml --all -- --check` | **PASS** |
| 5 | macos crate clippy | `cargo clippy --locked --manifest-path macos/src-tauri/Cargo.toml --all-targets -- -D warnings` | **PASS** |
| 6 | macos crate 单元测试 | `cargo test --locked --manifest-path macos/src-tauri/Cargo.toml --lib` | **61 passed / 0 failed / 1 ignored** |
| 7 | windows host 验证 | `bash scripts/check-windows-host.sh`（fmt + check --all-targets + test --no-run） | **PASS**（3 个 dead-code 警告为 host 编译预期，CONTEXT.md 已说明） |
| 8 | windows 前端 lint | `npm run lint`（oxlint） | **2 warnings / 0 errors**（既有：Settings.tsx L75 `routeSupportsLatencyTest`、L79 `pairingAddressForPeer` 的 react/only-export-components；exit 0） |
| 9 | windows 前端测试 | `npm test`（vitest） | **45 passed / 8 files**（Settings 10、History 5、hooks/utils 其余） |
| 10 | windows 前端构建 | `npm run build`（tsc -b && vite build） | **PASS** |
| 11 | site lint | `npm run lint` | **0 warnings / 0 errors** |
| 12 | site 构建 | `npm run build` | **PASS** |
| 13 | swift 测试 | `swift test --package-path macos/swift-ui` | **30 passed / 0 failed** |
| 14 | node 脚本测试 | `node --test scripts/generate-update-manifest.test.mjs scripts/validate-release-version.test.mjs` | **5 pass / 0 fail** |
| 15 | python v1 迁移测试 | `python3 -m unittest shared/scripts/test_migrate_v1.py` + py_compile ×3 | **3 tests OK / PASS** |
| 16 | settings 契约 | `node shared/schema/generate-settings.mjs --check` | **PASS** |
| 17 | 跨平台漂移 | `node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core` | **PASS**（"Cross-platform contract passed: shared Rust core, 38 Swift API commands, …"） |
| 18 | 版本一致性 | `node scripts/validate-release-version.mjs --root . --tag v2.1.0` | **PASS**（stable channel） |

## 2. UNVERIFIED（本机不可运行，CI 负责）

- Windows 原生 clippy / 打包（NSIS）/ 打包烟测 —— 需 windows-latest runner。
- 双向互操作探针 `test_cross_project_interop.ps1` —— 本机无 pwsh。
- macOS 打包 + `verify_macos_bundle.sh` —— 需完整构建链（本机未跑，属 release 流程）。
- `repeated_rtt_probes`（真实 QUIC timing）—— 已 ignore（K005）。

## 3. 基线速查数字

```text
core:        202 passed / 0 failed / 1 ignored
macos crate:  61 passed / 0 failed / 1 ignored
swift:        30 passed / 0 failed
frontend:     45 passed / 8 files（windows），lint 2 warnings（既有）
site:         lint 0 / build PASS
scripts:      node 5 + python 3
契约面:        settings check / drift check / version check 全 PASS
```

## 4. 记录说明

- 前端 lint 的 2 个 warning 为**基线既有**（非本次引入），重构 Settings 时（T201+）可顺带消除，但需单独验证。
- Settings.test.tsx 冲突用例运行时会向 stderr 打印预期错误日志（"Shortcut registration failed"），测试仍 PASS —— 属预期行为，非噪音。
- 测试过程未修改任何被跟踪文件（git status 复核：仅新增 docs/maintainability/ 与既有 windows/ci-artifacts/ 未跟踪项）。
