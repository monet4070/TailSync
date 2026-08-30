# TailSync 全量代码审查（2026-08-30）——重点：断点续传跨崩溃/重启

> **版本记录**
> - v1（初次审查）：四路并行深查 + 逐条人工复核；发现 P0 编译错误与 batch 重放幂等缺口。
> - v2（提交 `4039413` 后复核）：标签错位已随提交修复（含守卫测试）；Windows 前端 7 个失败测试已修复（199/199）。
> - v3（当前版）：**并入作者的第二轮独立核验结论**——撤销 1 条误报（§1.7 macOS 唤醒恢复）、降级/收窄若干夸大描述、撤回 1 条不充分的修复建议（仅查内存 `completed_batches`），并新增作者发现的更基础 P0（ACK 先于持久化提交）。v3 的所有反转点均已由本审查独立验证（验证链见各节）。

## 〇、总体判断

**断点续传跨崩溃/重启的架构方向正确**：发送端 journal（`data/outgoing-transfers/`，原子写+fsync，private_fs.rs:303-320）+ 接收端 `.part`/`.batch.json` 持久态 + 进程内 claim 防并发重放（outgoing.rs:22-59）；接收端偏移量从 `.part` 文件长度推导而非信任 sidecar（resume.rs:115-120、receive_engine.rs:140-147），恢复后强制全文件校验。`4039413` 的路由重选改动（回退路由上的 FileResume 触发重选首选路由）已带守卫测试落地。

**但"崩溃/重启"主路径上有两个 P0**，且都源于同一类根因——**完成事务不是原子的、身份不是持久的**：

1. 接收端"先 ACK、后异步写库"：DB 写失败时发送端已删 journal，接收端无任何重试（§1.A）。
2. 完成批次被重放时不幂等：重复入库 → 2N 行 → 批次永久损坏（§1.B，已最小复现）。

**核验状态总表**（v3 定稿）：

| 项目 | 结论 | 真实影响 |
|---|---|---|
| §1.1 编译标签 | ✅ 已随 `4039413` 修复 | 当前不存在 |
| §1.A ACK 先于 DB 提交 | **属实，P0**（v3 新增） | DB 写失败后发送端已认为成功，接收端历史缺文件，无自动重试 |
| §1.B 完成批次重放不幂等 | **属实，P0**（已复现） | ACK 丢失/发送端崩溃后批次重复入库，行数 2N，"全部拷贝"/预览/批量收藏持续失效 |
| §1.C 存储不可用通知风暴 | **属实，P1** | 重复哈希+重复通知，可持续到 journal 24h 过期 |
| §1.D 恢复循环重复哈希 | **部分属实，P1** | 完全离线时在 `FileBatchStart` 即失败、不会哈希；连接成功后失败、协议拒绝或 §1.B 重放失败时会重复哈希 |
| §1.E peer 改名后 journal 卡住 | **属实，P2** | 每 2s 空扫描（无传输/哈希），用户看不到原因、无法主动取消 |
| §1.F Windows 文件+路径文本双广播 | **属实，P1** | 同一拷贝产生两条记录，最终剪贴板内容不稳定 |
| §1.G macOS 缺唤醒恢复 | **误报，已撤销** | Swift 层监听 `didWakeNotification` 并调用守护进程断连+恢复 |
| §2.4 Windows 前端测试 | ✅ 已随 `4039413` 修复 | 当前不属于问题 |

测试实测（v3，串行）：core 353 ✓ / macOS Tauri 76 ✓ / Windows Tauri 79 ✓ / Windows 前端 199 ✓。
已知非回归问题：`sync_warning` 测试在并行执行时可能 352/353（单跑通过）——根因是 sync_warning.rs:10 的**单一全局 `LATEST_WARNING` 槽**被四个测试共享并消费，属测试隔离缺陷，应顺手修（如每个测试用例注入独立槽或串行化该模块测试）。

---

## 一、断点续传专项（跨崩溃/重启）

### 1.A 🔴 P0：接收端"先 ACK、后异步写库"——DB 写失败即静默丢批次（v3 新增，根因比 §1.B 更基础）

**代码链**（已逐行验证）：
1. `server.rs:436-467`：`FileBatchComplete` → `finish_file_batch` 返回 `Ok` 后**立即**回 `FileBatchAccept`。
2. `batches.rs:157-196 finish_file_batch`：先删内存态与 `.batch.json`（:182），再调 `platform.files_received()`（:184-193），函数本身对"历史是否写成功"一无所知。
3. `sync_adapter.rs:90-140 files_received`：仅 `spawn` 一个异步任务执行 `add_file_batch_with_status`；失败路径只 `log::error` + 通知（:126-137），**无重试、无状态留存**。

