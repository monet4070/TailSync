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
