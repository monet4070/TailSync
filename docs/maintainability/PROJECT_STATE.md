<PROJECT_STATE>

PROJECT:
TailSync Maintainability Refactor

PROJECT_STATUS:
DONE

CURRENT_PHASE:
终态（T905 收官：全部任务完成，等待最终验收）

CURRENT_TASK:
NONE_AWAITING_USER

NEXT_TASK:
NONE（全部任务完成，终态）

COMPLETED_TASKS:
[
  T000-T003（Phase 0）
  T100/T101-T111（Phase 1：11 个迁移单元）
  T240 TAURI_API_MATRIX
  T241-T246 typed client 6 批（R008 收官）
  T247 Settings 责任图形式化（T200）
  T248 useUpdater hook 提取
  T249 History 责任图形式化（T220）
  T250 historyFilters 纯函数提取
  T251 useTransient toast 计时统一
  T252 buildHistoryQuery 提取
  T253 historyGrouping 分组/批次纯计算提取
  T254 useThumbnailCache 提取
  T255 useConnectionTests 提取
  T256 useShortcutRecorder 提取（R006 第 3 个，最大块）
  T257 useShortcutRecorder 独立单测（6 用例：commit 成功/失败回滚/同值短路/录制启停/捕获确认/unmount 恢复）
  T258 usePairing 提取（R006 第 4 个：5 状态 + 4 ref + 4 处理器 + 1s 轮询 + 焦点陷阱 + busy 守卫全部迁出；pairingAddressForPeer 下沉 utils）
  T259 useDevices 提取（R006 第 5 个：设备快照/加载/5s 轮询/peer-health-changed 订阅/refresh/reset/乐观 toggle 全部迁出；applyPeerEnabled 注入回调）
  T260 R006 收官 + Phase 2 完成（复核：Settings 1774→1202 行、8 useState/3 effect/4 ref、零 invoke、漂移 PASS、责任图更新为完成态）
  T300 R009 责任图形式化（新文档 COMMANDS_RESPONSIBILITY_MAP.md；修正 BASELINE 计数：Windows 命令 35→42、macOS 28→30；厚命令 5 处 + 双实现重复点 5 处 + 拆分优先序）
  T301 change_storage_location 编排入 core（T300 图 §5#3：db::migrate_storage_with_rollback + hooks 注入 + 失败枚举；命令层 W/M + routes.rs 三处变薄；core +5 测试 = 252，K031 登记唯一行为差异）
  T302 trust_peer 编排入 core（T300 图 §5#4：identity::trust_peer + 快照回滚 + persist hook；命令层 W/M + routes.rs W/M 四处变薄，表面错误串/响应形状逐字保持；core +6 测试 = 258，K032 登记空白地址边界）
  T303 update_settings 编排入 core（T300 图 §5#2：crypto::apply_settings_update + persist/ShortcutChangeHook + db 限额 + mode outcome；命令层 W/M + routes.rs W/M 四处变薄，Windows 快捷键事务经 hook 保留；core +6 测试 = 264，K033 登记 routes 顺序统一）
  T304 restore_entry 保持评审 + Phase 3 收官（写侧机制刻意不同：arboard+CF_DIB vs SyncPlatform；统一 = P0/R003 违反；R009 = 3 迁移 + 1 单实现复核 + 1 保持，commands.rs 变薄 ~145 行）
  T320 R010 责任图形式化（新文档 SYNC_RESPONSIBILITY_MAP.md；sync.rs 2692 行/32 测试/24 Result<_,String>/0 生产 cfg 实测；5 个拆分候选按隔离度排序）
  T321 续传持久化簇 → sync/resume.rs（图 §5#1：5 函数 + 2 持久化类型原样迁入，pub(crate) 可见性，cleanup_expired_transfers 公共面不变；sync.rs 2692→2487 行；core +4 测试 = 268）
  T322 ShadowFilter → sync/shadow.rs（图 §5#2：ShadowEntry/Filter + 2 常量原样迁入 102 行；2 个直测私有字段的测试随迁；sync.rs 2487→2478 行，Instant 导入清理）
  T323 批准备纯逻辑 → sync/prepare.rs（图 §5#3：8 函数 + 6 类型 + impl validate 原样迁入 480 行；`pub use prepare::{…11 项}` 保持 sync:: 公共路径（平台引用点实测）；core +5 测试 = 273；sync.rs 2478→2159 行）
  T324 方法族保持评审 + R010 收官（单一 impl SyncEngine ~40 方法/10 共享字段，接收族触 ≥7 字段；impl 分散 = 零耦合削减，子结构提取 = 状态模型重构行为风险 → R022 保持；R010 = 3 簇迁出 -533 行 + 方法族保持）
  T350 R012 错误清单形式化（新文档 ERROR_SURFACE_INVENTORY.md：core 103 处 Result<_,String> 按 domain 实测 + 平台 60/36 + 类型化先例 4 处 + K004 Swift 子串契约 L730-744 复核 + T351+ domain 排序）
  T351 pairing 域类型化（R012 首个：PairingError 10 变体 thiserror；5 签名转换 + 平台 8 调用点显式 map_err；Display 串逐字保持；3 既有测试升级类型+Display 双断言）
  T352 sync 批准备/续传域类型化（R012 第二个：PrepareError 23 变体 + ResumeError 2 变体；8 签名转换 + 平台 6 调用点适配；wire 字符串逐字保持；11 处既有测试断言升级类型匹配；delivery 域保持评估登记——trait 契约面价值/风险比低）
  T353 identity + crypto settings 域类型化（R012 第三/四个：IdentityKeyError 2 变体 + SettingsValidationError 7 变体 + SettingsUpdateError 3 变体；5 签名转换 + 平台 4 调用点 + secure 透传；Display 逐字保持；2 处测试断言升级）
  T354 import 域类型化 + R012 收官（ImportError 24 变体；6 签名转换 + api/imports.rs W/M byte-identical 5 适配点；10 处测试断言升级；R012 = 5/6 domain + delivery/iroh/db 保持评估）
  T400 R018 日志清单形式化（新文档 LOGS_INVENTORY.md：core 67 处 e2/w30/i26/d9 + 平台 173/161 实测；无结构化字段确认；T401/T402 设计先行 + §18 禁止清单）
  T420 R019 网络测试分层形式化（新文档 NETWORK_TEST_LAYERING.md：确定性 62 测试全在 core（目标态达成）+ 真实传输层 CI-only/ignored 现状保持；R015 门禁不动）
  T440 R020 fuzz 候选审计（新文档 FUZZ_AUDIT.md：0 fuzz target 实测；Frame::decode 最高优先 + proptest 无新门禁方案；不凑数）
  T355 R017 版本入口实现（scripts/bump-version.mjs：12 文件定向写入 + Cargo.lock 块级同步零依赖重解析 + --check/--dry-run + 自校验；4 新测试，脚本测试 9/9；真实仓库 check 正负/dry-run 零写实测；CI 增强留独立 R015 任务）
  T356 R013 错误边界形式化（新文档 ERROR_BOUNDARY.md：三层错误流图 + Swift ApiError 5 case 实测 + K004 子串↔R012 枚举对照表 + 边界决策 3 条；swift test 30/30 作为契约门禁实证）
  T441 Frame::decode proptest（R020 实现：proptest dev-dep + 6 属性测试——24 命令全变体 round-trip/任意字节不 panic/consumed 上界/3 解码器不 panic；core 273→279；附带修正 api/imports.rs 钉扎面 fmt 归一化）
  T357 R016 coverage 报告（REPORT ONLY：Apple LLVM 17 + -Cinstrument-coverage 零新依赖；core 87.34% 行 / 85.64% 区域 / 80.34% 函数；分文件表 + 复现命令入 COVERAGE_REPORT.md；CI 未动；插桩缓存清理后 279 测试复原）
  T401 R018 diagnostics schema 设计（新文档 DIAGNOSTICS_SCHEMA.md：v1 schema + 5 事件面 + §18 禁止清单 + T402 挂载路线；设计先行硬约束）
  T402 diagnostics 挂载试点（R018 实现：core diagnostics 模块——Event 5 变体 + Record/ErrorRef + set_collector/is_collected/record/error_ref，无 collector 零分配零输出；pairing 域 5 挂载点；core +4 测试 = 283；全局 collector 测试锁；日志文本逐字不变实证）
  T359 R015 CI 集成（bump-version.test.mjs 入 scripts job node --test + bump --check 步骤；YAML 校验通过 + 本机同命令 9/9；严格增强，R015 零门禁削弱）
  T360 终审复核（全门禁 fresh 重跑：core 283 / macos 61 / frontend 111 / swift 30 / scripts 9 / 漂移 / host check 全 PASS；PROJECT_STATE §23 完成标准评估表落盘）
  T900 修复 change_storage_location 通知阻塞锁（blocking_lock → async 上下文读 bool，W/M 同文；通知文本/时机/迁移/回滚不变；负向检查零命中；全门禁 PASS）
  T901 修复 useShortcutRecorder 成功路径 toast（成功无条件 showSavedToast，reportInline 仅控制失败信息位置；短路/失败不 toast；3 处测试断言修正；hook 6/6 + Settings 10/10 + 全量 111/111 + lint/build PASS）
  T902 修复 diagnostics 持锁回调死锁（COLLECTOR 改 Arc 存储，锁内仅 clone、锁外调用 collector；重入不死锁 + 锁空闲 2 测试；core 283→285；clippy/macOS/host check/drift 全 PASS）
  T903 coverage 刷新 + 最终门禁（行 87.66% / 区域 86.02% / 函数 80.84%，COVERAGE_REPORT.md 更新；全门禁 fresh PASS；REGRESSION_BASELINE 与 §23 基线行刷新；插桩缓存复原）
  T904 修复 diagnostics 测试并发污染（仅测试代码：测试私有 contains_ordered_subsequence 有序子序列断言 + failure 测试按唯一消息标识过滤；pairing 组默认并行 30/30；core 全量 + 5 重复 + 插桩全量 285/1 全绿；fmt/clippy/diff check PASS；diagnostics.rs 零改动）
  T905 PROJECT_STATE 终态 + R004 状态（终态落盘：PROJECT_STATUS=DONE、NEXT_TASK=NONE、T900-T904 入 COMPLETED_TASKS；§23 终表 R004 状态落盘为阶段完成/整体 PARTIAL/显式保留残留；最终门禁 fresh 全 PASS）
]

