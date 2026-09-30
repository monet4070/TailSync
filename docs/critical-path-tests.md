# 关键路径测试清单

> 用途：把“哪些测试真的护住了关键路径”固定下来，供第 1 阶段的覆盖空白分析、第 8 阶段重构的验收标准使用。
> 权威状态在 `docs/remediation-ledger/`（机器可校验）。本文件只做按路径的汇总视图。
> 基线：`origin/main` = `b61a0a04b88f5828707f7ef64781389944471be8`。

测试命令（与 CI 步骤一致）：

| 范围 | 命令 | CI job |
|---|---|---|
| shared core | `cargo test --locked --manifest-path shared/rust-core/Cargo.toml` | rust-macos, rust-windows |
| shared runtime | `cargo test --locked --manifest-path shared/tailsync-runtime/Cargo.toml` | rust-macos, rust-windows |
| macOS 应用 | `cargo test --locked --manifest-path macos/src-tauri/Cargo.toml --all-targets` | rust-macos |
| Windows 应用 | `cargo test --locked --manifest-path windows/src-tauri/Cargo.toml --lib` | rust-windows |
| Swift UI | `swift test --package-path macos/swift-ui` | rust-macos |
| 仓库脚本 | `node --test scripts/*.test.mjs` | scripts |

## 路径 → 具名测试

| 关键路径 | 具名测试 | 位置 |
|---|---|---|
| 握手：帧边界在窗口中继下不被破坏 | `window_signal_preserves_a_partially_read_frame` | `shared/rust-core/src/secure/handshake.rs` |
| 配对：匿名异常不消耗失败预算 | `anonymous_protocol_anomalies_do_not_consume_the_lockout_budget` | `shared/rust-core/src/pairing/tests.rs` |
| 配对：未确认槽位可抢占 | `newer_session_preempts_an_unconfirmed_slot_but_not_a_confirmed_one` | `shared/rust-core/src/pairing/tests.rs` |
| 信任锚：同名异钥被拒 | `re_pairing_same_hostname_with_a_different_key_is_rejected` | `shared/rust-core/src/crypto/tests.rs` |
| 发现：lan_only 拒绝 Tailscale | `lan_only_rejects_tailscale_but_keeps_other_ula_and_link_local` | `shared/rust-core/src/peer/directory.rs` |
| 发现：零设备是成功、仅全传输失败才报错 | `lan_discovery_succeeds_with_zero_devices_and_fails_only_when_all_transports_fail` | `shared/rust-core/src/peer/directory.rs` |
| 候选解析：跳过无作用域链路本地 IPv6 | `resolve_candidates_skips_link_local_ipv6_without_a_scope` | `shared/rust-core/src/peer/directory.rs` |
| Windows 广播目标语义 | `broadcast_targets_always_include_the_global_broadcast` | `windows/src-tauri/src/network/lan.rs` |
| Windows 发现：接口过滤、零设备与发送失败 | `lan_discovery_filters_interfaces_and_distinguishes_send_failure_from_zero_peers` | `windows/src-tauri/src/network/lan.rs` |
| 发现：本机 LAN IP 选择 | `local_lan_ip_prefers_private_ipv4_and_never_returns_unspecified` | `shared/rust-core/src/peer/directory.rs` |
| 候选排序：私网优先 | `candidate_sort_prefers_private_addresses_over_apipa_and_loopback` | `shared/rust-core/src/peer/directory.rs` |
| 投递：静默 ACK 不重放 | `silent_event_ack_ends_the_attempt_instead_of_replaying_on_the_stream` | `shared/rust-core/src/peer/delivery/tests.rs` |
| 投递：竞速快速失败释放备选 | `race_releases_a_delayed_fallback_when_the_preferred_route_fails_fast` | `shared/rust-core/src/peer/delivery/tests.rs` |
| 连接池：路由变化保留 worker | `sender_for_candidates_keeps_a_live_worker_across_route_changes` | `shared/rust-core/src/peer/pool.rs` |
| 恢复：源校验跨对端共享 | `source_validation_is_shared_for_one_two_and_eight_peers` | `shared/platform-clipboard-tests.rs` |
| 恢复：续传字节不重复计账 | `pending_file_batch_bytes_exclude_partial_data_already_on_disk` | `shared/rust-core/src/sync/tests.rs` |
| 恢复：孤儿清理保留近期文件 | `expired_transfer_cleanup_removes_orphans_but_preserves_recent_and_nested_files` | `shared/rust-core/src/sync/tests.rs` |
| 接收：并发限流计入 pending/inflight | `pending_and_inflight_receives_count_toward_the_peer_limit` | `shared/rust-core/src/sync/tests.rs` |
| 数据库事务：配额预检只淘汰外部载荷 | `batch_preflight_stops_when_only_inline_history_can_be_evicted` | `shared/rust-core/src/db/storage.rs` |
| 数据库事务：删除截断 WAL（正常路径） | `explicit_delete_truncates_the_write_ahead_log` | `shared/rust-core/src/db/tests.rs` |
| v9：忙时保留迁移状态、释放读者后重开恢复 | `v9_busy_migration_keeps_pending_state_and_completes_on_reopen` | `shared/rust-core/src/db/tests.rs` |
| GC：保留被大小写别名引用的载荷 | `orphan_sweep_preserves_reused_payloads_with_case_aliases` | `shared/rust-core/src/db/tests.rs` |
| GC：无法解析存活引用时先中止清扫 | `orphan_sweep_aborts_before_deletion_when_a_live_reference_is_invalid` | `shared/rust-core/src/db/tests.rs` |
| 配额：每次准入只扫描一次、按实际回收记账 | `quota_eviction_measures_the_storage_tree_once_per_reserve`、`quota_eviction_credits_the_wal_the_delete_reclaims` | `shared/rust-core/src/db/storage.rs` |
| 预览安全：非图片先拒后解密 | `image_payload_rejects_non_image_entries_before_decrypting` | `shared/tailsync-runtime/src/history.rs` |
| 设置合并：跨窗口补丁（PR #68） | 见 PR #68 的设置回归测试 | `shared/rust-core/src/crypto/tests.rs` 等 |
| 投递退避：失败后按 reconnect_delay 退避 | `delivery_failure_backoff_delays_the_next_reconnect` | `shared/rust-core/src/peer/delivery/tests.rs` |
| IPC：断开取消长轮询 | `disconnected_client_drops_pending_response` | `macos/src-tauri/src/api/transport.rs` |
| 跨平台能力：拒绝非 macOS 平台 | `testLocalCapabilitiesRejectsNonMacOSPlatform` | `macos/swift-ui/Tests/TailSyncTests/ApiClientCancellationTests.swift` |
| Windows 成品不监听旧 TCP API | 打包 smoke 内的 `PackageWindows` 端口断言（integration 门禁） | `windows/scripts/package-windows.ps1` |
| 台账自身：测试属性、执行命令与当前源码的实际结果 | `node --test scripts/check-remediation-ledger.test.mjs scripts/remediation-test-results.test.mjs` | `scripts` |