**故障时序**：最后一个文件收完 → `finish_file_batch` 成功返回 → ACK 发出 → 发送端 `mark_outgoing_peer_completed` → journal 全部清除（transfer.rs:169-173）→ 此时接收端 DB 写失败（磁盘满、外置盘拔出、SQLite busy 超时、权限异常、进程退出）→ 接收端历史中**永远没有这批文件**，临时明文文件最终被 `cleanup_expired_transfers` 当孤儿清理；发送端认为已送达，journal 已删，除非用户重新复制否则无法弥补。

**为什么"仅查内存 `completed_batches`"不能修 §1.B**（撤回 v2 报告的建议 1）：`completed_batches` 只保留 10 分钟、跨重启失效；更糟的是它无法区分"已 ACK 且已写库"和"已 ACK 但写库失败"——若按 v2 建议实现，写库失败的批次会被当成已完成，把 §1.A 的丢失**永久固化**。两个 P0 必须一起修，方案见 §三。

### 1.B 🔴 P0：完成批次被重放时不幂等 → 重复入库，批次永久损坏

**入口**：`batches.rs:19 begin_file_batch_at_epoch` 只查 `incoming_batches`、`cancelled_batches` 和磁盘 `.batch.json`，**不查任何"已完成"记录**。

**复现**（作者最小调用链，本审查确认逻辑）：

```text
第一次完成：成功
重放 Start + Meta + Complete：
Err("File batch is incomplete")
```

**可达时序**：接收端完整收完批次 → `finish_file_batch` 删除 `.batch.json` → 发送端在收到最终 ACK 前崩溃（或 ACK 在途丢失）→ 重启后恢复循环重放 → 批次被重新接纳（files 全 `None`）→ 10 分钟内 `completed_transfers`（内存、10 分钟保留，receive_engine.rs:66-78）短路文件数据 → `FileBatchComplete` → "File batch is incomplete" → Reject → 发送端每 2s 重放至 24h；**10 分钟后**去重表过期 → 整文件重收 → `add_file_batch_with_status`（db/files.rs:228）以同一 `batch_id` 再插 N 行（`UPDATE ... SET batch_status='complete' WHERE batch_id=?1`，:289-294，新旧行一起标 complete）→ 行数 2N ≠ `batch_total` N → `materialize_file_batch`（db/files.rs:58 的严格行数校验）**永久报错**："全部拷贝"、预览导航、批量收藏全部失效，直到人工清理数据。

**现有测试未覆盖**：`completed_file_batch_is_idempotent`（sync/tests.rs:445-458）只连续调用两次 `finish_file_batch`，没有重放 `begin_file_batch`，因此未覆盖真实崩溃时序（"连续两次 Complete"经 `completed_batches` 命中返回 `Ok`，是另一条路径）。

### 1.C 🟠 P1：存储不可用时，恢复循环每 2 秒弹通知 + 重复哈希，最长 24 小时

`transfer.rs:89-96`（两端一致）：storage 不可用 → `notify_file_batch_error` → 直接 `return`，发生在 journal 持久化/selection 删除之前 → selection 保留 → 恢复循环（`OUTGOING_RECOVERY_PENDING_DELAY = 2s`，transfer.rs:3）不断重入。且 `prepare_file_batch`（全量哈希）在 storage 检查**之前**（:68-88 早于 :89-96），每轮都白哈希一遍。磁盘满/外置盘拔出场景下，通知理论上限约 4.3 万条/24h。

### 1.D 🟠 P1（修正）：恢复循环重复哈希——仅限"连接成功后失败"的场景

修正 v1 的夸大描述：peer 完全离线时，`queue_peer_batch_frame`（transfer.rs:499）在 `FileBatchStart` 即失败返回，**不会进入** per-file 循环，也就不会执行 `revalidate_prepared_file`（transfer.rs:520-524 → prepare.rs:317-331 全量 blake3）。重复哈希的真实触发条件是：连接成功但后续失败、协议拒绝、或 §1.B 的重放失败循环。`resume_attempts > 32`（transfer.rs:597-603）只限单次调用内，跨轮无退避仍然成立。

### 1.E 🟠 P2（修正）：peer 改名/解除配对后 journal 卡死

