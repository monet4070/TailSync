# 变更说明：回环 peer 加固 + 测试数据目录隔离（2026-09-28）

本文件记录两个独立缺陷的修复及其验证证据。改动只涉及 `shared/rust-core`，不改变线协议（v4）与数据库 schema（v11）。

- 分支：`fix/loopback-peers-and-test-isolation`
- 提交：`7136fab` — `fix(core): refuse loopback peers and isolate test data directory`
- 规模：5 个文件，+264 / −18

---

## 一、缺陷概览

### 缺陷 1：受信 peer 被固定在回环地址，daemon 无限连接自己

**现象**：daemon 每 5 秒尝试连接 `127.0.0.1:19890`（它自己的监听端口），安全握手每次都以身份不符失败，永远无法成功，日志被持续刷屏。设备列表里出现一个永远连不上的幻影设备。

**根因**：
- 自配对保护只比对公钥：`shared/rust-core/src/identity.rs:397` 判定的是 `public_key == identity.public_key_base64()`，**不检查地址**。因此「主机名 + 另一个公钥 + 地址 127.0.0.1」会被正常接受。
- 回环被当作合法 LAN 地址：`shared/rust-core/src/peer/directory.rs:72` 的 `source_matches_mode` 接受 `is_loopback()`；排序里回环只是优先级最低（`directory.rs:130`）。
- 配对时保存的是会话观测到的对端地址（`shared/rust-core/src/pairing/manager.rs` 的 `persist_pairing`）。**同一台机器上两个实例互相配对时，观测到的地址就是 `127.0.0.1`**，于是被写入 `trusted_peer_addresses`。
- 出站解析不为「目标即本机」做任何过滤（`resolve_candidates`）。

**结论**：这是本地同机配对/测试的产物，但出厂代码没有设防——任何做过同机双实例配对的人都会得到同样的死循环。

### 缺陷 2：单元测试写入真实数据目录，覆盖用户配置

**现象**：执行 `cargo test` 后，用户的真实 `config-v2.json` 和 `iroh-identity-v1.bin` 被测试内存中的夹具覆盖。已配对设备在磁盘上丢失（运行中的进程内存里还在，重启后失效）。

**根因**：
- `shared/rust-core/src/db/paths.rs` 的 `get_data_dir()` 在 `TAILSYNC_DATA_DIR` 未设置时回退到 `directories::ProjectDirs` 的真实 Application Support 目录；`cargo test` 默认不设该变量。
- 设置路由会落盘：`macos/src-tauri/src/api/routes/settings.rs:114/166/217` 调用 `settings.save()` → `settings_path()` → `get_data_dir()`。
- keystore 对 DEK 已有测试进程防护（`is_cargo_test_harness_executable`），但设置持久化没有对应保护。

**触发链**：`api::routes::settings::tests` 里构造 `connection_mode: "iroh_only"` + 夹具 peer 的 Settings，调用 `set_connection_mode` 路由 → `settings.save()` → 写真实目录。

> 注：`private_fs::create_private_dir_all` 会**拒绝符号链接路径**，所以手动把 `TAILSYNC_DATA_DIR` 指向 `/tmp/...` 隔离是无效的（`/tmp` 是符号链接），会得到 ENOENT。`std::env::temp_dir()`（`$TMPDIR`，位于 `/var/folders`）可用，因为遍历在遇到已存在的目录时即停止。

---

## 二、改动内容

### A. 回环 endpoint 的拒绝与丢弃（缺陷 1，三层）

| 位置 | 改动 |
|---|---|
| `shared/rust-core/src/peer/directory.rs:117` | 新增 `pub fn is_self_referential_address(ip)`（判定 `127.0.0.0/8`、`::1`） |
| `shared/rust-core/src/peer/directory.rs:217` | `parse_pairing_target` 拒绝回环的配对地址（与既有「拒绝 IPv6 link-local」同一风格） |
| `shared/rust-core/src/peer/directory.rs:658` | `resolve_candidates` 丢弃回环候选，daemon 绝不把回环地址变成连接目标 |
| `shared/rust-core/src/crypto.rs:591` | 新增私有助手 `reject_self_referential_endpoint` |
| `shared/rust-core/src/crypto.rs:307` | `trust_peer_without_save` 在**任何状态变更之前**校验：被拒时不会插入 key、不会置 enabled、不会写地址（原子拒绝） |
| `shared/rust-core/src/crypto.rs:378` | `remember_peer_address_without_save`（发现/健康回写路径）拒绝回环 |

分层意图：**源头不允许新建**（pairing/trust/remember），**出站兜底**（resolve_candidates），因此已存在的脏配置也不会再连自己。

### B. 测试数据目录隔离（缺陷 2）

