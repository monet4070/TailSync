# TailSync Maintainability Refactor — COMMANDS RESPONSIBILITY MAP (T300)

> 生成时间：2026-08-15（T300，Phase 3 R009 入口，T260 后实测）
> 依据：T000 BASELINE §8（1774/1374 前端时点）+ T101-T111 迁移后复核 + 本文件全部行号为 **T300 实测**。
> 只读交付。本图是 R009（T301+ 逐领域拆分）的唯一事实来源。修正 BASELINE §8 两处计数：Windows 命令 35→**42**、macOS 28→**30**（按 lib.rs invoke_handler 注册清单实测）。

---

## 1. 命令面（FACT，T300 实测）

| 平台 | commands.rs | 注册命令数 | lib.rs 注册点 | in-file 单测 | 全部签名 |
|---|---|---|---|---|---|
| Windows | 1045 行 | **42** | W lib.rs:579-622 | 5（shortcut_transaction_* L965-1033） | `Result<…, String>` |
| macOS | 676 行 | **30** | M lib.rs:503-534 | 0 | `Result<…, String>` |

- 注解形式：`#[command]`（`use tauri::command`），非 `#[tauri::command]`。
- 两端差异（12 个 Windows-only）：`test_connection`、`get_sync_state`、`set_sync_enabled`、`toggle_sync`、`suspend_sync_shortcut`、`resume_sync_shortcut`、`set_sync_shortcut`、`open_history_window`、`open_settings_window`、`get_update_status`、`check_for_update`、`install_update`。
- macOS 死表面（K003）：SwiftUI 经 ApiClient.swift 直连 TCP 19889，不调用 invoke；macOS commands.rs 对真实 UI 无调用面。

## 2. 领域分组（FACT，行号 = windows commands.rs 实测）

| 领域 | 命令（行号） | 厚度 | 依赖 |
|---|---|---|---|
| history（10） | get_history W128、get_history_page W150、get_history_capabilities W178、get_migration_diagnostics W183、search_history W196、delete_entry W207、clear_history W216、**restore_entry W226-353**、set_history_pinned W802、restore_file_batch W827 | 厚：restore_entry ~128 行（DB + 剪贴板写 + 事件 bump + 回滚状态机） | db、CLIPBOARD_VERSION、materialize_file_batch_paths |
| devices/peers（4） | get_peers W354、refresh_peers W367、test_connection W375、toggle_peer W492 | 薄转发 | network/pool、sync_engine、AppState |
| pairing（7） | enable_pairing W448、get_pairing_status W455、start_pairing W462、confirm_pairing W477、cancel_pairing W484、**trust_peer W394-433**、forget_peer W434 | 厚：trust_peer ~40 行（公钥解码 + 指纹 + 双向写 db） | pairing 状态机、crypto、db |
| settings（4） | get_settings W565、**update_settings W572-612**、set_sync_shortcut W546、get_sync_state W510 | 厚：update_settings ~40 行（合并 + 校验 + 写回 + 事件） | crypto::Settings、db、emit_sync_state、shortcut 事务 |
| sync（4） | set_sync_enabled W520、toggle_sync W525、suspend_sync_shortcut W530、resume_sync_shortcut W537 | 薄（经 set_sync_enabled_for_app/toggle_sync_for_app/register_saved_sync_shortcut） | global_shortcut 插件、emit_sync_state |
| storage（4） | get_storage_status W730、**change_storage_location W735-801**、delete_old_storage W818、cancel_file_batch W700（实现 cancel_file_batch_impl W706-729） | 厚：change_storage_location ~66 行（迁移 + 双回滚）；cancel_file_batch_impl ~23 行 | db、CANCELLED_FILE_BATCHES、FILE_PROGRESS |
| updater（3） | get_update_status W874、check_for_update W882、install_update W889 | 薄（经 crate::updates） | updates 模块、AppHandle |
| clipboard/misc（7） | get_image_data W670（+rgba_to_dib W895/set_clipboard_dib W938）、get_file_progress W691、get_version W853、get_sync_warning W860、open_history_window W613、open_settings_window W642 | 薄 | db、windows 剪贴板（macOS 无） |

