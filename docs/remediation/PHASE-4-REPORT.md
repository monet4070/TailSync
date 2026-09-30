# 阶段 4 报告：配对单边信任（S3-P1-2）

> 基线：`origin/main` = `b61a0a04b88f5828707f7ef64781389944471be8`。
> 工作分支：`codex/remediation-2026-09-29`。日期：2026-09-30。
> 环境：单机 macOS（Apple Silicon）。真实双设备验收**未做**，见文末。

## 1. 发现的问题（先复现，再动手）

审计措辞：信任在双方确认前落盘；超时后只把窗口退回 `Waiting`，不回收已落盘的单边信任。

按代码核对后的准确描述是：会话在**双方确认后立即**写 `trusted_peer_keys`，然后才发 `PairingPersisted` 并等待对端确认。因此存在两个失败窗口：

1. 本端已落盘、对端从未落盘，链路断开 → 本端持有对端的**单边 active 信任**。
2. 本端已落盘、对端已落盘但确认丢失，会话超时 → 同样留下单边 active 信任，且没有任何记录表明这条记录是半成品，因此也无法事后对账。

`trusted_peer_keys` 是**唯一**的信任事实源：`peer_is_allowed`（`shared/rust-core/src/peer/admission.rs:18`，被入站准入与逐帧再授权调用）、连接池握手（`shared/platform-network-pool.rs:546/618`）、peer 目录（`shared/rust-core/src/peer/directory.rs:486/543`）以及两端 UI 的 `Peer.trusted` 全部读它。所以"单边记录被当作 active"的后果是被审计点名的：本端会接受一个从未接受本端的设备。

修复前没有任何测试覆盖"落盘后对端未确认"这条路径（`pairing_waits_for_remote_persisted_ack_before_marking_paired` 只验证了"未收到 ack 前 phase 不是 Paired"，从不切断链路、也从不永久扣留 ack）。本阶段先补上这个复现，再改实现。

## 2. 设计

**权威与非权威分离。** 新增非权威的 pending 注记，存放于 `pairing-pending.json`（与 `config-v2.json` 同目录的 sidecar）：

- 双方确认后，本端只写注记并发 `PairingPersisted`；
- **只有**在同一已认证会话上收到对端的 `PairingPersisted` 之后才 `promote_pairing` —— 写 `trusted_peer_keys` 并清除注记；
- 超时**不删除**注记（对端可能已落盘而确认丢失）。

由此得到不变量：**任何失败路径都不会留下单边 active 信任**。本端推进到 active 的前提是收到对端"我已落盘"的证明；而对端只有在注记已 durable 之后才会发出该帧。顺序本身由测试钉住（`the_older_peer_concludes_success_only_after_the_new_note_is_durable`）。

**为什么是 sidecar 而不是新增 `Settings` 字段。** `Settings` 带 `deny_unknown_fields`（`shared/rust-core/src/crypto.rs:9`），新增顶层键会让旧版构建**整份配置加载失败**（不是拒绝一个字段，而是丢掉全部设置）。sidecar 对旧版完全不可见，旧版行为恰好落在安全方向：`trusted_peer_keys` 里没有这个键，就视为未配对。测试断言 `Settings` 的序列化结果里不含任何 `pending*` 键。

**协议不新增 `Command`。** 新增命令会让旧端 `Command::from_u16` 返回 `None` → `UnknownCommand` → 直接失败。新旧端只使用既有的 `PairingConfirm` / `PairingPersisted`，旧端策略一字未改。旧端"显示已配对"只可能发生在收到本端持久化帧之后，而本端只在注记落盘后才发该帧，因此旧端的成功总是对应一个本端可以继续处理的记录。

**"不把 pending 表示为已配对"无需任何 UI 改动**：pending 不进 `trusted_peer_keys`，两端 UI 读到的就是未配对。

**未实现自动对账（明确记录）。** 重启后不会因为"对端持有该密钥"就自动 promote：持有密钥不能证明对端曾落盘，那样做会重新造出本项要消灭的单边信任。完成路径是再次经过携带对端落盘证明的配对会话（UI 对未配对设备本就提供该入口）。这一点在台账的 `fix_evidence` 中同样写明。

## 3. 改动清单