`transfer.rs:344-353`：`pending_peers(&peer_names)` 与当前发现 hostname 求交，改名后恒空 → 跳过投递；`all_peers_completed()`（outgoing.rs:113-115）恒 false → journal 永不删除。修正：此场景**通常不会继续传输或哈希**（无 pending peer 就不走投递路径），实际代价是每 2s 一次轻量 journal 扫描 + 用户不可见、不可主动取消，持续 24h。

### 1.F 🟠 P1：Windows 文件事件后缺 `continue`，同一拷贝双广播

`windows/src-tauri/src/clipboard.rs:293-309`：`spawn(send_file_batch_to_peers(...))` 后直接落入文本检查（:312 起）；macOS 对应分支有 `continue`（macos/clipboard.rs:303）。Windows 应用常同时提供 `CF_HDROP` 和 `CF_UNICODETEXT` → 同一内容产生 file batch + 路径文本两条广播，接收端两条记录且互相 supersede，最终剪贴板内容不稳定。

### 1.G ~~macOS 缺少唤醒恢复~~ —— 误报，撤销（v3）

v1/v2 判断"macOS 从未调用 `request_wake_recovery`"**错误**，根因是搜索只覆盖了 `clipboard.rs` 调用点，漏掉 API 路由。实际链路完整：

```
macos/swift-ui/Sources/TailSync/TailSyncApp.swift:209-230
  registerSleepWakeNotifications()
  → NSWorkspace.didWakeNotification
  → 等待 2s（Tailscale 重建隧道）
  → ApiClient.shared.reconnectPeers()
macos/swift-ui/Sources/TailSync/Services/ApiClientStorageSettings.swift:114
  → {"cmd": "reconnect_peers"}
macos/src-tauri/src/api/routes/peers.rs:190-198
  → pool.disconnect_all() + clipboard::request_wake_recovery() + network::clear_peer_cache()
```

README 宣称的"休眠/唤醒后自动恢复"在 macOS 上成立。可保留的小建议：该链路无自动化测试覆盖（Swift 层无法单测唤醒通知），可在守护进程侧为 `reconnect_peers` 命令补集成测试。

### 1.H 其余小项（v3 校准后）

- **进度回跳**（属实，体验）：`completed_bytes` 在 `'restart_batch` 内重置（transfer.rs:508），`FileResume` 重放时进度从 0 重计。应从接收端确认的 offset 重建，只前进不回跳。
- **取消残留**（收窄）：发送循环在批次与 chunk 边界都检查取消并发送 `FileBatchCancel`（transfer.rs:487、510-517、561-569）；残留仅限"删除 journal 后、取消帧送达前"的窄窗口（含对端不可达时取消被跳过的变体）。影响可接受，可选做短期 tombstone。
- **2s journal 扫描**（属实，代价小）：无 pending peer 时仅轻量读盘，远小于全文件哈希。
- **dead code 单文件发送路径**（修正）：`send_file_to_peers`（transfer.rs:640-836）若直接复活，接收端会因 `validate_incoming_file_meta` 的 `batch.is_none()` 检查（prepare.rs:103-105）**直接拒绝**，并非 v1 所述"必然挂起"；v1 提到的 `Ok(_) => {}` 无响应分支（server.rs:541）需要 `batch=Some` 且 `transfer_id=None` 的组合，当前无发送方产生。建议仍可删除该路径。

---

## 二、其他功能面（v3 校准后）

### 2.1 数据库 / 历史存储

**确认属实：**
- 🟠 **配额逐出失败放大**（storage.rs:148 起逐出循环 + lifecycle.rs:299 起）：DB 行先删、外部文件删除失败仅 warn；`bulk_storage_size` 不下降则循环继续删后续未收藏历史，最坏删到无候选项才报错。建议：每轮比较 `used_before/used_after`，未下降立即停止。
- 🟠 **先删重复、后插入，无事务**（entries.rs:5 起、files.rs 同构）：文本/图片场景进程在两步之间退出会丢历史；文件场景载荷文件通常保留但历史行可能消失。建议同事务 + 外部载荷提交后清理。
- 🟠 **后台加密与存储迁移并发**（窄窗口）：主库锁挡不住后台迁移的独立 SQLite 连接，checkpoint+copy（storage.rs:220-244）可能拷出不一致组合。建议迁移期间暂停加密 worker 或统一迁移锁。
- 🟡 **未认证 header 触发大容量预分配**（file_encryption.rs:439-447）：先按 header 申请最高 5 GiB 再验证标签；主要是本地损坏/篡改场景。先认证 header 再分配。