## 3. 公共 helper（FACT，拆分时不得重复内联）

- 两端共有（行号：windows / macos）：`cancel_file_batch_impl` W706/M452、`materialize_file_batch_paths` W837/M583、`rgba_to_dib` W895/M613、`set_clipboard_dib` W938/M656。
- Windows 专用（macOS 无对应命令，死表面不承载）：`emit_sync_state` W10、`set_sync_enabled_for_app` W19、`toggle_sync_for_app` W36、`install_sync_shortcut` W52、`register_saved_sync_shortcut` W75、`apply_shortcut_change` W85、`rollback_shortcut` W107。
- api/routes.rs：`peer_snapshot_data` R3、`history_capabilities_data` R1113。

## 4. api.rs 双实现重复点（FACT，routes.rs 行号实测）

`routes.rs` 1120 行单 `handle_cmd` match（R110-1111，43 个 cmd 分支：42 个 cmd 名 + clear_history|clear_all 双名别名；含 ping/get_status/reconnect_peers/migrate_entry/quit；import 3 经 core）。与 commands.rs 重复实现的 5 处（实现路径与命令层不同，拆分时必须同一来源）：

| cmd | routes.rs 实测 | 与 commands.rs 的差异 |
|---|---|---|
| restore_entry | R403-500（~98 行） | 走 sync_engine.restore_*（不直接剪贴板写）→ 剪贴板数据经 db.get_data |
| update_settings | R569-628 | 语义相近：合并 + 校验 + 写回 + sync-state 事件 |
| change_storage_location | R628-717 | 迁移 + 回滚逻辑与命令层重复 |
| get_peers / refresh_peers | R754-778 | peer_snapshot_data 共享，仅编排差异 |
| trust_peer | R819-933（~114 行，含接口推断） | 公钥解码 + 指纹 + 双向 db 写 + network::infer_interface |

- import 命令面已迁 core（T101-T111：`begin_import`/`import_chunk`/`finish_import` 经 `tailsync_core::import::ImportRegistry`，api.rs 仅 re-export，R1021-1025）。
- 共享契约面（Swift 38 命令）由漂移检查器钉扎，API 面拆分不得改 cmd 名/载荷。

## 5. 拆分建议（T301+ 输入，BEHAVIOR_CHANGE_ALLOWED=FALSE）

优先级按 R009 traceability（厚命令优先）+ §20 Maintainability Decision Test：

1. **restore_entry**（W226-353 + R403-500）：两实现路径不同 → 先抽共享状态机（DB 读取 + 剪贴板回滚 + CLIPBOARD_VERSION bump）入 core，命令层/API 层各自薄转发；两端同步改。**→ T304 评审：保持（见 §0）**
2. **update_settings**（W572-612 + R569-628）：合并/校验/写回入 core `SettingsStore`（settings 契约四端钉扎已存在），命令层与 API 层同一入口。**→ T303 已完成（见 §0；shortcut 语义差异经 ShortcutChangeHook 保留）**
3. **change_storage_location**（W735-801 + R628-717）：迁移编排 + 双回滚入 core；注意 db 迁移器已存在。**→ T301 已完成（见 §0）**
4. **trust_peer**（W394-433 + R819-933）：公钥解码/指纹/双向写 + 接口推断入 core；`network::infer_interface` 是唯一平台差异点，需参数化。**→ T302 已完成（见 §0，infer_interface/mode_interface 已在 core peer/directory，无需参数化）**
5. **cancel_file_batch_impl**（W706-729）：已含 CANCELLED_FILE_BATCHES 单例逻辑 → 直接入 core（类似 rate_limit 先例）。**→ 已核：routes 已调用 commands 单实现，无重复（T300 §4 勘误）**

每步验收：core 测试先行（R014）+ 两端 `cargo test --locked` + 漂移检查 PASS + 前端/API 契约不变（38 Swift 命令面）。

---