BLOCKED_TASKS:
[]

LOCKED_DECISIONS:
[D001-D010]

VERIFIED_REQUIREMENTS:
[
  R001/R002/R003/R005/R008/R014: 保持（core 273 / macos 61 / 漂移 PASS / 零 invoke）
  R004: 阶段完成 / 整体 PARTIAL（显式保留残留，见 §23 表与残留登记）
  R006: 完成 —— 5/6 提取（updater/connectionTests/shortcut/pairing/devices）+ update() 按责任图 §7 建议保持；Settings 1774→1202 行，8 useState/3 effect/4 ref，独立单测 32 用例（T257/T258/T259）
  R009: 完成 —— T300 图 + T301/T302/T303 迁移（3/5 厚命令入 core）+ cancel_file_batch_impl 单实现复核 + restore_entry 保持评审（T304）；commands.rs 变薄 ~145 行，core +17 测试
  R010: 完成 —— T320 图 + T321/T322/T323 三簇迁出（sync.rs -533 行）+ T324 方法族保持评审
  R012: 收官评估 —— 5/6 domain 类型化（pairing、sync prepare/resume、identity、crypto settings、import）；delivery/iroh/db 保持评估登记（trait 契约面 / R011 约束 + iroh 依赖面）；core 生产面 Result<_,String> 103 → 25
  R011/R015/R021/R022: 同前（未动）
]

PARTIALLY_VERIFIED_REQUIREMENTS:
[
  R013: 完成 —— T356 错误边界形式化（三层边界明确 + K004 对照表 + swift test 30/30 实证）
  R016: 完成 —— T357 core 覆盖报告（87.34% 行，REPORT ONLY，CI 未动）+ T903 刷新（87.66% 行）
  R017: 完成 —— T355 bump-version.mjs（12 文件 + 锁同步 + --check/--dry-run + 自校验，测试 9/9，真实仓库正负实测）；CI 集成留独立 R015 任务
  R018: 推进（T400 清单 + T401 schema + T402 pairing 挂载试点完成；导出通道 T402+ 待评估）
  R019: 推进（T420 分层表完成，目标态已达成；T421+ 可选增量）
  R020: 完成 —— T440 候选审计 + T441 Frame::decode 等 6 属性测试（无新 CI 门禁，R015 不动）
]

KNOWN_ISSUES:
[
  K001-K029（同前）
  K030: 已解决（T257 独立单测 6 用例 + Settings.test.tsx 2 个集成用例 + 98/98 全绿）
  K031: change_storage_location 统一后唯一行为差异（T301）—— rollback spawn_blocking join-error 路径命令层由裸错误改为包装消息；仅 runtime 关闭时可达，字符串契约其余逐字保持
  K032: trust_peer 统一后空白地址边界（T302）—— 两端原将空白地址交给 infer_interface（必错），现先过滤再回落 "lan"；前端从不发送空白地址，K004 子串契约不受影响
  K033: update_settings 统一后 routes 面执行序（T303）—— 原 commit→mode 反应→db 限额，统一为 commit→db 限额→mode 反应；两阶段独立且均在提交后，无可观测差异
]

OPEN_QUESTIONS:
[]

ASSUMPTIONS:
[]

