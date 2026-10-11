# Windows 修复与本地修改整合（2026-10-12）

整合分支：`codex/integrate-windows-fixes-2026-10-11`。任务于 10 月 11 日开始，验证跨至 10 月 12 日。

## 比较范围与来源

| 来源 | 基准 | 纳入内容 |
| --- | --- | --- |
| 最新复合剪贴板 / 双机验收分支 | `7759a232fc7657df01fd4e482bcb9f7f26f1c930` | 完整已提交实现，以及该工作区未提交的测试清理和清理记录 |
| 远端 `codex/windows-dual-device-build-2026-10-11` | `e776f73b1264800e7bdb97a88e040bb34b495342`，直接父提交为 `7759a23` | 31 个文件的 Windows 修复、共享历史摘要逻辑、回归与验收记录 |
| 当前 `fix/phase4b-storage-feedback` 工作区 | `f719d581a1aa5198a1b3f2483222df5c69e3dfc0` 加未提交修改 | S4-P1-4 文件接收进度回调锁外执行、回归和台账；运行时开关测试入口；适配后的协议属性测试 |

本地修改先形成 `b19bb3a`，再通过双父提交合并远端 Windows 分支。远端修复提交保留在整合分支的历史中。

对其他较旧工作区做了分支和状态盘点，并比较 remediation 工作区与最新实现。它们的历史草稿、预览 App、旧 UI 原型和临时诊断程序不属于本次整合候选；原位置保留。

## 合并判断

- Windows helper 的启动自检和写入回执、历史紧凑行和 popover、预览原生窗口立即销毁和焦点交回、富文本历史正文和搜索投影、打包配置文件传递全部纳入。
- 最新工作区的测试清理完整保留，包括有价值场景迁入常规测试文件及 RR-11 入口更新。
- 旧工作区的 `sync.rs`、`sync/batches.rs`、`sync/engine_state.rs`、`sync/receive_engine.rs`、`sync/tests.rs` 有独立的进度回调修复，最新基准没有包含，按三方比较纳入。
- 旧 `CapabilitySet` 字段、协商、feature、命令注册及 payload 限额已由最新实现包含；保留最新版本的资源更新能力、helper 健康和来源/日志健康门禁。旧 `from_env` 测试入口单独纳入，运行时行为不变。
- 旧 `composite/mod.rs`、`fingerprint.rs`、`support.rs`、独立 `tests.rs` 和旧 protocol codec 属于已被替代的早期实现。采用最新 codec、原生归档、wire digest 与 origin fingerprint 分离的实现，避免旧版覆盖新功能。
- 旧协议属性测试按现有 `Mime` / `PortableBundle` / 资源 byte range 和 `tailsync-resource:` 引用规则适配；随机帧覆盖扩展至 `0x0113`，含资源更新命令。任意 composite 字节无 panic 场景由现有 fuzz 测试保留。
- 唯一文本冲突是 `historyRowActions.test.ts` 中已被本地清理删除、远端改了选择器的 CSS 字面量测试；保留删除，远端生产 CSS 与组件/位置行为回归保留。

## 验证发现与补修

Windows 工程的新增 `.transparent(false)` 在 macOS host 上没有可调用的 Tauri 方法。把该显式设置限定到 Windows，保留 Windows 预览不透明的行为；macOS host 编译及该工程 130 项测试随后通过。

属性测试首次适配的生成器仍生成空 representation、普通 RTF 字节、从零开始的 resource id 和旧资源引用，因此两个测试在编码夹具时失败。修正生成器以满足当前格式，协议完整 66 项复测通过；没有放宽 codec 校验。

## 当前整合源码的本机证据

| 门禁 | 结果 |
| --- | --- |
| Core lib，全部 features，串行 | 929 passed，3 ignored；正文摘要、旧记录无改写、HTML 损坏 fallback、进度锁外回调、开关门禁均通过 |
| Core 集成套件 | composite followup 11 passed，legacy isolation / crash matrix / reliable event / repair regression 父入口全部通过；子进程 worker 按原约定 ignored，不重复计数 |
| Protocol 完整套件（修正夹具后） | 66 passed |
| Runtime lib 与集成 / Themes / Classifier / Helper host | 134 / 46 / 6 / 3 passed；Windows 专属 helper 系统测试在 macOS 上未执行 |
| macOS Rust all-targets | 116 passed，3 ignored；main 和 example 编译通过 |
| Windows Rust 工程，macOS host（补修后） | 130 passed，2 ignored；不代表原生 Windows 编译或 Win32 测试 |
| Swift 完整套件 | 353 项中 349 passed，4 按原有 opt-in 条件 skipped |
| Windows Vitest 完整套件 | 56 文件，432 passed |
| Windows TypeScript / Vite / lint | 构建及 lint 退出成功；lint 仍报告 React warnings |
| 仓库 Node 脚本 | 153 passed |
| 台账、生成本地契约、跨平台契约、canonical include、native capture policy、生产 feature 约束 | 通过 |
| Workspace Clippy，all-targets / all-features，`-D warnings` | 通过；协议夹具修正后另跑 protocol all-targets Clippy，通过 |
| Windows 工程 Clippy，macOS host，`-D warnings` | 未通过：Windows 专属 clipboard 错误变体/重试函数及 `prepare_readable_restore` 的 host dead_code；相关源码与两份基准提交相同，未为此新增 lint 豁免；原生 Windows Clippy 本轮未执行 |
| 格式与空白 | workspace 及平台 fmt、`git diff --check` 通过 |

首次 workspace 命令在协议夹具处停止，已通过的 Core 不重复执行；协议复测后单独完成剩余 runtime/themes/updater 目标。保留首轮失败记录，不将复测描述为首轮全绿。跨平台脚本最初使用错误路径/缺省目录，改用 `node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos` 后通过。

本轮没有重新运行 Windows 原生系统剪贴板、Win32 关闭/快速重开、Windows 打包和安装、macOS opt-in GUI/WebView、真实双设备验收或 hosted CI。远端附带的验收记录是其历史候选的证据，不能替代本整合分支的原生或设备验证。

## 原工作区保护

整合在独立 managed worktree 进行：`/Users/monet/.codex/worktrees/integrate-windows-fixes-2026-10-11/TailSync-rc-phase35`。

本地两份输入的完整 tracked patch、相关 untracked 源码/文档、HEAD、Git 状态和文件 SHA-256 保存在同级 `integration-evidence/`；原有 artifacts 与 App 原位置保留。复核两份输入的 HEAD、Git 状态和所有快照文件内容均未变化。原分支不被切换、提交、清理或覆盖。

日志和机器可读结果位于同级 `integration-evidence/`：`rust-workspace.log`、`protocol-recheck.log`、`rust-remaining.log`、`platform-rust.log`、`windows-rust-recheck.log`、`swift.log`、`windows-frontend.log`、`static-gates.log`、`cross-platform.log`、`final-checks.log`、`source-preservation.json`。