## 已知空白（第 1 阶段的结论，待后续阶段填补）

| 路径 | 现状 |
|---|---|
| 配对单边信任（`S3-P1-2`） | 无 pending/active 与互通夹具 |
| 配额淘汰扫描与锁等待（`S4-P1-3`） | 已有扫描次数和物理账目测试；缺并发双批次准入与锁等待测试 |
| 进度回调锁范围（`S4-P1-4`） | 无每分块进度顺序测试 |
| 通知 gap/实例标识（`S6-P2-1`） | 仅覆盖环形缓冲有界与游标，未覆盖溢出提示 |
| Windows 后台警告不被吞（`S6-P2-4`） | 无测试 |
| 文件/即时队列隔离（`S2-F5`） | 未实现，无测试 |
| Windows 发现（`S1-P1-5`） | 新门禁在 macOS 主机通过；原生 Windows CI 与断网/VPN 场景尚待验 |
| 孤儿文件对账 GC（`S5-P2-1`） | 已有对账/宽限期/别名/失败保护测试；尚未周期接线 |

`.github/workflows/coverage.yml` 每日/手动采集 shared core 与 runtime 的 LCOV、JSON 汇总和源码 SHA，保存 30 天，不进入 PR 必需门禁；平台 UI、Windows 分支和真机路径尚不包含在这份覆盖率内。工具固定为 [cargo-llvm-cov 0.6.21](https://docs.rs/crate/cargo-llvm-cov/0.6.21)，插桩测试串行执行，`report` 只转换同一轮结果，不再执行测试。工作流上线后的首份托管产物仍待验证，本清单不设全仓库百分比门槛。

耗时采样仅统计 `ci.yml` 的已完成运行，避免把采样工作流自身或发布流水线混入均值。台账执行记录绑定提交和源码摘要，不能复用另一版本的通过结果；忽略、缺失、仅编译或失败均不能通过台账门禁。