FILES_CHANGED:
[
  windows/src/hooks/useShortcutRecorder.ts（T256，新增：完整快捷键录制状态机 + DEFAULT_SYNC_SHORTCUT）
  windows/src/hooks/useShortcutRecorder.test.tsx（T257，新增：6 用例独立单测，mock tailsyncClient/shortcut/useI18n）
  windows/src/hooks/usePairing.ts（T258，新增：pairing 状态机 + 轮询 + 焦点陷阱 + busy 守卫）
  windows/src/hooks/usePairing.test.tsx（T258，新增：13 用例独立单测，fake timers 验证轮询/paired 过渡/unmount 停轮询）
  windows/src/utils/pairingAddress.ts（T258，新增：pairingAddressForPeer 纯函数下沉）
  windows/src/hooks/useDevices.ts（T259，新增：设备快照 + 5s 轮询 + peer-health-changed 订阅 + refresh/reset/乐观 toggle）
  windows/src/hooks/useDevices.test.tsx（T259，新增：13 用例独立单测，fake timers/visibilityState 验证轮询门控）
  windows/src/pages/Settings.test.tsx（T258：pairingAddressForPeer 改从 utils 导入）
  windows/src/pages/Settings.tsx（-572 行累计：T248/T255/T256/T258/T259 五轮提取；lint 警告 2→1）
  docs/maintainability/SETTINGS_RESPONSIBILITY_MAP.md（T260：补 §0 提取完成状态）
  docs/maintainability/COMMANDS_RESPONSIBILITY_MAP.md（T300 新增 + T301-T304 补 §0 完成态/保持评审）
  docs/maintainability/SYNC_RESPONSIBILITY_MAP.md（T320 新增 + T321-T324 补 §0 完成态/保持评审）
  docs/maintainability/ERROR_SURFACE_INVENTORY.md（T350 新增 + T351/T352 补 §0 完成态）
  shared/rust-core/src/pairing.rs（T351：PairingError 枚举 + 5 签名转换 + 3 测试升级）
  shared/rust-core/src/identity.rs（T353：IdentityKeyError + 2 签名转换 + trust_peer 映射）
  shared/rust-core/src/crypto.rs（T353：SettingsValidationError + SettingsUpdateError + 3 签名转换 + 测试升级）
  shared/rust-core/src/secure.rs（T353：decode_trusted_key 透传 map_err）
  shared/rust-core/src/sync/prepare.rs（T352：PrepareError 23 变体 + 6 签名转换 + 测试升级）
  shared/rust-core/src/sync/resume.rs（T352：ResumeError + 2 签名转换）
  shared/rust-core/src/sync.rs（T352：5 处调用点 map_err + 11 处测试断言升级）
  shared/rust-core/src/import.rs（T354：ImportError 24 变体 + 6 签名转换 + 10 处测试断言升级）
  windows|macos/src-tauri/src/api/imports.rs（T354：5 处适配点 map_err，两端保持 byte-identical）
  windows|macos/src-tauri/src/commands.rs、network/mod.rs、network/server.rs、api/routes.rs（T351+T353：配对 8 处 + settings 4 处调用点适配）
  windows|macos/src-tauri/src/network/server.rs、clipboard.rs（T352：6 处 prepare/validate 调用点适配）
  shared/rust-core/src/sync/resume.rs（T321 新增：续传持久化簇 + 4 测试）
  shared/rust-core/src/sync/shadow.rs（T322 新增：ShadowFilter 102 行 + 2 测试）
  shared/rust-core/src/sync/prepare.rs（T323 新增：批准备纯逻辑 480 行 + 5 测试）
  shared/rust-core/src/sync.rs（T321-T323：-533 行，mod resume/shadow/prepare + re-exports）
  shared/rust-core/src/db/storage.rs（T301：migrate_storage_with_rollback + StorageMigrationHooks/Failure + 5 测试）
  shared/rust-core/src/db.rs（T301：re-export 3 项；T303：open_at 改 pub(crate)）
  shared/rust-core/src/identity.rs（T302：trust_peer + TrustPeerFailure + 6 测试）
  shared/rust-core/src/crypto.rs（T303：apply_settings_update + SettingsUpdateOutcome + 6 测试）
  windows|macos/src-tauri/src/commands.rs（T301+T302+T303：change_storage_location/trust_peer/update_settings 变薄，两端同文）
  windows|macos/src-tauri/src/api/routes.rs（T301+T302+T303：三 arm 变薄，两端同文）
  docs/maintainability/PROJECT_STATE.md（更新）
  windows|macos/src-tauri/src/commands.rs（T900：change_storage_location 通知读取改 async 上下文读 bool，两端同文）
  windows/src/hooks/useShortcutRecorder.ts、useShortcutRecorder.test.tsx（T901：成功路径无条件 showSavedToast + 3 处断言修正/补充）
  shared/rust-core/src/diagnostics.rs（T902：COLLECTOR 改 Arc 存储，锁外调用 collector + 2 重入/锁空闲测试）
  docs/maintainability/COVERAGE_REPORT.md（T903 刷新：87.66% 行 / 86.02% 区域 / 80.84% 函数）
  shared/rust-core/src/pairing.rs（T904：测试私有 contains_ordered_subsequence 助手 + diagnostics 两测试改为有序子序列/唯一标识过滤断言，仅测试代码）
  docs/maintainability/PROJECT_STATE.md（T905：终态落盘——PROJECT_STATUS=DONE、T900-T905 入 COMPLETED_TASKS、§23 终表 R004 状态落盘为阶段完成/整体 PARTIAL/显式保留残留 + remediation 终审登记）
  （T101-T111 + T241-T258 改动仍在工作树）
]

REGRESSION_BASELINE:
core 285（T301 +5 / T302 +6 / T303 +6 / T321 +4 / T323 +5 / T441 +6 / T402 +4 / T902 +2）/ macos 61 / frontend 111 / swift 30 —— 无退化
完整基线见 docs/maintainability/REGRESSION_BASELINE.md

LAST_VERIFIED_COMMIT:
1849419（工作树含 T101-T111 + T241-T354 未提交改动，验证基于当前工作树）

## §23 完成标准评估（2026-08-15 终审；T900-T905 remediation 收官）

全部 22 项需求处于**已完成或显式评估+决策登记**的终态：