| 位置 | 改动 |
|---|---|
| `shared/rust-core/src/db/paths.rs:19,46` | `get_data_dir()` 接入 `test_harness_data_dir()`：Cargo 测试进程重定向到每进程独立的 `$TMPDIR/tailsync-test-data-<pid>`，先用普通 `create_dir_all` 建目录以绕开符号链接检查 |
| `shared/rust-core/src/db/paths.rs:61` | `is_cargo_test_harness()`：可执行文件名形如 `tailsync*-<≥16 位十六进制>` 且父目录为 `deps` 才判定为测试进程（出厂/`cargo run` 二进制不满足，仍走真实目录） |
| `shared/rust-core/src/db/paths.rs:92,99` | `is_real_data_dir` / `pub fn is_unisolated_test_write`：兜底判定 |
| `shared/rust-core/src/crypto.rs:192` | `Settings::save()` 命中 `is_unisolated_test_write` 时**拒绝写入并报错**，绝不静默覆盖真实配置 |
| `shared/rust-core/src/db.rs:37` | 重新导出 `is_unisolated_test_write` |

隔离放在**数据目录解析器**里而非每个测试里，因此 settings、Iroh 身份、history 库等所有消费者一次性被隔离，无需改测试。

---

## 三、测试与验证

### 新增测试（7 个）

回环加固：
- `peer::directory::tests::pairing_target_rejects_loopback_addresses`
- `peer::directory::tests::resolve_candidates_drops_self_referential_routes`
- `peer::directory::tests::resolve_candidates_yields_nothing_when_only_loopback_is_known`
- `crypto::tests::trust_peer_rejects_a_loopback_endpoint_without_mutating_settings`
- `crypto::tests::remember_peer_address_rejects_a_loopback_route`

隔离：
- `db::paths::tests::cargo_test_binaries_never_use_the_real_data_directory`
- `crypto::tests::settings_save_stays_inside_the_isolated_test_data_directory`

### 结果

| 套件 | 结果 |
|---|---|
| `cargo test -p tailsync-core --lib` | **449 passed / 0 failed / 1 ignored**（原 447，+2 新测试） |
| `cargo test --lib`（`macos/src-tauri`，即缺陷 2 的触发套件） | **100 passed / 0 failed / 1 ignored** |

实时生效的判定标准：跑测试前后对真实 `config-v2.json` + `iroh-identity-v1.bin` 取 SHA-256 比对，**两个套件跑完均为未变**；测试产生的 `config-v2.json` / `iroh-identity-v1.bin` / `outgoing-transfers/` 全部落在 `/var/folders/.../tailsync-test-data-<pid>/`。

---

## 四、影响与兼容性

- **行为变化**：不能再把受信 peer 固定到 `127.0.0.1` / `::1`（配对、trust、remember 均会拒绝并给出明确错误）。同机两实例通过回环直连不再可用；如需同机联调，请使用本机真实 LAN 地址或依赖发现。
- **不改变**：入站来源分类 `source_matches_mode` 仍接受回环（与 `RELEASE-NOTES-v2.3.1.md` 中「回环保持原语义」一致）；线协议 v4、DB schema v11 未动；已配对设备无需重新配对。
- **对出厂二进制无影响**：`is_cargo_test_harness()` 的判定条件（`deps` 目录 + `tailsync*` 前缀 + 十六进制哈希）不会被打包后的 `tailsync` / `tailsyncd` 命中。
- **平台**：Windows 侧同样 `include!` 了这些共享代码，会自动受益；其测试无法在 macOS 上运行，需由 CI 覆盖。

---

## 五、事故说明（透明记录）

在实现上述加固的过程中，我为验证而执行了 `macos/src-tauri` 的测试套件，触发了**缺陷 2 本身**：

- 12:10:21 真实 `config-v2.json` 被写入测试夹具（`connection_mode: "iroh_only"`、peer `iroh-admission-client`），`iroh-identity-v1.bin` 同时被覆盖且**未生成 `corrupt-` 备份**；
- 后果：本机与 `DESKTOP-V62KRAF` 的配对在磁盘上丢失（进程内存中仍在）。
- 处置：已由重新配对恢复（当前配置为 `DESKTOP-V62KRAF`，新的公钥与 iroh endpoint `5c0d96bf…`）。

该事故正是本次「B. 测试数据目录隔离」的动机；修复后已用指纹比对确认测试不再触碰真实目录。

---

## 六、遗留与后续

1. 本变更**仅在源码**：本机已安装的 2.3.1 安装包不包含，需要重新构建安装才生效。
2. 是否在 `RELEASE-NOTES` 中为下一版加入一条用户可见说明（例如「修复同机配对产生的自连循环」），待定。
3. 可选评估：入站方向的回环来源是否需要一并收紧（当前保持放行以兼容既有的同机调试与上线语义）。
4. 可选评估：为 `Settings::save()` 之外的其它真实目录写入点（history 库、身份材料）补充同类断言测试。
