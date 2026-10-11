# 图文复合剪贴板实施记录

日期：2026-10-03；Iroh 诊断与验证补充于 2026-10-04。当前交付是默认关闭的本地候选实现。用户已确认“先完成本地实现，双设备验收稍后安排”。真实 Windows、双设备和目标应用结果仍为 UNKNOWN，不能据此声明原需求的微信粘贴门槛全部达成。

## 工作目录与基线

- 原目录：`/Users/monet/TailSync/TailSync-rc-phase35`，未修改其代码、已有应用或运行中的 daemon。
- 新目录：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`。
- 新分支：`codex/composite-clipboard`。
- 基线提交：`f719d581a1aa5198a1b3f2483222df5c69e3dfc0`，继承当前 Phase 3.5 / Phase 4B 和历史键盘导航进度。
- 本次变更尚未暂存、提交、推送或发布。基线 SHA 不能独自标识候选源码；应同时使用 `artifacts/composite-evidence/source-fingerprint.json`。
- 设计、固定版本开源调研与来源链接：`docs/COMPOSITE-CLIPBOARD-IMPLEMENTATION-PLAN-2026-10-03.md`。该文件保留原设计时的状态说明，本实施记录给出后续执行结果。

## 已接入生产路径

| 边界 | 本次实现 | 关键文件 |
|---|---|---|
| 采集 | 同时采集受支持文字、HTML、RTF 和图片；区分 native items；稳定 revision 与有限重试；转换放在阻塞工作线程 | 两端 `src/clipboard.rs`、`shared/rust-core/src/composite/` |
| 原生 I/O | macOS AppKit helper、Windows Win32 sidecar；有界二进制 IPC、进程超时；发布前构造全部格式；实际 revision/token 回执 | `macos/src-tauri/clipboard-helper.swift`、`tools/clipboard-helper/`、`shared/platform-clipboard-composite.rs` |
| 来源因果 | 有界 revision + 内容指纹 + token 回执；旧标记不能吞掉用户新 revision；整组恢复及文件恢复共用 generation | `composite/origin.rs`、两端 `clipboard_change.rs`、Core sync 与历史恢复路由 |
| 协议 | CBP1 标准包；CNS1 原生快照；CAK1 明确终态；三个新命令仅 v5 且双向认证能力协商后接收 | `shared/tailsync-protocol/src/{composite,native_clipboard,bitmap}.rs`、Core `secure.rs` |
| 可靠投递 | 有界 raw/zlib 分片、摘要、限额、并发与期限；每 peer 固定编码选择；旧端仅发送一种降级表示并记录不完整 | Core `peer/delivery*`、`composite/chunks.rs` |
| 可靠接收 | 发布前 durable claim；不确定结果不自动重做；最终 ACK 后才确认；ACK 丢失不重复发布；发布前再次检查授权与本机新复制 | Core `peer/composite_{receipts,receiver}.rs`、`shared/platform-network-server.rs` |
| 历史 | schema v12、加密 manifest/parts、共享资源、搜索、预览、收藏、删除、配额与 degraded 恢复；升级前私有原子备份 | `shared/rust-core/src/db/composite.rs` 及 `db/` 生产读取和生命周期 |
| UI / API | 一个复合条目与缩略图、格式/图片数、不完整状态、整组恢复及显式文字/图片恢复；生产 DTO 生成双端契约 | `tailsync-runtime/src/history.rs`、`shared/schema/`、Swift History、React History |
| 打包 | macOS 显式环境变量构建候选；Windows 可选 sidecar 构建和 `-CompositeClipboard` 包入口 | `macos/build-mac.sh`、`windows/scripts/{build-composite-helper,package-windows}.ps1` |

一个语义图文内容不会拆成先后覆盖的文字事件和图片事件。真实多 native items 及 RTFD-only 内容保留本地形状，不静默选取第一项同步。文件仍沿用文件批次。未知/OLE 格式未开放透传。

## 功能范围与阶段状态

| 阶段 | 本地结果 | 尚需证据或实现 |
|---|---|---|
| M0 格式探针 | PASS：5 组合成命名剪贴板探针、稳定性/回执/发布前拒绝；实际 AppKit 写入回读回路 | 真实 Chrome / Word 来源格式、延迟 provider、来源应用版本和真实负载性能 UNKNOWN |
| M1 模型与协议 | PASS：有界包、命令注册、v4 拒绝、双向协商、摘要与解析回归 | 真实旧版安装包互通 UNKNOWN |
| M2 原生采集/发布 | macOS 合成原生回读 PASS；Windows sidecar MSVC 目标编译检查 PASS；两端调用已接通 | Windows 原生读取/所有权/发布、provider 硬超时与竞争场景 UNKNOWN |
| M3 历史 | PASS：生产 DB 回归覆盖加密、共享引用、失败清理、quota、收藏、多 item、恢复、预览与 v11 升级备份 | 正式设备历史长期运行、满盘原生故障与用户目录迁移实测 UNKNOWN |
| M4 可靠事件 | PASS：认证传输生产接线、重组、持久回执、重试、新本机复制和撤销授权 | Mac↔Windows、LAN / Tailscale / Iroh 实际双设备链路 UNKNOWN |
| M5 UI / API | PASS：生产契约、React 回归、Swift 测试与构建；完整恢复和显式格式恢复已接线 | 原生窗口可见效果、键盘/IME/辅助功能与目标应用粘贴 UNKNOWN |
| M6 资源与应用适配 | PASS：已有内嵌 PNG 字节提取、精确节点绑定、双图离线 HTML 物化、CF_HTML 字节偏移 | RTFD 附件跨平台转换、微信专用输出配置、URL-only 资源补齐未实现；真实目标组合 UNKNOWN |
| M7 包与验收 | macOS arm64 候选构建、ad-hoc 严格签名检查、macOS 13 最低部署检查、包内 helper 探针 PASS | Windows 原生包/脚本执行、完整候选 daemon/GUI smoke、双设备及微信 Exit Gate UNKNOWN |

HTML 资源仅提取剪贴板已经包含的 `data:image/png;base64` 字节；独立 PNG 不被猜测为某个 img 节点。远程 URL、file URL、cookie 和登录状态不用于自动抓取。URL-only 或读取缺失被明确保存为不完整状态，接收端不把它算成完整应用成功。RTFD-only 保存并支持原平台回放，未声称可以转换为跨平台完整图文。

## 最终本地验证

详细命令、日志文件摘要和 SHA-256 见 `artifacts/composite-evidence/verification.json`；完整日志保存在同目录，均来自合成、隔离数据。

| 检查 | 结果 |
|---|---|
| Core，三个协议扩展开启，排除 `iroh_transport` | 540 passed，4 ignored，12 filtered；0 failed |
| Core 默认 feature 组合，排除 `iroh_transport` | 540 passed，4 ignored，12 filtered；0 failed |
| Core，三个扩展开启，完整串行运行（2026-10-04） | 552 passed，4 ignored，0 filtered；0 failed，包含全部 12 项 Iroh |
| Protocol / Runtime | 33 / 24 passed；0 failed |
| macOS Rust | 116 passed，3 ignored；0 failed |
| Windows crate 在 macOS host 上 | 125 passed，2 ignored；0 failed；不代表 Win32 原生代码运行 |
| Swift | 执行 250，4 skipped，0 failures |
| React / Vitest | 267 passed，51 文件 |
| fmt、root/Mac clippy `-D warnings`、feature 关闭编译、UI build/lint、生成契约、跨平台漂移 | PASS |
| Windows helper `x86_64-pc-windows-msvc` | `cargo check --features portable-check` PASS；未链接或运行 Windows EXE |
| 包内 helper 合成回放 | 5 组格式与拒绝路径 PASS |
| 包内 helper 实际 AppKit 回路 | 10,000 自身写入，0 unexpected echoes；10,000 相同内容新用户 revision 均识别 |

UI lint 退出码为 0，但仍输出 React effect 等警告；Windows crate 的 host 编译也有平台条件下的未使用代码警告。严格 `-D warnings` 通过项仅指上表明确列出的 root / macOS clippy，不包括 Windows 原生 clippy。

2026-10-03 的全量 Core 尝试出现 Iroh 失败和等待，因此当日门禁过滤了 12 项 Iroh。2026-10-04 核对发现原基线也有偶发关窗失败，基线与候选各自重跑 12 项均通过；本轮候选三个扩展的完整串行 Core 已取得 552 passed、4 ignored、0 filtered、0 failed。默认 feature 组合仍保留当日的过滤结果，未补跑其完整组合。未执行 CI，未将本地结果登记为发布验收完成。

关窗机制已由回环地址 + 6 秒延迟确认：测试在建连前开启的 5 秒窗口先于握手到期；改为产品默认 120 秒，两种配对均通过。本机还存在约 30 秒超时的代理隧道单地址，但最初自然建连延迟的具体诱因未锁定。产品普通 Iroh 配对/重连的 5 秒连接预算被登记为待实机验证的既存可靠性风险，未修改产品代码或安全窗口。完整分析见 `docs/IROH-TEST-DIAGNOSIS-2026-10-04.md`；追加证据、命令与日志摘要在 `artifacts/composite-evidence/iroh-diagnosis-2026-10-04/summary.json`，原 `verification.json` 保留当日记录。

回音回路验证的是实际 AppKit 命名 pasteboard → 生产 Core 转换器 → 来源分类器；其 0 echoes 指标不是完整 daemon 的网络广播计数，也不替代两台设备交替复制。全部合成探针报告 `general_clipboard_modified=false`。

## 候选包与复现

macOS 候选：`artifacts/TailSync-Composite.app`，arm64、ad-hoc 签名，最低 macOS 13。包和各二进制的摘要见证据目录。未安装、未启动候选 daemon；当前旧 `tailsyncd` PID 1208 正监听 TCP 19890，因此没有运行会竞争端口或修改日常剪贴板的完整包 smoke。

默认 build feature 不包含 `protocol-composite-clipboard`。候选构建明确开启；运行时设置 `TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD` 后关闭能力与自动复合采集。R1–R5 补修后 v11→v12 受运行时开关和 helper 健康控制，已有 v12 关闭态仍可读取；回退旧二进制使用完整升级前数据库与外置 payload 快照，不能直接打开 v12。

在新目录运行：

```bash
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline -p tailsync-core \
  --features 'protocol-composite-clipboard protocol-file-sliding-window protocol-image-compressed-chunks' \
  --lib -- --skip iroh_transport
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  -p tailsync-protocol -p tailsync-runtime --features tailsync-core/protocol-composite-clipboard
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  --manifest-path macos/src-tauri/Cargo.toml --features protocol-composite-clipboard --lib
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  --manifest-path windows/src-tauri/Cargo.toml --features protocol-composite-clipboard --lib
node scripts/run-isolated-tests.mjs -- swift test --package-path macos/swift-ui
npm --prefix windows test
node scripts/check-local-contracts.mjs
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
TAILSYNC_COMPOSITE_CLIPBOARD=1 TAILSYNC_APP_BUNDLE="$PWD/artifacts/TailSync-Composite.app" bash macos/build-mac.sh
python3 tools/clipboard-probe/probe.py --helper artifacts/TailSync-Composite.app/Contents/MacOS/clipboard-helper --self-test
node scripts/run-isolated-tests.mjs -- cargo run --locked --offline -p tailsync-core \
  --example composite_origin_probe -- "$PWD/artifacts/TailSync-Composite.app/Contents/MacOS/clipboard-helper" 10000