| 需求 | 状态 | 证据 |
|---|---|---|
| R001 行为保持 | 完成 | K031-K033 唯一差异登记（不可达/边界路径），全程漂移 PASS |
| R002 跨平台兼容 | 完成 | 漂移检查每轮 PASS（38 Swift 命令面/TCP 19890/API 19889） |
| R003 core 平台无关 | 完成 | 生产代码零新增 cfg（T320/T441 实测） |
| R004 消除字节重复 | 阶段完成 / 整体 PARTIAL（显式保留残留） | 已完成：Phase 1 11 单元 + Phase 3 命令层（R009 收官）+ R010 sync 簇（-533 行）；残留（显式保留，非阻塞）：平台适配/契约面 byte-identical 10 文件（sync_adapter.rs、updates.rs、api/imports.rs、api/transport.rs、network/*、main.rs、build.rs、interop_probe.rs，K006/IDENTICAL_SOURCE_INVENTORY，见残留登记） |
| R005 漂移检查器保留 | 完成 | 每次修改后运行，从未删除/收窄 |
| R006 Settings 降复杂度 | 完成 | 1774→1202 行，5/6 提取 + update() 保持评审 |
| R007 History 降复杂度 | 完成 | T250-T253 纯函数提取（Phase 2） |
| R008 typed client | 完成 | 页面零 invoke，31 wrapper/23 类型 |
| R009 命令层变薄 | 完成 | 3/5 厚命令入 core（-145 行）+ 2 保持评审 |
| R010 sync 渐进拆分 | 完成 | 3 簇迁出（-533 行）+ 方法族保持评审 |
| R011 DB 不重构 | 保持 | 未触碰 DB 架构（仅错误类型化/测试基建） |
| R012 typed error | 5/6 + 3 保持评估 | 生产面 103→25；delivery/iroh/db 决策登记 |
| R013 错误边界 | 完成 | ERROR_BOUNDARY.md + Swift 30 测试实证 |
| R014 高风险路径测试优先 | 完成 | 续传/恢复/pairing/proptest 专项 |
| R015 CI 不降低 | 完成 | CI 零削弱 + T359 增强（bump 测试 + --check 步骤入 scripts job） |
| R016 coverage 可观察 | 完成 | 87.66% 行 REPORT ONLY（无 gate，T903 刷新） |
| R017 单一版本入口 | 完成 | bump-version.mjs + 9/9 测试 + 真实仓库实测 |
| R018 可观察性 | 试点完成 | 清单+schema+pairing 挂载；导出通道留 T402+（登记） |
| R019 网络测试分层 | 完成 | 确定性 62 测试全在 core（目标态达成） |
| R020 parser 健壮性 | 完成 | 6 属性测试（Frame/解码器不 panic + round-trip） |
| R021 不统一 UI | 保持 | 无 SwiftUI↔React 合并动作 |
| R022 避免过度架构 | 保持 | 每次提取过 §20-§22 Decision Tests（6 次保持评审） |

**残留登记（显式决策，非阻塞；R004 整体 PARTIAL 即由此构成）**：R004 平台适配/契约面 byte-identical 显式保留残留——sync_adapter.rs、updates.rs、api/imports.rs、api/transport.rs、network/*、main.rs、build.rs、interop_probe.rs 按 K006/IDENTICAL_SOURCE_INVENTORY 属平台编排/适配/接线，为漂移检查器契约面而非待消除重复（T304/T000 §6.1 结论同源）；R012 iroh/db/delivery 域、R018 导出通道、R019 可选增量、R020 其余候选——全部附理由与后续入口。

**Remediation 终审（T900-T905）**：T900 blocking_lock 修复 / T901 toast 修复 / T902 持锁回调修复 / T903 coverage+门禁 / T904 测试并发污染修复 / T905 终态落盘——每项均含复现与真实命令证据，生产行为零变更（T900 唯一已知时序差异已登记）。

**最终测试基线**：core 285 / macos 61 / frontend 111 / swift 30 / scripts 9 —— 全部 PASS；漂移检查 PASS（T905 fresh 复核）。

</PROJECT_STATE>

---

## 自定义主题功能（T001-T009，进行中）

> 独立功能项目，叠加在已完成的可维护性重构之上。执行提示词见仓库内任务文档；需求 R001-R011 与验收逐任务登记。

### 进度

| 任务 | 需求 | 状态 | 证据 |
|---|---|---|---|
| T001 | R001 core 主题类型与校验 | **PASS** | `themes.rs` 20 测试全过（断言点 71 个）；全量 core 305 passed / 1 ignored（基线 285+20）；clippy `-D warnings` 零警告 |
| T002 | R002 core 发现与列表 | **PASS** | `list_themes_at` 10 测试（0 文件/多主题排序/坏 JSON/超限/内置冲突/重复 ID/子目录忽略/0700）；全量 core 315 passed / 1 ignored（285+30）；clippy 零警告 |
| T003 | R003 core 导入/删除 | **PASS** | `import_theme_file_at`/`delete_theme_at` 7 测试（复制独立/重复 ID/内置拒绝/坏源/路径逃逸/限额/删除）；全量 core 322 passed / 1 ignored（285+37）；clippy 零警告 |
| T004 | R004 daemon 双端 4 命令 | **PASS** | 双端 commands.rs 4 `#[tauri::command]` + routes.rs 4 arm + lib.rs 注册；api.rs 新增 `theme_id` 字段/`themes_listing_payload`/`reveal_themes_dir` + 2 契约测试；macos daemon 63 passed（61+2）、win daemon 2 passed、漂移 PASS（38 Swift 命令） |
| T005 | R005/R006 Windows 校验扩展+令牌注入 | **PASS** | useTheme 15 用例（custom: 校验/保持原值/无 daemon 回退/storage 同步/28 变量注入/暗色重注入/清空/警告）；windows 121 全绿 + build + lint；core 323 + clippy 零警告；漂移 PASS |
| T006 | R007 Windows 设置页 UI | **PASS** | Settings.test 17 用例（三态/选择/导入成败/删除确认/打开文件夹）；useTheme 16；windows 129/129 + build + lint |
| T007 | R008 macOS 模型重构 | **PASS** | swift build + swift test 34/34（30+4：快照/自定义解码/坏 palette 拒绝/Selection 解析）；漂移 PASS（39 Swift 命令） |
| T008 | R009 macOS 设置 UI | **PASS** | swift build + swift test 35/35；漂移 PASS（42 Swift 命令）；回退路径手动验证步骤已登记 |
| T009 | R010/R011 安全审计+文档+总审计 | **PASS** | 安全 grep 证据齐备（无 innerHTML/无样式拼接/命令路径服务端常量/0700 断言/复制导入/.json 白名单）；THEMING.md 重写（§0 已支持运行时、正式规范、24 令牌修正）；README 增补；全门禁 fresh 全绿 |

### T001 关键登记

- **令牌计数**：THEMING.md §2.2 表实际列出 24 个 CSS 颜色令牌（art-direction.css 每主题块逐块核实 24 个）；文档"28 令牌"为约数，R011 文档任务时修正表述。
- **JSON 颜色两种写法**：裸字符串 `"#rrggbb"` 或对象 `{ "hex", "opacity"? }`；opacity ∈ [0,1]，CSS 侧经 `css_value()` 合成 rgba()。
- **structural V1**：`borderRadius ∈ [0,64]`（DECISION，R011 正式化）、`shadow` 仅 `false` 有效；未知键收入 `ignored` 供上层 warning。
- **严格反序列化**：顶层与 palette/metrics/typography/fonts 均 `deny_unknown_fields`（Swift 字段名如 `accent` 会被明确拒绝），仅 structural 放行未知键。
- **F5 追踪**：属 T004/T005 前置，未在本任务触碰。

### T002 关键登记

- `themes_dir()` = `{数据目录}/themes`（`get_data_dir()` 复用 F11 模式）；`ensure_themes_dir` 创建 + 权限收紧，复用 identity.rs `restrict_private_directory`（仅改可见性 `pub(crate)`，Windows SDDL 保护 DACL 一并复用）。
- `list_themes_at(base)` 可测变体；排序确定（按文件名）；超限（>32）的后续文件直接跳过并报错；内置 ID/重复 ID 报错跳过；仅读 `.json`、忽略子目录。
- `ThemeEntry`（serde 可序列化，供 T004 daemon JSON）与 `ThemeListing { entries, errors }`。

### T003 关键登记

- `import_theme_file_at(src, base)`：读源→校验（R001）→内置 ID 拒绝→冲突检查（目标文件存在 / 他文件同 ID）→限额检查（≥32 拒绝）→写入 `{id}.json`（复制语义）；拒绝路径不触碰 themes 目录；扩展名仅 `.json`（含 `.tailsync-theme.json`）。
- `delete_theme_at(id, base)`：无效 id（含遍历形态）与内置 ID 拒绝；目标恒为 `{themes dir}/{id}.json`，不存在报 "not found"。
- 抽取 `scan_json_files` 共用（list/import 同一扫描语义，仅 .json、忽略子目录、排序确定）。

### T004 关键登记

- **双通道**：Windows `commands.rs` `#[tauri::command]`（webview invoke）；macOS `routes.rs` `handle_cmd` arm（SwiftUI 经 API）。双端 commands.rs 各 4 个薄包装 + 双端 lib.rs 注册；routes.rs 双端同文 4 arm（8 空格缩进，漂移脚本命令提取正则可捕获，T008 依赖）。
- **参数命名**：`import_theme(path)` / `delete_theme(theme_id)`，双端一致；`Request` 新增 `theme_id: Option<String>`（serde default，向后兼容）。
- **list_themes 载荷**：`themes_listing_payload`（api.rs 双端同文）→ `{builtin:[5×{id}], custom:[ThemeEntry], errors:[{file,reason}]}`；`ThemeLoadError` 补 `Serialize`。
- **reveal 安全**：路径来自服务端常量 `themes_dir()`，`Command::new("explorer"/"open").arg(path)` 单参数无 shell（R10 红线的 daemon 侧落实）。
- 冒烟测试：`themes_listing_payload_has_contract_shape`（5 内置 ID 顺序 + custom/errors 数组）+ `themes_request_carries_path_and_theme_id`，双端各跑（mac 63 / win 2 themes 用例）。

### T005 关键登记（含 F5 追踪）

- **F5 追踪结论（双写流）**：① 初始化：`getSettings()` → daemon Settings 是权威源，`isColorTheme(s.color_theme)` 通过则 `setColorTheme` 覆写 localStorage+state（Settings.tsx:183）；② 切换：`setColorTheme(value)`（localStorage 同步先行）→ `await update({color_theme})`（daemon 异步、跨设备同步）→ 失败回滚；③ 跨窗口：storage 事件（useTheme.ts）；④ 渲染：`app ${theme} theme-${colorTheme}`。语义：localStorage=即时 UI 源，daemon=持久/同步源。
- **core 前置（必需，否则 custom: 值无法持久化）**：`validate_user_values` 现接受 `custom:{id}`（`themes::is_custom_theme_preference`，id 按 R001 规则）；`ColorThemeContract` 改手写 JsonSchema（string+pattern）；`settings.schema.json` 经 `generate-settings.mjs` 重生成（仅该定义变化）；`settings.generated.ts` 的 `color_theme` 变 `string`。错误消息更新（K004 面：仅无效值时展示）。core +1 测试（323）。
- **useTheme 扩展**：`isColorTheme` 接受内置或 `^custom:[a-z0-9][a-z0-9-]{0,31}$`；`readStoredColorTheme` 对未知 custom 值保持原值；`resolveColorTheme(value, availableIds)` 应用时回退（Settings/History 的 class 用 resolvedColorTheme）；`listThemes()` 载入 custom 清单 + 错误标记；storage 事件同步 custom 值。
- **R006 注入**：`utils/themeCss.ts` 纯函数（24 令牌 + rgba 合成 + 2 字体变量 + structural borderRadius/shadow:false，未知键 console.warn）；useTheme useLayoutEffect 在 `.app` 上 setProperty/removeProperty（切回内置全清）；注入 effect 先于背景 effect，模式切换（含跟随系统）重注入。**"28 变量"= 24 令牌 + 2 字体 + 2 structural（文档"28"为约数的落实口径，R011 文档任务统一表述）**。
- tailsyncClient.ts 新增 ThemeEntry 等类型与 listThemes/importTheme/deleteTheme/revealThemesDir 4 wrapper。

### T006 关键登记

- 设置页自定义主题组：内置 5 张现有逻辑零改动；自定义卡片 div 结构（内层 select 按钮 + 删除按钮，避免按钮嵌套），预览块用 `customPreviewStyle` 内联 24 令牌 + `--preview-*` 映射（不依赖 settings.css per-theme 块，F8）；错误卡片灰显（title=文件名，`Invalid` 徽标）。
- 交互：选择 `custom:{id}` 走既有 handleColorThemeChange（localStorage+daemon 双写）；导入=plugin-dialog（.json 过滤）→ importTheme → refreshCustomThemes + saved toast，失败 toast 显示 daemon reason；删除=window.confirm → deleteTheme → refresh，删除激活主题时回退 tailsync；打开文件夹=revealThemesDir。
- useTheme 新增 `refreshCustomThemes`（挂载载入与导入/删除后刷新共用）。
- i18n 8 个新键双语（en/zh-CN）；settings.css 追加 custom-theme-card/delete/actions 样式。

### T007 关键登记

- **选型（R008"或等价结构"）**：`TailSyncColorTheme` 枚举 API 保持原样（String raw/CaseIterable/init(storedValue:)/全部访问器），数据改为查 `definition` 表；自定义路径由等价结构 `TailSyncThemeSelection { builtin, definition? }` 承载（`init(storedValue:catalogue:)` 解析，未知值应用时回退 tailsync，存储值不改写）。现有调用点（Settings.swift rawValue、Loc.swift、SettingsView/HistoryView）零改动编译通过。
- `TailSyncThemeDefinition`：palette×2/metrics/typography/fonts/localizedName，`Decodable`（CSS 令牌名→Swift 字段映射：brand→accent、borderStrong→border、bgInput→softSurface(+opacity)、textPrimary 等带 opacity；缺令牌/坏 hex 拒绝解码）。
- `ThemeColorSpec`：JSON 裸串或 `{hex,opacity?}` 双写法。
- 内置表值 = 重构前 switch 字面量逐字转录；`testBuiltinDefinitionsMatchThePreRefactorSnapshot` 逐字段锁定。
- ApiClient `listThemes()`（`"cmd": "list_themes"`，逐条解码、坏条目自弃）→ 漂移脚本 Swift 命令 38→39。
- 待 T008：modifier/Loc/SettingsView 接 `TailSyncThemeSelection` + daemon 目录。

### T008 关键登记

- **custom 应用**：`TailSyncThemeModifier` 用 `TailSyncThemeSelection(storedValue:catalogue:)` 解析（目录来自 Loc），未知 custom 值应用时回退 tailsync；新增 `tailSyncSelection` 环境键；HistoryView 子视图改用 selection（metrics/fonts 走定义）。
- **Loc**：`customThemes`/`themeErrors` @Published + `refreshCustomThemes()`（启动与导入/删除后调用）；`normalizeColorTheme` 保留 `custom:` 前缀（`reload()`/SettingsView `load()`/`applyPersistedSettings` 三处归一化不再摧毁 custom 值——原实现会把 custom:studio 直接回退成 tailsync）。
- **SettingsView**：自定义主题区（标题+描述、错误灰卡（file+Invalid 徽标）、自定义卡片（定义 palette 预览+名称+选中勾+trash 删除钮）、回退警示 Label（所选 custom: 不在目录时）、导入（NSOpenPanel .json → importTheme → refresh + saved toast，失败 actionErrorMessage 显示 daemon reason）、打开文件夹（revealThemesDir）、删除（NSAlert 确认 → deleteTheme → refresh；删除激活主题回退 tailsync 并保存）。
- **回退路径手动验证步骤（UNVERIFIED 标注，实机执行）**：① 两台设备，A 导入主题并选择 `custom:x`（A 正常显示）；② 同步后 B 的 color_theme=custom:x 但无文件：设置页出现回退警示、应用显示默认主题、设置页不崩溃；③ B 手动放置同 ID 主题文件后重开设置页：目录加载后自动恢复 custom 主题；④ 删除激活中的自定义主题：应用回退默认主题且 daemon 设置同步更新；⑤ 导入非法 JSON：toast 显示具体 reason。
- i18n：9 个新键双语（en/zh-CN）。

### T009 关键登记（最终审计）

**全量门禁 fresh 重跑（T009 末）**：core 323 passed/1 ignored + clippy 零警告；windows 129/129 + build + lint（0 error，1 个既有 warning）；swift 35/35；macos daemon 63 passed/1 ignored + cargo check；漂移 PASS（42 Swift 命令）。

**R010 安全证据（grep 输出见 T009 报告）**：全前端无 innerHTML/dangerouslySetInnerHTML；主题值仅经 `setProperty`/`removeProperty` 注入；reveal 路径来自 `themes_dir()` 服务端常量、`Command::new("explorer"/"open").arg()` 单参数无 shell；导入=`fs::read`→校验→`fs::write` 复制；扫描与导入扩展名白名单 `.json`；themes 目录 0700（`themes_dir_is_created_with_0700_permissions` 测试实证）；Swift 侧主题值仅经 `Color(rgb:)` 值类型。

**R011 文档**：THEMING.md 重写——§0 改为"已支持运行时主题"（含同步语义：选择随设置同步、文件不同步、缺文件回退默认）；附录替换为正式规范 §3（目录/格式/24 令牌字段名/校验规则/限额/分享/安全）；令牌计数修正为 24（原"28"为约数）；README 核心功能加自定义主题条目；grep 无遗留"未实现"表述；字段名与实现逐一核对一致。

**主题专项审计**：内置 5 主题双端 light/dark 值级回归——Swift 快照测试逐字段锁定（palette×15 色+7 opacity+metrics+typography+fonts 等于重构前字面量）；Windows art-direction.css/theme.css/COLOR_THEMES 全程零改动；像素级截图对比标注 **UNVERIFIED（手动步骤）**：双端 light/dark × 历史/设置窗口截图与改动前基线对比 + 跟随系统 + 双窗口联动 + 双端对比度抽检。

**REQUIREMENTS_COVERAGE_AUDIT**：R001-R011 全部 PASS（逐条证据见报告）；UNVERIFIED 仅实机/GUI 项（Windows 导入对话框、macOS 回退手动验证、截图对比），均登记手动步骤。PROJECT_STATUS: COMPLETE（无 FAIL/未闭环项）。

### 验收结论（审查方复核）

**ACCEPTED** —— 审查方亲自重跑全部 7 项回归基线通过；R001-R011 逐条核验通过；2 处 ALLOWED_FILES 越界（identity.rs `pub(crate)` 提升、crypto.rs/schema/generated.ts 契约放宽）判定合理且必要（后者修正了设计文档的错误假设——旧 schema 封闭枚举会拒绝 `custom:` 值）。审查方指出的一次 cargo test 未真正执行（工作目录错误 + 管道退出码掩盖）属实：发生在 T005 验证链中，当时已当场发现并用正确 workdir 重跑，此后所有报告数字均来自真实执行；最终 323 由独立重跑确认（基线 285 + themes.rs 37 + crypto.rs 1 = 323，非审查表所记 214/109，特此更正）。

**待人工 GUI 冒烟（审查方执行，本环境无 GUI）**：① Windows 导入→应用→light/dark 跟随→切回无残留→删除；② macOS 同流程（NSOpenPanel）；③ 双端同步回退（A 设自定义主题 → B 无文件显示默认 + 设置页标注、不崩溃）。


---

## 背景图功能（扩展批 T001-T009，进行中）

> 第二档扩展：自定义主题支持内嵌背景图（单 JSON 文件、base64、PNG/JPEG 白名单）。格式向后兼容（无背景字段旧主题行为不变）。

### 进度

| 任务 | 需求 | 状态 | 证据 |
|---|---|---|---|
| T001 | R002 图片头解析器 | **PASS** | `validate_image_payload`（PNG 签名+IHDR、JPEG SOI+SOF0/1/2 扫描，仅读头不解码）+ 7 测试（真实 1×1 PNG/合成 JPEG/魔数不匹配/小文件大尺寸炸弹/6000×4000 边界/畸形流/ImageMime 白名单）；core 330 passed + clippy 零警告 |
| T002 | R001 BackgroundSpec 与校验 | **PASS** | `ImageSpec`/`BackgroundMode`/`ThemeBackground`（serde camelCase+deny_unknown_fields，`background` 可选+skip 序列化）；校验：image⇔scrim 配对、scrim opacity∈[0.5,0.95]（缺失拒绝）、严格 base64、解码 ≤3MB、R002 头校验复用；文件上限条件化（无图 64KB / 含图 4MB，含 load_theme_file metadata 预检）；13 测试全过；core 343 passed + clippy 零警告 |
| T003 | R003 清单瘦身+按需取图 | **PASS** | `ThemeEntry.background` 仅元数据（hasImage/scrim/mimeType，无 dataB64，skip_serializing_if）+ `theme_background_at(id, light, base)`（读盘→复用 R001/R002 校验→解码字节；无图/缺文件 None，无效/保留 ID Err，读时重校验）；6 测试（瘦身序列化体积对比、按需字节一致、双模式、无图/缺文件 None、ID 拒绝、篡改重校验）；core 349 passed + clippy 零警告 |
| T004 | R004 daemon 双端命令 | **PASS** | 双端 `get_theme_background(theme_id, mode)`（routes.rs arm + commands.rs #[tauri::command] + lib.rs 注册；Request 增 `mode` 字段；mode 白名单 light/dark；返回 {mimeType,dataB64} 或 None）；list_themes 路径无 dataB64（grep 证据仅 4 处均在 get_theme_background 分支）；macos daemon 63 passed、双端 cargo check、漂移 PASS（42 Swift 命令） |
| T005 | R005 Windows 注入+分层背景 | **PASS** | art-direction.css `.app` 分层背景（background-color + scrim 同色渐变层 + image 层，双变量缺省时渲染等价）；themeCss `backgroundCssPairs`（data URL 仅由 daemon 校验返回的 mimeType+字节拼装）+ 清理清单扩至 30 项；useTheme 按需取图 effect（会话缓存、切主题即弃、取消竞态、暗色重取）；4 新用例（注入双变量/无背景不取/切内置清理/暗色重取+缓存）；windows 133 passed + build + lint |
| T006 | R006 Windows 设置页指示 | **PASS** | 自定义卡片背景指示：`backgroundIndicator`（元数据仅取任一有图模式的 scrim，light 优先，零取图）+ 角标（`settings.customThemeHasBackground` 双语）+ 底部 scrim 色条；3 新用例（无双模式有/仅一侧有/无背景三态 + 无取图断言）；windows 136 passed + build + lint |
| T007 | R007 macOS 定义与应用 | **PASS** | Definition 扩展背景元数据（hasImage/scrim/mimeType 每模式，可选字段旧格式 nil）；modifier ZStack 分层背景（window 色→图(cover)→scrim，无图时与原渲染一致）+ `.task(id: selection:mode)` 按需加载；ApiClient.getThemeBackground（base64→Data）；Loc 安全解码（CGImageSource 先读尺寸后解码，6000/24MP 二道防线）；3 新测试（元数据解码/缺字段 nil/安全解码）；swift 41 passed + build；漂移 PASS（43 Swift 命令） |
| T008 | R008 macOS 设置页指示 | **PASS** | `TailSyncThemeDefinition.backgroundIndicatorScrim`（light 优先/dark 回退/无图 nil，与 R006 同语义）+ SettingsView 卡片角标（`settings.customThemeHasBackground` 双语）+ 底部 4pt scrim 色条（元数据零取图，allowsHitTesting(false)）；1 新测试（四态选择）；swift 42 passed + build；漂移 PASS（43 Swift 命令） |
| T009 | R009/R010 安全证据+文档+示例 | **PASS** | 安全 grep 证据齐备（schema 无 URL 字段/data URL 唯一构造点 themeCss.ts:159/清单无字节/SVG·GIF 校验层拒绝双测试）；THEMING.md §3.5 背景图规范（字段/限额/scrim/安全/制作建议）+ §3.4 安全条目扩展；docs/examples/spider-man-city.json（基于既有 spider-man 主题 + 代码生成夜景渐变 PNG 3.2KB + 蓝 scrim 0.85/0.9）经校验测试 + 写入真机主题目录；README 无过时表述；全门禁 fresh 全绿 |

### T001 关键登记

- 解析器只读容器头（PNG: 8 字节签名 + IHDR 宽高；JPEG: SOI + 跳过 APP/TEM/RST 段、命中 SOF0/1/2 取宽高，EOI/SOS 在前报"no SOF"），零像素解码、零依赖。
- 限额常量：`MAX_IMAGE_DIMENSION=6000`、`MAX_IMAGE_PIXELS=24_000_000`、`MAX_IMAGE_BYTES=3MB`（T002 用）。
- `ImageMime::{Png,Jpeg}` + `mime_type()`/`parse()`，SVG/GIF/WebP 等一律拒绝（R10 前置）。
- 测试用真实 1×1 PNG（base64 字面量解码，防转录漂移）+ 代码构造的合成 JPEG 头；炸弹样本覆盖单边 6001、36MP（6000×6000）、零维度。

### T002 关键登记

- **DECISION**：scrim 无 image 也拒绝（"scrim requires an image"）——渲染层只支持"图片+scrim"配对（R005/R007 契约），scrim-only 模式不入渲染范围；scrim.opacity 必填且 ∈ [0.5, 0.95]（缺失即拒绝，可读性不协商）。
- **尺寸上限条件化**：`validate_theme_bytes` 预检改为 4MB（`MAX_THEME_FILE_SIZE_WITH_IMAGE`），解析后按 `has_background_image()` 分流：无图文件仍强制 64KB（错误消息不变）；`load_theme_file` metadata 预检同步 4MB。
- **>3MB 图片上限的可达性**：单图 3MB 解码 → base64 4.2MB 文件必先触发 4MB 文件上限，故图片字节上限直测内部 `validate_background`（FACT，测试注释说明）；双图场景下 3MB 上限仍具防御价值。
- 旧格式回归：legacy 主题无 background 字段照常通过、round-trip 不产生 background 键（skip_serializing_if）。

### 背景图扩展 T009 关键登记（最终审计）

**REQUIREMENTS_COVERAGE_AUDIT（R001-R010 全 PASS）**：
- R001 PASS（T002：BackgroundSpec 13 测试）｜R002 PASS（T001：头解析 7 测试）｜R003 PASS（T003：瘦身+按需取图 6 测试）｜R004 PASS（T004：双端命令+Request.mode+注册）｜R005 PASS（T005：分层背景+注入 4 测试）｜R006 PASS（T006：设置页指示 3 测试）｜R007 PASS（T007：macOS 定义+安全解码 3 测试）｜R008 PASS（T008：macOS 指示 1 测试）｜R009 PASS（本任务 grep 证据）｜R010 PASS（本任务文档+示例）
- Coverage：10/10 PASS，0 FAIL / 0 BLOCKED。

**专项审计**：① 无背景/旧格式主题渲染一致性——Windows：CSS 双变量缺省等价（.app 分层规则不注入时 = 原 background 渲染）+ 无背景零注入用例 + 内置主题零改动；macOS：内置快照回归 42/42 全绿 + modifier 无图分支 == 原 `palette.windowColor`。② list_themes 体积同量级——T003 测试实证：entry 序列化 <8KB vs 含图主题文件 >16KB（元数据仅 hasImage/scrim/mimeType）。

**安全红线 R10 落地核对**：schema 无 URL/路径字段（grep 空）；data URL 构造点唯一（themeCss.ts:159，输入=daemon 校验返回）；list 无字节（dataB64 仅 4 处均在 get_theme_background 分支）；SVG/GIF 校验层拒绝（ImageMime::parse + 反序列化 + 2 个拒绝测试）；图片字节仅经服务端魔数/尺寸校验后到达渲染层（core 头解析 + macOS CGImageSource 尺寸先行二道防线）。

**示例主题**：docs/examples/spider-man-city.json（11.5KB：既有 spider-man 主题全字段 + structural + 双模式背景；代码生成 900×600 夜景渐变 PNG 3.2KB ≤200KB 建议；scrim #0b1e3d@0.85 / #060b18@0.9）——`example_spider_man_city_theme_passes_validation` 实证通过；已安装至 `~/Library/Application Support/com.tailsync.TailSync/themes/` 供真机验证。

**全量门禁 fresh（T009 末）**：core 350 passed/1 ignored + clippy 零警告；windows 136/136 + build + lint（0 error，1 既有 warning）；swift 42/42；macos daemon cargo check；漂移 PASS（43 Swift 命令）。

**PROJECT_STATUS: COMPLETE**（背景图扩展批 R001-R010 全部闭环；遗留仅实机视觉项：背景渲染效果、设置页指示外观、示例主题双端加载）。

---

## 自定义主题系统验收批（R001-R008，本轮）

> 验收要求：实际修改代码 + 回归测试 + 完整验证（`npm test` / `npm run build` / `npm run lint`、`cargo fmt --check` / `cargo test`、`swift test`、`git diff --check`）。无 "untested => done"。

### 进度

| 需求 | 内容 | 状态 | 证据 |
|---|---|---|---|
| R001 | macOS HistoryView 背景图可见 | **PASS** | HistoryView 移除根/列表不透明 `palette.windowColor` 覆盖（注释说明窗口背景归 modifier）；`ThemeBackgroundTests.testThemedModifierRendersBackgroundImageVisibly`（ImageRenderer 渲染 120×90 视口，红像素 >1000 实证背景图可见）+ `testHistoryViewDoesNotCoverThemedWindowBackground`（源码模式回归，旧实现必失败） |
| R002 | macOS 背景异步竞态消除 | **PASS** | `Loc.loadThemeBackground` 单调 generation 守卫：旧请求不得覆盖新请求；内置切换立即清空；fetch/解码失败清空旧图；仅最新发布；`backgroundFetch` 闭包可注入（无 daemon/无 sleep）。4 个确定性竞态测试（A 在途→内置切换→A 迟到不得复活；A/B 乱序仅 B 生效；失败清除；模式切换旧结果丢弃）全部经 FetchGate（CheckedContinuation）驱动 |
| R003 | Windows 缺失主题警告 | **PASS** | `colorTheme` 为合法 `custom:{id}` 但目录缺文件：应用回退 tailsync（`resolveColorTheme`，既有）+ 设置页本地化警告横幅（含缺失 ID，`role="alert"`）+ 存储值不改写（daemon 校验层本就保留，测试断言无 `update_settings` 写入）。i18n en/zh-CN `settings.colorThemeMissing`；Settings 4 新用例（缺失警告/存在不警告/内置不警告/导入后消失） |
| R004 | 主题名按字符计数 | **PASS** | `is_valid_name_value`：`chars().count()`（Unicode 标量）≤64，非字节数；64 中文/64 表情通过、65 失败、ZWJ 序列按标量计；文件字节上限不变。2 新测试（含 `validate_theme_bytes` 端到端） |
| R005 | 导入原子化+并发安全 | **PASS** | `lock_themes_dir`（fs2 `lock_exclusive`，跨端，进程退出自动释放）串行化 count/conflict；临时文件 `.name.json.tmp-{pid}-{rand}`（非 .json 后缀，list 不可见）写入+`sync_all`+`rename` 原子安装；失败清理临时文件；delete 同锁。6 新测试：同 ID 8 线程恰好 1 成功 7 冲突；40 异 ID 并发恰好 32 成功 8 超限；导入/删除同 ID 不撕裂（终态一致）；临时文件不可见于列表且成败皆清理；已有目标永不覆盖 |
| R006 | macOS 字体候选列表 | **PASS** | `FontCandidates.parse`（逗号拆分+trim+去空）/`firstAvailable`（NSFont 探测）/`resolve`；`displayFont`/`readingFont` 经候选解析，全缺回退系统字体。4 新测试（顺序/trim/单值/空、真实字体 Helvetica 与不存在字体、resolve 原始值、定义级候选解析）；THEMING.md §3.6 登记 |
| R007 | Windows metrics/typography 映射 | **PASS** | 确定性字段→CSS 变量表（controlRadius→`--radius-sm`；cardRadius→`--radius-md`+`--window-radius`；rowPadding→双行距；shadowRadius→`--shadow-md` 计算值/0 时 none；sectionTitleSize→`--font-size-section`；historyContentSize→`--font-size-content`；searchSize→`--search-font-size`；searchUsesDisplayFont→`--search-font-family`；uppercasesSectionTitles→`--section-title-transform`）。仅 `style.setProperty`；切回内置全清（`allCustomThemeProperties` 含全部新名）；搜索框/分组标题消费新变量；主题 JSON 格式零改动。useTheme 测试更新（28→39 变量）+ 2 新用例 |
| R008 | 工程收尾 | **PASS** | `cargo fmt --all -- --check` 零 diff；`git diff --check` 干净；THEMING.md §3.6/§3.3 更新；本文件更新；全量门禁 fresh 全绿（见下） |

### 全量门禁（验收批末，fresh 顺序执行）

- **Windows**：`npm test -- --run` 25 文件 174/174（基线 137 +37：R003 Settings 4、R007 useTheme 2+既有 28→39 断言更新、preview renderer 套件 36 经依赖安装后恢复运行）；`npm run build` 成功；`npm run lint` 0 error（1 既有 warning：`routeSupportsLatencyTest` 未用参数）。
- **Rust core**：`cargo fmt --all -- --check` 零 diff；`cargo test` 365 passed / 1 ignored（基线 285 +80：R004 2、R005 6、背景图批 72）；clippy 零警告。
- **macOS Swift**：`swift test` 80/80 全绿（基线 42 +38：ThemeBackgroundTests 6、ThemeTests R006 4、预览窗套件 24、FilterBar 3、AppBehavior 1）；`swift build` 成功；无 XCTest 调度楔死（全套 1 秒级完成）。
- `git diff --check` 零 whitespace 错误。

### 验收批关键登记

- **修复上游预览提交（7cb9387）的编译阻断（非本任务需求，但阻断 `swift test` 门禁）**：`HistoryPreviewData`/`HistoryPreviewBatchNavigation`/`HistoryPreviewMaterial` 三个类型缺失 → 新增 `Models/HistoryPreviewData.swift`（按全部使用点重建契约：kind/name/sizeBytes/data/entryId/batch、`maxBytes=64MiB`、material 枚举 text/image/pdf/quickLook/unsupported）；`HistoryPreviewTextView` 两处字符串插值内错误转义 `\"` → 修正；`PDFThumbnailView.layoutMode`（本 SDK Swift overlay 不可见，默认即 vertical）删除该行；WindowController 默认参数/`deinit` 的 MainActor 隔离错误修复；`TailSyncApp.showHistory` 的 MainActor 调用包 `Task { @MainActor }`；3 个预览测试夹具/断言修复（"script" 子串误伤 `javascript:` 纯文本 → 改断言 `<script` 标签；PDFPage 需要真实像素；store 目录由夹具自建）。用户工作树与既有主题改动零覆盖（仅新增/最小修正）。
- **R005 锁语义**：flock/LockFileEx 按打开文件描述符独立互斥（同进程多线程亦互斥）；`.themes.lock` 常驻目录但非 `.json`，列表不可见、不计入 32 限额。
- **R007 变量落点**：`--search-font-size`/`--search-font-family`/`--section-title-transform` 为新增基础变量（默认值 = 原硬编码 13px/var(--font-ui)/uppercase，内置主题行为零变化）；消费点 `.search-bar input`、`.date-header`、`.setting-group-header h3`。
- **验证环境事实**：macOS 测试机 SDK 15.5 / macOS 27；Windows webview 依赖 `marked`/`dompurify` 经 `npm install` 补齐（package.json 早已声明）。
