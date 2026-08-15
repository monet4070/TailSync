# TailSync Maintainability Refactor — TAURI_API_MATRIX (T240)

> 生成时间：2026-08-15
> 依据：T000 前端 invoke 全量清单（31+14 调用点，行号实测）+ commands 层全量盘点（35 命令，行号实测）+ Settings.tsx 内联 interface（L41-186 区域，实测）。
> 只读交付，未修改代码。本矩阵是 T241+（typed client 迁移）的唯一事实来源。

---

## 1. Windows Tauri 命令全集（35，全部 `Result<_, String>`）

注册点：`windows/src-tauri/src/lib.rs` L579-622 单点 invoke_handler。领域分组（行号 = commands.rs）：

### History（10）
| 命令 | 输入 | 输出 | 前端调用方 | 厚/薄 |
|---|---|---|---|---|
| `get_history`（W126-147） | keyword/category/start_time/end_time/limit/offset 可选 | `Vec<HistoryEntry>` | 无（后端/测试） | 薄 |
| `get_history_page`（W149-175） | 同上（limit 默认 30, offset 分页） | `HistoryPage {entries,total,has_more}` | History.tsx:706 | 薄 |
| `get_history_capabilities`（W177-180） | — | `HistoryCapabilities`（分类/日期能力） | History.tsx:672 | 薄 |
| `get_migration_diagnostics`（W182-192） | — | `MigrationDiagnostics`（unresolved_count） | History.tsx:662 | 薄 |
| `search_history`（W195-203） | keyword | `Vec<HistoryEntry>` | 无 | 薄 |
| `delete_entry`（W206-212） | id: i64 | `()` | History.tsx:846 | 薄 |
| `clear_history`（W215-221） | — | `()` | History.tsx:858 | 薄 |
| `restore_entry`（W225-350） | id: i64 | `()` | History.tsx:831 | **厚**（剪贴板恢复状态机） |
| `set_history_pinned`（W802-815） | id, pinned | `()` | History.tsx:891 | 薄 |
| `restore_file_batch`（W827-835） | batch_id: String | `()` | History.tsx:881 | 薄 |

### Devices / peers（4）
| 命令 | 输入 | 输出 | 前端调用方 | 厚/薄 |
|---|---|---|---|---|
| `get_peers`（W353-363） | — | `PeersResponse` | Settings.tsx:309/657 | 薄 |
| `refresh_peers`（W366-371） | — | `PeersResponse` | Settings.tsx:491 | 薄 |
| `test_connection`（W374-390） | address: String | `ConnectionTestResult` | Settings.tsx:642 | 薄 |
| `toggle_peer`（W491-506） | hostname, enabled | `()` | Settings.tsx:519 | 薄 |

### Pairing（7）
| 命令 | 输入 | 输出 | 前端调用方 | 厚/薄 |
|---|---|---|---|---|
| `enable_pairing`（W447-452） | — | `PairingStatus` | Settings.tsx:562/581 | 薄 |
| `get_pairing_status`（W454-459） | — | `PairingStatus` | Settings.tsx:534/588（轮询 1s） | 薄 |
| `start_pairing`（W461-474） | address: String | `PairingStatus` | Settings.tsx:583 | 薄 |
| `confirm_pairing`（W476-481） | — | `PairingStatus` | Settings.tsx:618 | 薄 |
| `cancel_pairing`（W483-488） | — | `PairingStatus` | Settings.tsx:602 | 薄 |
| `trust_peer`（W393-430） | hostname, public_key, address 可选 | fingerprint: String | 无（SwiftUI 侧） | **厚** |
| `forget_peer`（W433-445） | hostname | `()` | Settings.tsx:628 | 薄 |