```

Windows 原生机器候选入口：

```powershell
.\windows\scripts\package-windows.ps1 -CompositeClipboard -OutputDirectory release-composite
```

该入口同时构建并携带 sidecar，portable EXE 必须与 `tailsync-clipboard-helper.exe` 放在同一目录。本机没有 PowerShell / Windows，入口尚未原生执行，不能将脚本审查算成 Windows 包验收。

## 接下来安排的真实验收

按 `docs/COMPOSITE-CLIPBOARD-NATIVE-ACCEPTANCE-2026-10-03.md` 记录设备、应用版本、源码指纹、包摘要与具体粘贴结果。先关闭旧实例并确认端口空闲，再在隔离数据目录运行候选。优先 Chrome/Word 来源 → Windows/Mac → 微信、Word/Notes 与纯文本编辑器；随后覆盖新旧互通、撤销配对、传输中用户复制、睡眠/重连、历史重启、provider 超时和大型表格。

真实证据取得前保持可选候选状态，不默认开启，不发布/tag，也不宣称任意网页图文或任意微信版本的一次粘贴已达标。

## R6 当前核验（2026-10-05）

上文 M0–M7 的历史结果只归属于各自旧指纹。R1–R5 补修记录另见 COMPOSITE-CLIPBOARD-R1-R5-FOLLOWUP.md；R6 不继承旧包或旧日志的通过结论。新证据目录为 `artifacts/composite-repair/2026-10-05-r6-final-evidence/`，结果由实际命令日志/JSON diagnostics 解析，source/package/log/document 各自 SHA256。

复合剪贴板的版本分别管理：identity/base **4**；data frame **5**（认证连接可协商回退到 4）；main history **条件 v11/v12**；effect journal **1**；local contract **1**；helper protocol **2**；origin fingerprint **2**。v11→v12 受构建开关、运行时开关和 helper 健康控制；已有 v12 在关闭态可读取，承接已有 v12 数据的目的库保持至少 v12。版本变更须有独立决策，不能把这些编号统称为“协议版本”。

H-17 / I-1—I-6 / I-8 的逐项证据、故障矩阵、debug/App/Windows 形态检查与回退证明见 [R6 核验](COMPOSITE-CLIPBOARD-R6-VERIFICATION.md)。Windows 原生、目标 App、双设备保持 UNKNOWN，发布 NO-GO。