**v3 修正（v1 描述不准确或夸大）：**
- `wal_checkpoint(TRUNCATE)` 是**每次删除操作**执行一次，不是每行一次（一次操作可含整批 ID，lifecycle.rs:402-403）——存在性能成本但非 v1 暗示的 O(n) 逐行。
- `SCHEMA_VERSION` 写死在 v10 分支（migrations.rs:283-291）是**未来升级时的维护陷阱**，不是当前 v10 库的故障。
- `move_source` 有两个分支（file_storage.rs:160-173）：仅"目标不存在→加密新文件→删源失败"分支返回错误并留孤儿；"目标已存在"分支已把删除失败降级为忽略。
- 搜索逐条解密（queries.rs:206-239）属实，但历史上限 500 条，影响有限——应以性能测量决定是否改造，不建议盲改。

### 2.2 配对 / 加密 / 连接层

- 🟠 **配对单槽抢占：属实，范围收窄**（pairing/manager.rs:114 起）：120s 窗口内未认证设备完成 Noise 握手可占住唯一验证槽；连续超时最多拖到约 10 分钟进入 `Locked`。**攻击范围仅限用户主动打开配对窗口期间**。建议：验证会话单独 20-30s 超时、失败按来源限流、本地确认后禁止替换当前槽位。
- **撤回 v1 的"重连后重放漏洞"修复建议**：精确旧 `message_id` 已有去重、过旧时间戳已被拒；能生成新 `message_id` 和有效密文的设备**本来就是已配对的可信方**，有权发送任意剪贴板内容——这不是可修的漏洞，而是信任模型的边界。单纯持久化 sequence 还会与发送端重连后序号重置冲突。不做此修复。
- 🟡 发起端握手无内建读超时（handshake.rs:30、63、84、120）：属实但当前所有生产调用方外层已有 5s/候选级超时，低风险。
- 🟡 **FileChunk 不计入 64MiB 事件预算**：事实成立，但**不是绕过**——把 1 GiB 文件套进 64MiB/min 预算会阻断正常传输。正确做法是单独设计文件吞吐/每日配额，而非扩大事件预算。
- 🟡 Iroh 身份损坏静默轮换（iroh_transport.rs:377-397）：有 LAN/Tailscale 路由时可重新发现修复；仅 Iroh 路由时需重新配对。应给出明确恢复提示，不要静默改变唯一可用路由。
- 🟡 同 hostname 不同 key 静默覆盖（pairing/manager.rs:488-516）：需用户主动确认配对才可达；正确修法是 UI 显示"正在替换已有设备"及新旧指纹，而非协议层拒绝。
- 正面确认（不变）：入站 admission 绑定 hostname+Noise 静态公钥、出站双重 pin、Iroh endpoint 绑定校验无绕过；验证码 HKDF 派生抗 MITM；限流/发现应答有容量与摊销设计。

### 2.3 体验差异（非正确性）

Windows 缺少 macOS 那样的运行时通知缓冲，历史窗口内看不到传输失败原因（只有 OS toast）；两端最终都表现为系统通知。建议 Windows 增加 bounded runtime error queue。

---

## 三、修复方案（v3 采纳分阶段闭环）

### 第一阶段：P0——重做批次完成事务（同时修 §1.A + §1.B）

**不要**先做单独的内存 `completed_batches` 检查（理由见 §1.A）。一次完成以下闭环：

1. 新增**持久化接收回执表**：
   ```sql
   received_file_batches(
     source_device_id, batch_id, manifest_hash,
     status, completed_at,
     UNIQUE(source_device_id, batch_id)
   )
   ```
2. `source_device_id` 用**设备公钥指纹**，不再把可变 hostname 当事务身份。
3. 完成过程改两阶段：
   ```
   prepare_finish → 校验所有文件
     → SQLite 事务写历史 + completion receipt → commit
     → 删除接收 sidecar/临时状态
     → 发送 FileBatchAccept
   ```
4. 重放同一 `(source, batch_id, manifest_hash)` → 直接返回 `AlreadyComplete` 并 ACK；同 batch_id 但 manifest 不同 → 拒绝。
5. 加唯一约束 `UNIQUE(batch_id, batch_index)`；升级前扫描修复存量重复批次（同 index/hash/size 保留一条；内容冲突标记损坏并提示，不自动猜；修好 `batch_total`/状态后再建索引）。
6. `has_complete_file_batch`（db/files.rs:213-226）收紧为：行数 == `batch_total`、索引完整、全部 complete、成员 total 一致。