### Settings（4）
| 命令 | 输入 | 输出 | 前端调用方 | 厚/薄 |
|---|---|---|---|---|
| `get_settings`（W564-568） | — | `SettingsData` | Settings.tsx:250/360/411 | 薄 |
| `update_settings`（W571-609） | settings_json: String | `()` | Settings.tsx:347 | **厚**（快捷事务+DB 限额+模式编排） |
| `set_sync_shortcut`（W545-561） | shortcut: String | `()` | Settings.tsx:692 | 薄（事务在内部 helper） |
| `get_sync_state`（W509-516） | — | `{enabled, shortcut}` | 无 | 薄 |

### Sync（4）
| 命令 | 输入 | 输出 | 前端调用方 | 厚/薄 |
|---|---|---|---|---|
| `set_sync_enabled`（W519-522） | enabled | `()` | Settings.tsx:885 | 薄 |
| `toggle_sync`（W524-527） | — | bool | 无（tray） | 薄 |
| `suspend_sync_shortcut`（W529-534） | — | `()` | Settings.tsx:717/770 | 薄 |
| `resume_sync_shortcut`（W536-543） | — | `()` | Settings.tsx:276/703/742 | 薄 |

### Storage（4）
| 命令 | 输入 | 输出 | 前端调用方 | 厚/薄 |
|---|---|---|---|---|
| `get_storage_status`（W729-732） | — | `StorageStatus` | Settings.tsx:262/412 | 薄 |
| `change_storage_location`（W734-799） | parent: String | `StorageMigrationResult` | Settings.tsx:408 | **厚**（迁移状态机） |
| `delete_old_storage`（W817-824） | path: String | `()` | Settings.tsx:430 | 薄 |
| `cancel_file_batch`（W699-704） | batch_id: String | `()` | History.tsx:903 | 薄（实现共享） |

### Updater（3）
| 命令 | 输入 | 输出 | 前端调用方 | 厚/薄 |
|---|---|---|---|---|
| `get_update_status`（W873-879） | — | `UpdateStatus`（版本+updates_enabled） | Settings.tsx:263 | 薄 |
| `check_for_update`（W881-886） | — | `UpdateInfo \| null` | Settings.tsx:902 | 薄 |
| `install_update`（W888-891） | — | bool | Settings.tsx:916 | 薄 |

### Clipboard / misc（7，其中 3 已计）
| 命令 | 输入 | 输出 | 前端调用方 | 厚/薄 |
|---|---|---|---|---|
| `get_file_progress`（W690-697） | — | 进度对象 | History.tsx:793（轮询 800ms） | 薄 |
| `get_image_data`（W669-687） | id: i64 | `ImageThumbnail`（b64+尺寸） | History.tsx:634 | 薄 |
| `get_version`（W852-857） | — | `{version}`（剪贴板版本） | History.tsx:770（轮询） | 薄 |
| `get_sync_warning`（W859-863） | — | `SyncWarning \| null` | History.tsx:779（轮询） | 薄 |
| `open_history_window` / `open_settings_window`（W612-666） | — | `()` | 无（tray/事件） | 薄 |

## 2. 前端调用点映射（45 处）

- **Settings.tsx（31 处，L250-916）**：get_settings×3、get_storage_status×2、get_update_status×1、update_settings×1、change_storage_location×1、delete_old_storage×1、get_peers×3、refresh_peers×1、toggle_peer×1、get_pairing_status×3、enable_pairing×2、start_pairing×1、cancel_pairing×1、confirm_pairing×1、forget_peer×1、test_connection×1、set_sync_shortcut×1、suspend_sync_shortcut×2、resume_sync_shortcut×3、set_sync_enabled×1、check_for_update×1、install_update×1。
- **History.tsx（14 处，L621-903）**：get_settings×1、get_image_data×1、get_migration_diagnostics×1、get_history_capabilities×1、get_history_page×1、get_version×1、get_sync_warning×1、get_file_progress×1、restore_entry×1、delete_entry×1、clear_history×1、restore_file_batch×1、set_history_pinned×1、cancel_file_batch×1。
- **事件订阅**：`sync-state-changed`（Settings.tsx:281）、`peer-health-changed`（Settings.tsx:327）；`getCurrentWindow().hide()`（两页面标题栏）。