| 文件 | 内容 |
|---|---|
| `shared/rust-core/src/pairing/pending.rs`（新增） | `PendingTrustStore` / `PendingTrustRecord`：sidecar 持久化、原子私有写入、缺失或损坏时按空处理（只可能要求重新配对，不会把损坏字节变成信任）、`format_version` 不匹配时拒绝加载 |
| `shared/rust-core/src/pairing/manager.rs` | `persist_pairing` 拆为 `record_pending_pairing`（写注记）+ `promote_pairing`（写信任并清注记）；`promote_pairing` 只在 `local_persisted && remote_persisted` 分支调用；新增 `pending_trust()`；`with_store`/`with_pending_store_at` 便于重启测试 |
| `shared/rust-core/src/pairing.rs` | `mod pending;` 与重导出；`PairingManager` 增加 `pending` 字段（`persist_trust=false` 时纯内存，测试不写真实数据目录） |
| `shared/rust-core/src/pairing/tests_pending.rs`（新增） | 9 个测试，见下 |
| `shared/rust-core/tests/fixtures/legacy-config-v2.json`（新增） | 冻结的旧版 `config-v2.json`（含一台已配对设备） |
| `docs/remediation-ledger/entries/S3-P1-2.json`、`manifest.json`、`INDEX.md` | 状态 `unfixed` → `fixed_gated`，登记门禁与 `ci_job` |

## 4. 覆盖与变异验证

9 个测试（`shared/rust-core/src/pairing/tests_pending.rs`）：

| 测试 | 覆盖的边界 |
|---|---|
| `a_link_that_dies_before_the_peer_acknowledges_leaves_a_pending_note_not_trust` | **被审计向量**：本端落盘并发出持久化帧后链路断开；断言 phase 非 Paired、`trusted_peer_keys` 无该设备、注记存在（台账门禁） |
| `a_session_timeout_after_confirmation_keeps_the_pending_note` | 双方确认后会话超时：不留下信任，注记保留 |
| `repeated_confirmation_is_idempotent_and_grants_nothing` | 重复确认只产生一条注记、不发第二帧、不授予信任 |
| `a_completed_pairing_promotes_to_trust_and_clears_the_note` | 快乐路径：ack 到达前是"有注记、无信任"，ack 到达后恰好一条 active 记录且注记清除 |
| `a_restart_keeps_the_note_and_still_reports_the_device_unpaired` | 进程重启：注记仍在、设备仍为未配对 |
| `the_note_never_leaks_into_the_settings_file_an_older_build_reads` | `Settings` 序列化中不含 `pending*` 键；旧版设置形状仍可加载 |
| `a_frozen_legacy_configuration_still_loads_with_its_trust_intact` | 冻结旧版配置加载后既有信任不变；sidecar 缺失时读作空 |
| `an_older_peer_pairing_into_the_new_side_still_completes` | 旧端→新版：旧端序列（确认后立即落盘）仍能完成 |
| `the_older_peer_concludes_success_only_after_the_new_note_is_durable` | 互通安全性的顺序依据：旧端收到持久化帧时，本端注记已落盘 |

**变异验证（每次单独运行并恢复）**：

| 变异 | 结果 |
|---|---|
| `record_pending_pairing` 改回"本地确认即写 trust"（修复前行为） | 9 个测试中 **5 个失败** |
| 去掉注记写入 | 9 个测试中 **6 个失败** |

## 5. 本地校验

- `cargo fmt --manifest-path shared/rust-core/Cargo.toml --all -- --check` 通过；macOS、Windows crate 的 fmt 同样通过。
- `cargo clippy --locked --manifest-path shared/rust-core/Cargo.toml --all-targets -- -D warnings` 干净。
- `node scripts/run-remediation-tests.mjs --job rust-macos -- cargo test --locked --manifest-path shared/rust-core/Cargo.toml` 通过，`S3-P1-2` 已计入实际执行的门禁。
- macOS crate `cargo test --lib`：109 passed / 1 ignored；Windows crate `cargo check --all-targets` 通过。
- 台账：31 条、`fixed_gated 28 / partial 1 / accepted 1 / unfixed 1`。

## 6. 未完成与适用边界

- **真实双设备未验**：只有 macOS 单机可用，两台真机之间的中断/重启走查未做。上表全部为本地确定性测试。
- **两端 UI 走查未做**：pending 不显示为已配对由数据面保证，但 macOS SwiftUI 与 Windows React 在真实 pending 状态下的界面走查未做。
- **`pending_trust()` 尚无 UI 出口**：目前只有 Rust API 与测试读取它；"这设备配对未完成，是否重试"的界面提示属阶段 8 的 UI/runtime-IPC 深化。做成 UI 契约字段需要走 `contracts.rs` + 生成器流程（见 S6-P2-1 的先例：新增字段要求 app 与 daemon 同时升级），不在本阶段内。
- **未实现自动对账**：理由见 §2，完成路径是再次经过配对会话。
