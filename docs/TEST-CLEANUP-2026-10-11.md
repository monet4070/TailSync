# 测试清理记录（2026-10-11）

工作目录：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`。分支：`codex/dual-device-acceptance-2026-10-11`，比较基准：`7759a232fc7657df01fd4e482bcb9f7f26f1c930`。

本次逐项复核删除对象及其替代覆盖，清理重复夹具、只抄写实现值的断言、没有内容判定的截图测试，以及已被生产路径测试替代的模型。没有修改产品逻辑，没有提交或推送这轮修改。

## 准确行数

按 `git diff --numstat` 的物理行统计，包含测试辅助代码、注释与空行，排除本记录、性能说明和测试入口脚本：

| 范围 | 删除行 | 新增或迁入行 | 净减少 |
|---|---:|---:|---:|
| 两端 Rust re-export 测试 | 104 | 0 | 104 |
| Swift 测试 | 491 | 29 | 462 |
| Shared Rust / protocol 测试 | 369 | 14 | 355 |
| Windows 前端测试及初始化 | 570 | 206 | 364 |
| 合计 | **1,534** | **249** | **1,285** |

共删除 11 个独立测试文件。新增行主要是把独立 review 文件中有价值的场景迁入常规测试文件，而非新增防御性分支。不能将 1,534 行删除量当成净减少量，也不能将减少的测试函数数当成丢失的场景数。

## 整文件删除清单

| 文件 | 行数 | 删除理由及保留覆盖 |
|---|---:|---|
| `macos/swift-ui/Tests/TailSyncTests/SettingsUIPreviewTests.swift` | 124 | 仅生成多语言/主题截图并检查 PNG 大于 1KB，没有图片基准、文字、控件或布局断言。不是有效的视觉回归门禁。 |
| `macos/swift-ui/Tests/TailSyncTests/SettingsGeneralRenderTests.swift` | 56 | 同样仅检查截图大小，且一般设置已在上一文件重复渲染；开机启动服务行为测试保留。 |
| `macos/swift-ui/Tests/TailSyncTests/SettingsErrorRenderTests.swift` | 93 | 错误 toast/peer row 只验证能导出 PNG，错误语义与实际显示内容并未判定；协议错误映射与本地化测试保留。 |
| `windows/src/styles/artDirection.test.ts` | 40 | 检查 CSS 声明为空，不能证明真实 WebView 合成、点击命中或显示正常。 |
| `windows/src/styles/settingsDialog.test.ts` | 32 | 锁定 `100dvh`、`overflowY` 等声明，jsdom 不做真实布局，不能证明对话框实际可滚动。 |
| `windows/src/styles/themeLayout.test.ts` | 26 | 只抄写一条 padding CSS 变量；主题解析及组件行为测试保留。 |
| `windows/src/styles/windowSurface.test.ts` | 104 | 锁定透明背景、动画时间、颜色 token 与装饰几何，不能验证原生透明窗口或实际视觉效果；收藏和焦点交互回归保留。 |
| `windows/src/hooks/useThumbnailCache.parent-review.test.tsx` | 114 | 与常规 hook 文件分散、重复的 mock/夹具/重试和淘汰场景。独有的缓存归属、响应 key、取消队列、卸载与空响应重试迁入 `useThumbnailCache.test.tsx`。 |
| `windows/src/preview/documentReview.test.tsx` | 109 | 生命周期场景迁入 `useDocumentPreview.test.ts`；资产取消/内存计账迁入 `documentAssets.test.ts`；图片错误在替换文档后清除，已有 `renderers/DocumentPreview.test.tsx` 的实际 rerender 回归。 |
| `windows/src/utils/documentPreview.parent-review.test.ts` | 60 | 手写二进制 frame 和客户端 mock 重复；身份校验、数据库变更及不同取消时点迁入 `documentPreview.test.ts`，使用现有 frame 夹具。 |
| `windows/src/preview/documentCancelReview.test.ts` | 23 | 取消合并场景迁入 `documentPreview.test.ts`，共享生产客户端 mock。 |

## 文件内删除或合并

| 文件 / 测试 | 净减少 | 理由与保留覆盖 |
|---|---:|---|
| 两端 `src-tauri/src/network/rate_limit.rs` / `types.rs` 的 `contract_tests` | 104 | 直接 `pub use` 已由编译器检查；`same()` 两次赋值只验证函数签名，不能证明函数身份。Core 限流行为、平台实际连接及序列化测试保留。 |
| `AppBehaviorTests.swift` 的 `testCanvasUsesTheWindowsSemanticColorTokens` / `testCanvasUsesTheWindowsEditorialFontFamilies` 及 helper | 61 | 将具体色值和字体名抄成期望值，只冻结设计选择，没有独立行为判定。 |
| `HistoryPreviewLayoutTests.swift::testPreviewControlsUseConsistentMinimumSizes` | 9 | 仅检查尺寸常量。实际最小宽度渲染、控件可见性、换行及鼠标命中测试保留。 |
| `HistoryRowDiagnosticsTests.swift::testTheProductionRowUsesTheSharedPolicy` 及 `source()` | 26 | 用源码含有类型名/不含某字符串代替执行，重命名会误报，死代码也可通过。诊断值、隐藏状态及菜单策略测试保留。 |
| `CompositeRestoreLocalizationTests.swift` 两项 | 18 | 单独的错误本地化断言已由 negotiated socket 错误测试包含；菜单四个固定文案的完全相等断言删除，翻译存在性及动作策略仍受测试。 |
| `CompositeRestoreMenuTests.swift` | 75 | 将同一显式恢复策略的六种输入合为表驱动；保留纯文本、富文本、withheld PNG、native-only、单图片和缺资源场景。撤销菜单的三种输入也合并，删除对构造夹具本身的重复断言。 |
| `windows/src/pages/history/historyRowActions.test.ts` | 46 | 删除 CSS 字面量与固定菜单措辞断言，保留真实策略、双语言键及用户可见错误代码映射。 |
| `shared/rust-core/src/crypto/tests.rs` | 46 | 合并四处分散的 settings 默认/旧 JSON 测试，保留实际磁盘加载中的信任为空、快捷键、自动连接及网络图片默认关闭；缺配置和开关保存/加载回归保留。 |
| `shared/rust-core/src/db/tests.rs::preview_payload_reports_missing_history_id` | 15 | 与 `typed_preview_payload_reports_missing_entry` 调用同一入口、同一 404 输入；保留的测试还验证稳定错误码。 |
| `shared/rust-core/src/peer/delivery/tests.rs` | 139 | 删除 timeout 默认毫秒值检查；删除使用人工服务时间、64 文件/64B payload 的旧调度模型。生产认证 worker 基线、文件 ACK 隔离、满队背压及实际容量检查保留。 |
| `shared/rust-core/src/peer/types.rs::peer_status_defaults_to_discovered` | 5 | 只检查 `#[default]` 枚举字面值；旧 peer JSON 默认、安全状态一致性及序列化测试保留。 |
| `shared/rust-core/tests/repair_regressions.rs` | 134 | RR-05b 合入 `OriginTracker` 的来源测试，并补上回执尚未消费时的新 revision 断言；RR-11 手工 prepare/port/finish 不能验证生产调用方，改由 `event_receiver` 真实接收入口回归负责。删除未使用的 `SERIAL`，它从未被加锁，不能起到隔离作用。 |
| `shared/rust-core/src/composite/origin.rs` 的现有来源测试 | -4 | 迁入 RR-05b 的关键断言：相同 digest/token、尚未消费回执、新 revision 仍是用户复制，防止测试合并遗漏状态差异。 |
| `shared/tailsync-protocol/src/tests.rs::test_encode_decode_roundtrip` | 13 | 固定 hello 帧 roundtrip 已由从实际 command 集合抽样并随机生成 flags/sequence/payload 的 property test 更完整覆盖。版本、损坏、校验和、边界测试保留。 |
| `shared/tailsync-protocol/src/proptests.rs::frame_decode_consumes_at_most_the_input_length` | 7 | 随机原始字节几乎都在 magic 检查前后失败，成功分支通常零执行；有效 frame property test 已断言 consumed 等于输入长度，任意字节无 panic 测试仍保留。 |