### 第二阶段：P1——恢复调度器与 Windows 行为

- journal 增加 `attempt_count`、`next_attempt_at`、`last_error`、`last_notified_at`；带抖动退避（如 2s/10s/30s/2min/10min）。
- storage 检查移到 `prepare_file_batch` 全量哈希之前；相同故障仅在状态变化或退避到期时通知。
- journal 保存设备指纹，hostname 仅作展示。
- 增加"挂起传输"UI：重试 / 取消 / 查看原因。
- Windows 文件事件成功后补 `continue`。
- 进度从接收端确认 offset 重建，只前进不回跳。
- 取消操作使用短期持久化 tombstone（保留到取消帧送达或超时）。

### 第三阶段：P1/P2——数据库存储安全

- 外部文件引入同文件系统 staging/`.trash`：移载荷 → SQLite 事务 → commit 后真删 / rollback 恢复。
- 配额逐出每轮 `used_before/used_after` 比较，未下降立即停止。
- 重复项替换改同一事务，载荷提交后清理。
- 存储迁移期间暂停后台加密 worker（或统一迁移锁）。
- 认证 header 后再大容量分配。
- `move_source` 新文件分支的删源失败降级为 warn（目标已校验并持久化成功）。

### 第四阶段：配对与平台体验

- 配对验证会话独立 20-30s 超时；失败按来源限流；确认后锁槽。
- 同 hostname 不同 key 显示替换确认 + 新旧指纹。
- 文件传输独立吞吐/每日配额（不复用 64MiB 事件预算）。
- Windows bounded runtime error queue 进历史窗口。
- Iroh 身份轮换给出明确提示。
- 顺手修：`sync_warning` 测试隔离（全局单槽 → 每测试独立槽或串行化）。

### 实施顺序

**批次持久化回执与 ACK 时序 → 数据库唯一约束与存量修复 → 恢复退避 → Windows 双广播 → 配额逐出 → 配对/通知体验。**

### 验收要求：真实崩溃矩阵（不能只测函数返回值）

- 接收端在最后一个 chunk 后、DB commit 前退出。
- DB commit 后、ACK 前退出。
- ACK 后、发送端 journal 更新前退出。
- 发送端/接收端分别在 10%、50%、99% 进度退出。
- 外置存储中途拔出再恢复。
- peer 改名、解除配对、重新配对。
- 同 batch 完成后重放 20 次。

每场景须同时满足：文件 hash 正确；历史恰好 N 行（非 0/2N）；整体复制可用；journal 最终清除；进度不倒退；相同故障不每 2s 通知；无持续全盘重哈希。

---

## 四、复核更正记录（防误修存档）

| v1/v2 结论 | v3 定论 | 验证依据 |
|---|---|---|
| §1.1 标签错位（P0） | 已随 `4039413` 修复 | worker.rs:208 + 守卫测试 tests.rs:1118-1148 |
| §1.2 重放不幂等（高） | **属实，P0**，已最小复现 | batches.rs:19 无 completed 检查；现有幂等测试未覆盖 Start 重放 |
| 仅查 `completed_batches` 即可修 §1.2 | **撤回**——与 §1.A 组合会固化丢失 | 批次完成事务须重做（§三第一阶段） |
| §1.4 每 2s 全量哈希 | 部分属实：离线在 Start 即失败不哈希 | transfer.rs:499 在 per-file 循环（:509-524）之前 |
| §1.7 macOS 缺唤醒恢复 | **误报，撤销** | Swift didWake → reconnectPeers → disconnect_all+request_wake_recovery+clear_peer_cache（完整链路见 §1.G） |
| "非批次 FileMeta 绕过配额"（v1 已驳） | 维持驳回 | prepare.rs:103-105 直接拒绝 |
| §2.2 重连后重放漏洞 | **撤回修复建议**——信任模型边界，非漏洞 | 新 message_id + 有效密文 = 可信方本身权限 |
| §2.1 wal_checkpoint 逐行 | 修正为逐操作 | lifecycle.rs:402-403 每次调用一次 |
| §1.8 取消残留 | 收窄为窄窗口 | 批次/chunk 边界均发 Cancel 帧 |
| §1.8 dead code 复活即挂起 | 修正为"会被拒绝" | prepare.rs:103-105 拒绝 batch=None |
| —（v3 新增） | **§1.A ACK 先于 DB 提交，P0** | server.rs:436-467 → batches.rs:182/184-193 → sync_adapter.rs:126-137 |
