# S2 首批：历史操作与技术信息减负

日期：2026-10-07，Asia/Shanghai。状态：**首批限定本机部分 LOCAL_COMPLETE；完整 S2 未完成，Release NO-GO。**

范围为 S2-00 的源码/隔离 UI 基线，以及 S2-01 的 F02 普通行操作、F03 格式信息减负。真实来源基线、完整性 UI 新决定和后续内容分析/分类/文稿预览/QQ/首图均未在本批完成。

## 1. 执行方式与候选

- 唯一工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`；分支 `codex/composite-clipboard`；HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。
- 一个内置子代理 `01a11238-6aeb-7ee2-a62a-06c386a4d037`／Erdos，`cline-pass/deepseek-v4.1-flash`／high，`fork_context=false`；完整明文任务和 cwd/branch/范围回执已核实。当前工具的 false 对应不继承历史，未使用外部后台代理。
- 父代理保存 baseline 后派发，等待完整 HANDOFF 并关闭子代理后，才修改产品。子代理未创建其他代理；未 stage/commit/reset/clean/new branch、替换 App/测试包、读用户库或操作 QQ/general clipboard。
- 输入指纹：`a996992fc3922c4fe37164f7241fee40780d751c3dde04042f396fb396d91096`，796 文件。
- 子代理交接：`a74d31b448c154ad45c3fc91506ce00561de410697083ffbd6700e61eaf5c1a4`，800 文件。
- 最终指纹：`7e321b376bab7c1831266cc601790a9d66c695a6d3f3c6e7fe8e708a8537e5c1`，801 文件。
- 输入→最终 13 个产品/测试文件变化；子代理→最终 10 个变化。完整 inventory 和相对输入 patch 保留，未以 HEAD diff 混称本批增量。

## 2. 实际交付

### F02：普通行不再提供复制文字

Mac/Windows 普通历史行移除显式 text 转换入口。后端 text API、服务端可恢复格式、整组恢复、Return/双击、预览恢复及异常再次复制规则不删除，不新增全局 Cmd+Shift+V 拦截。

次级图片转换仍只来自 `restorable_formats`；native-only 和 withheld PNG 不提供无效转换。Mac 没有有效动作时不显示动作菜单；只读详情独立存在，不用空的复制菜单承载诊断。菜单含维护/授权操作时用“更多操作”，不误称全部是复制格式。

O04/O05 的归档、来源授权、packet 导入及撤销入口保留；Mac 另补修了完成归档后撤销仍可达。显式联网/授权/一次消费/身份校验等后端语义完全未改。

### F03：计数下沉，完整性决定不擅自变更

默认行移除格式名/格式数/资源数，保留内容、类别、时间、大小和来源。后台字段未删。

- Mac：独立 info 按钮打开只读 popover，显示格式、表示/图片/原生 item 计数、native-only、完整性、可恢复格式和 withheld 原因。
- Windows：独立详情按钮打开只读定义列表；关联的 `aria-controls` / `aria-labelledby` 每行唯一，面板可键盘聚焦，Escape 关闭并返回按钮焦点。
- 未知完整性/withheld code 使用本地化退化文案，不将原始错误码当用户提示。
- 行上的既有 localOnly/incomplete 文案仍保留；存储失败、Partial/Unknown 和手动恢复路径不改。本批没有决定删除、合并或重新定义完整性标签。

## 3. 独立复验：子代理全绿不等于安全

主代理在冻结交接源码上运行自己的真实 React 行组件探针：**2 PASS／3 FAIL，实际 exit 1**。三个失败均属于新增详情区的事件隔离缺失：

| 行为 | 原结果 | 二次修复 |
|---|---|---|
| 双击只读详情文字 | 调用整条恢复 callback | 详情子树拦截 double-click 冒泡；不阻止正常文本选择 |
| 右键详情文字 | 调用历史删除 callback | context-menu 不进入行删除；保留原生只读文本菜单可能性 |
| 长按详情文字 | 调用收藏 callback | pointer-down 与行长按控制器隔离，并聚焦详情面板 |

这些是在合成 UI fixture 中实际观察的 callback，并未对用户历史执行删除或写系统剪贴板。最终原探针 **5 PASS／0 FAIL**；原测试文件保留，不通过改期待值修复。

另用实际 Swift 生产策略与生成 DTO 编译纯 native-policy 探针：**1 PASS／2 FAIL，exit 1**，证明 complete 和 native-only 行的 live revoke 被 maintenance 条件隐藏。

该撤销限制在前序 Mac UI 中已存在，子代理沿用了它；不是本批新引入的后端授权绕过。修复将 live revoke 与“incomplete HTML 可归档”条件分离；新增参数在实际行视图接线。最终 **3 个策略断言通过**。探针版本适配的是新增独立 revoke 参数，complete/native-only/no-grant 三个期待不变；这不是 AppKit 菜单现场验收。

其他二次完善：

- 详情按钮自身的 context-menu 也与行删除隔离。
- 详情 Enter/Space 不触发 History 的全局整组恢复/预览，Escape 只关闭详情并恢复焦点。
- 多行详情使用独立可访问性 owner，避免重复 id；普通行本身的双击恢复仍保留。
- Mac footer 为行尾控件预留空间，减少覆盖 source_peer 的风险；实际主题布局仍需 GUI 验证。
- 删除本批新增代码注释，修正新策略/测试的一字母局部命名；未新增 allow/ignore/skip。
- 父代理新测试首次 TypeScript 因 `batch_status` 被推宽为 string 失败，补 literal 类型标注后通过；原失败记录保留，不改生产合同。

## 4. 最终同源码门禁

**15 条最终命令 exit 0，验证器 errors=[]，全部绑定最终 7e321b37… 源码。** 不将子代理测试或早期诊断结果充作最终证据。

| 门禁 | 实际结果 | 说明 |
|---|---|---|
| 原独立 React 详情探针 | 5 PASS | 原三项 RED 已 GREEN |
| native 生产策略探针 | 3 个断言 PASS | 不是 Swift XCTest 或原生 GUI 计数 |
| Swift full | 280 PASS／4 skipped | 4 个是既有 opt-in GUI/WKWebView 条件 |
| React/Vitest full | 309 PASS | 包含实际 History 接线与只读面板交互回归 |
| Runtime all features | 62 PASS | 显式格式、恢复/授权接缝回归，不等于 OS 粘贴现场 |
| Core default | 688 PASS／4 ignored | 既有 ignored 保留 |
| TypeScript、oxlint、Clippy `-D warnings`、fmt | exit 0 | 未用新增豁免使门禁通过 |
| contracts、production features、canonical、capture visibility、diff | exit 0 | 生成契约/生产防漏和可见性保持 |

上述重叠 suites 和策略探针不相加为“唯一用例总数”。本批未运行新的 Windows 原生/Rust 平台包或 helper/App 包验收。

修复回归位于 `CompositeRestoreMenuTests.swift`、`HistoryRowDiagnosticsTests.swift`、`HistoryItem.test.tsx`、`History.test.tsx` 和 `historyRowActions.test.ts`。测试使用生产策略或真实组件，不复制一份 UI 过滤条件来证明自己。

## 5. 范围与原件保全检查

13 个改变文件只在 SwiftUI/React 行展示、对应测试、本地化和 history 样式。Rust/core/DB/helper/API/auth、schema、生成契约、原件/分类/cache/预览架构在本批 input→final inventory 中完全不变。

O05 seal 的 **1,228 个文件复算无 mismatch**；8 份 O05 单独封存的当前文档也无 mismatch。本批不写入或重封旧证据；旧总计划/执行记录首页的旧状态不作为本批完成依据。

所有 8 次非零 command attempts 保留：子代理 3 次 Vitest 诊断失败（包括既有 CSS 测试 cwd 条件）、主代理原 React RED、native-policy RED、父新 fixture TypeScript 首次失败、父文档检查误把 `git diff --no-index --check` 的正常差异退出 1 判成失败，以及最终封存脚本排序检查失败。修正文档检查为接受 0/1 且无空白诊断输出，原程序/FAIL 和新 ID 重跑均保留；不是产品修复或放宽空白规则。每条 raw log SHA256 复核一致。两次 apply_patch 上下文校验失败没有改动源码，也没有被包装成测试 PASS。

首次 r1 封存逐文件 hash 全部正确，但生成聚合 hash 时沿用 `Path` 排序，复核时采用相对路径字符串排序，两者在含目录/标点的路径上顺序不同，校验实际 exit 1。r1 已封存后不修改、不重封；原无效 seal、程序、已复制文档及实际 traceback 保留。另建 r2 并统一为相对路径字符串排序，重新核对当前文档、同一产品源码、原门禁日志及全部文件后封存。r1 不作为最终有效封存依据；该失败不涉及产品修改或重新解释测试结果。

共享 `target/o04-shared` 约 4.2 GiB，本批证据约 31 MiB（封存前测量），剩余磁盘约 71 GiB。未为本批新建巨大 Rust target；Rust 串行并关闭 incremental/debug，Swift/Vitest/Rust suites 使用隔离 runner。

## 6. 仍为 UNKNOWN 的部分

- 新 Mac popover 的锚定、最窄窗口/长来源名下行尾布局、深浅色视觉、实际 VoiceOver/辅助功能。
- 新 Windows 详情在原生 WebView2 中的文本选择、右键菜单、焦点/主题交互；jsdom 不是原生 Windows。
- QQ 来源与系统提示现场、TextEdit/Preview 等直接粘贴/历史恢复对照、真实用户复制样本。
- 两设备、hosted CI、完整 S2-00 来源样本矩阵，以及候选包/通用发布验收。

本批基线为源文件快照和隔离 UI fixtures，不声称建立了完整 NativeSnapshot/RTFD/QQ 的真实来源基线。未进行 CPU/RSS/p95 的新性能测量，不把单元测试耗时当性能验收。

## 7. 文件、证据与下一项

主要实现：

- `macos/swift-ui/Sources/TailSync/Views/HistoryRow.swift`
- `macos/swift-ui/Sources/TailSync/Views/HistoryRowActions.swift`
- `windows/src/pages/history/HistoryItem.tsx`
- `windows/src/pages/history/historyRowActions.ts`

最终有效证据根：`artifacts/clipboard-improvement/2026-10-07-s2-01-ui-simplification-r2/`。r1 原封存原样保留但聚合校验无效；子代理及初始门禁仍在原 r1 实际执行，r2 保存这些原始记录，不混称重新运行。

- `source-before.json` / `baseline/`：开工前精确源文件。
- `agent/ACK.md` / `agent/HANDOFF.md`：实际明文 receipt、实施交接与失败披露。
- `review/source-agent-frozen.json` / `review/agent-source/`：二次修改前源码。
- `review/probe/`：主代理独立探针。
- `commands/` / `logs/` / `sources/`：所有不可覆盖的命令、actual exit、raw log、before/after 指纹。
- `review/source-final-v1.json` / `review/increment-final-v3.patch` / `review/matrix-final-v1.json` / `review/verified-final-v3.json`：最终范围与门禁；v1/v2 核对结果也保留，v3 包含第八次非零 attempt。
- `review/invalid-seal-r1.json` / `review/seal-final-v1.py.txt` / `review/invalid-seal-r1-documents/` / `logs/parent-seal-final-v1.log`：无效封存的原数据与失败保留。
- `status-final.json` / `evidence-sha256.json`：本批状态与文件封存。

下一批是 **S2-02 共享有界内容分析与 native-only 提取**，之后才接 F01 分类、F06 首图和 F04 文稿预览。此处按逐项模式停止，不自动续做。

本批结论：**F02/F03 确定部分本机 LOCAL_COMPLETE；F03 完整性设计保留项仍待审阅；完整 S2 未完成；真实应用/GUI/Windows/设备 UNKNOWN；Release NO-GO。**