## 0. 迁移完成状态（T301-T304，2026-08-15）

| # | 目标 | Task | 结果 |
|---|---|---|---|
| 3 | change_storage_location 编排 | T301 | `tailsync_core::db::migrate_storage_with_rollback`（hooks 注入：wait_timeout/has_active_transfers/notify/persist_settings；失败枚举 TimedOut/Migrate/SaveFailedAfterRollback/RollbackAlsoFailed）；命令层 W/M + routes.rs（W/M）四处变薄转发，错误串逐字保持 |
| 4 | trust_peer 编排 | T302 | `tailsync_core::identity::trust_peer`（hostname/self-pairing 校验 + 接口推断 + 快照回滚 + persist hook；失败枚举 InvalidHostname/SelfPairing/Key/Interface/Trust）；命令层 W/M + routes.rs（W/M）四处变薄，表面错误串与响应形状逐字保持 |
| 2 | update_settings 编排 | T303 | `tailsync_core::crypto::apply_settings_update`（prepare 校验 + persist hook + 可选 ShortcutChangeHook + db 限额应用；返回 mode_changed outcome）；命令层 W/M + routes.rs（W/M）四处变薄，三面语义逐字保持 |
| 1 | restore_entry 共享状态机 | T304 | **保持（评审结论）**：写侧机制刻意不同 —— 命令层经 tauri_plugin_clipboard_manager（arboard）+ Win32 CF_DIB 回退（rgba_to_dib/set_clipboard_dib），API 层经 sync_engine.restore_image/restore_text（SyncPlatform 抽象）；统一写侧 = 丢失 CF_DIB 回退（Windows 剪贴板健壮性回归，P0 违反）或拖入 Win32 平台代码（R003 违反）；统一读侧（~15 行）需调和 strict vs 防御性错误处理（损坏 DB 路径行为变化），价值为负（§20/§21 Decision Tests）。macOS 命令层为死表面（K003） |
| 5 | cancel_file_batch_impl | 复核 | 已核为单实现（routes 调用 commands::cancel_file_batch_impl），无重复（T300 §4 勘误） |

**T301-T304 实测：**
- core：264 测试全绿（T301 +5、T302 +6、T303 +6；全局 storage dir 用测试锁串行化）；clippy 0 warning。
- macOS crate：61 测试全绿 + clippy 0 warning；Windows host check（fmt + check --all-targets + test --no-run）通过。
- 漂移检查 PASS（"Cross-platform contract passed…"）。
- R009 收官判定：5 个厚命令 —— 3 迁移入 core（change_storage_location/trust_peer/update_settings）、1 单实现复核（cancel_file_batch_impl）、1 保持评审（restore_entry，T304）；commands.rs 变薄合计 ~145 行（Windows）。
- 行为差异登记：
  - K031（T301）：仅 rollback 的 `spawn_blocking` join-error 路径 —— 命令层原返回裸 join 错误，现与 API 层统一为包装消息；该路径仅在 tokio runtime 关闭时可达，字符串契约其余逐字保持。
  - K032（T302）：空白地址 + auto 模式 —— 两端原将原始地址交给 `infer_interface`（必错），现统一先过滤空白再回落 mode_interface("auto")→"lan"；前端从不发送空白地址（pairingAddressForPeer 已 trim/过滤）。
  - K033（T303）：routes 面原为 commit→mode 反应→db 限额，统一为 commit→db 限额→mode 反应；两阶段互相独立且均在提交后，无可观测差异。
- 两平台 commands.rs 与 routes.rs 的对应改动保持同文（macOS 侧同步应用，避免二次漂移）。

## 6. 约束

- commands.rs 在漂移检查 allowed-drift 清单内（两端不同步字节），但 R002 要求行为契约一致：拆分保持两端同步改、Swift 子串错误契约（K004）不动。
- macOS commands.rs 是死表面（K003）：拆分优先级低于 Windows，但同源逻辑修改必须同步两端避免二次漂移。
- R011 约束：不重构 DB；R022：新增 abstraction 必须通过 §20-§22 Decision Tests。