表中“独立 review 文件”属于迁移合并，因此不能把整文件行数与文件内净减少量全部相加；总量以第一张表为准。

## 配套修改与保留项

- `scripts/run-repair-regressions.sh` 的 RR-11 指向 `inbound_text_does_not_stall_the_runtime_on_a_slow_platform`，没有留下匹配零测试的旧过滤器。
- `docs/performance-budgets.md` 标记旧模型已删除并移除失效运行命令；签署阈值、历史观测与原始失败记录保留。
- 删除 CSS 文件后，原先藏在这些测试中的 Node 类型引用不再生效。将类型声明移至 `windows/src/test/setup.ts`，不更改运行时逻辑。
- 数据库迁移、事务/崩溃恢复、配对和撤销、格式边界、资源限额、旧配置兼容、富文本/QQ/原始图片保留、生产锁与取消时序测试保留。
- 本次没有把不同 wire/archive 格式、不同平台入口或不同取消阶段，仅因函数名称相似就认定为重复。
- 删除这些 CSS/截图大小测试后，外观验收要由真实布局/交互回归及原生人工检查承担，不能宣称视觉覆盖完全不变。

## 验证

日志位于 `artifacts/test-cleanup/2026-10-11/`，初次失败与复测分别保留，历史 artifacts 未删除。

| 范围 | 本轮结果 | 日志 |
|---|---|---|
| Windows Vitest 完整套件 | 55 文件、418 项通过 | `windows-final.log` |
| Swift 完整套件 | 353 项中 349 通过、4 按原有 GUI/WKWebView opt-in 条件跳过 | `swift-final.log` |
| Core 库、全部 features、串行 | 首轮 921 通过、1 Iroh 连接超时、3 ignored；超时项单独复测 1 通过 | `rust-workspace-final.log` / `iroh-invite-recheck.log` |
| 工作区其余 crates 及 runtime 集成、全部 features、串行 | 253 项通过；在首轮 Core 失败后单独完成 | `rust-remaining-final.log` |
| Core 的 5 个集成套件 | 全部通过；3 个子进程 worker helper 按原有约定 ignored，由父测试显式驱动 | `core-integration-final.log` |
| Origin 合并后定向回归 | 8 项通过 | `origin-merged-final.log` |
| macOS Rust 应用 all-targets、串行 | 116 通过、3 ignored，main/example 目标编译通过 | `macos-rust-final.log` |
| 仓库 Node 脚本 | 153 项通过 | `scripts-final.log` |
| Windows TypeScript + Vite build | 删除 CSS 测试后首轮缺 Node 类型；引用移到测试 setup 后构建通过 | `windows-build-final.log` / `windows-build-recheck.log` |
| 入口与台账 | 31 项台账校验、shell 语法、diff 空白检查通过；RR-11 新入口实际运行 1 项通过 | `rr11-entry-final.log` |

最后新增的 Origin 断言只改变现有测试本身，随后运行相关 8 项回归；其余测试结果对应未再变动的代码。没有把一次超时后的复测描述为首轮全绿，也没有将子进程 helper 的输出重复计入集成测试数量。

这是本机测试与构建证据。Windows Rust 原生编译/系统剪贴板、macOS opt-in 原生 GUI/WebView 和真实双设备验收没有在这轮清理中执行。