## 3. 类型现状（R008 缺口）

- 带 TS 泛型的调用：**28 处**（Settings 20 + History 8，实测 grep），覆盖 14 种泛型类型：`SettingsData`、`StorageStatus`、`UpdateStatus`、`StorageMigrationResult`、`PeersResponse`、`PairingStatus`、`ConnectionTestResult`、`UpdateInfo | null`、`boolean`（Settings）；`HistoryCapabilities`、`HistoryPageResult`、`ImageThumbnail`、`MigrationDiagnostics`、`SyncWarning | null`（History）。
- **未类型化（约 17 处调用）**：update_settings/toggle_peer/forget_peer/delete_old_storage/set_sync_enabled/set_sync_shortcut/suspend|resume_sync_shortcut/enable|start|cancel|confirm_pairing/refresh_peers/restore_entry/delete_entry/clear_history/restore_file_batch/set_history_pinned/cancel_file_batch/get_version/get_file_progress —— 参数/返回形状只能从调用点与测试 fixtures 推断。
- TS 侧契约锚点：Settings.tsx 内联 interface（PeerDevice/PeerRoute/PeersResponse/PairingPeerStatus/PairingStatus/StorageStatus/StorageMigrationResult/UpdateStatus/UpdateInfo/ConnectionTestResult/ConnectionTestState，L41-186）；History.tsx 的 HistoryPageResult `{entries,total,has_more}`（L697-710 形状）；`settings.generated.ts` 的 SettingsData（生成契约）。
- 错误行为：全部 `Err(String)` → 前端 catch 后 toast/回滚（update() 双回滚、乐观写模式）；无命令级错误码。

## 4. T241+ 迁移设计（typed client）

目标模块：`windows/src/tailsyncClient.ts`（新文件），按领域分组导出：

```text
tailsyncClient
├── settings: getSettings / updateSettings(next: Partial<SettingsData>) / setSyncEnabled / setSyncShortcut /
│             suspendShortcut / resumeShortcut / getUpdateStatus / checkForUpdate / installUpdate
├── storage:  getStorageStatus / changeStorageLocation(parent) / deleteOldStorage(path)
├── devices:  getPeers / refreshPeers / togglePeer / testConnection / forgetPeer
├── pairing:  enablePairing / getPairingStatus / startPairing / cancelPairing / confirmPairing
├── history:  getHistoryPage(query) / getHistoryCapabilities / getMigrationDiagnostics / restoreEntry /
│             deleteEntry / clearHistory / setHistoryPinned / restoreFileBatch / getImageData / getSyncWarning
└── transfer: getFileProgress / cancelFileBatch / getVersion
```

- 迁移顺序（每 feature 一批，T241+ 逐批）：
  1. **updater**（3 命令，Settings 中隔离度最高，无共享 state）—— 试点。
  2. **storage**（4 命令，独立 state）。
  3. **history 读侧**（get_history_page/capabilities/diagnostics/image_data/version/sync_warning/file_progress —— History.tsx 无状态耦合）。
  4. **history 写侧**（restore/delete/clear/pin/batch）。
  5. **devices + pairing**（共享 devices state，同批）。
  6. **settings 写侧**（update_settings/set_sync_* —— 依赖 update() 编排，最后）。
- 每个 wrapper：`invoke<Out>(cmd, args)` 内部封装；页面不再直接 import `invoke`（grep 验证 AC）。
- 类型定义随 wrapper 落位（从 Settings.tsx 内联 interface 迁出，页面 import）；不改 JSON 契约（命令名/参数名不变）。

## 5. API cmd 面（参考，非前端调用面）

api/routes.rs 的 JSON-lines cmd（43 个，含 begin_import/import_chunk/finish_import/migrate_entry/quit/reconnect_peers/clear_all 别名等）服务于 SwiftUI（macOS）与测试，不经 Windows React 前端调用；其类型化属 macOS 侧未来工作（R008 范围外，记录备查）。
