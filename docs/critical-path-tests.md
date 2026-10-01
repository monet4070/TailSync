# 关键路径测试清单

> 用途：把“哪些测试真的护住了关键路径”固定下来，供第 1 阶段的覆盖空白分析、第 8 阶段重构的验收标准使用。
> 权威状态在 `docs/remediation-ledger/`（机器可校验）。本文件只做按路径的汇总视图。
> 更新：2026-10-01，见 `docs/remediation/IMPLEMENTATION-2026-09-30.md`；当前代码本机实际执行通过，原生 Windows/设备另验。
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
| 恢复：生产 fanout/restart 源校验共享 | `production_fanout_and_resume_reuse_source_hashes_and_check_completion` | `shared/platform-clipboard-tests.rs` |
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
| IPC：实际认证 Unix 长轮询断线释放许可 | `authenticated_long_poll_disconnect_returns_connection_permit` | `macos/src-tauri/src/api/transport.rs` |
| 跨平台能力：拒绝非 macOS 平台 | `testLocalCapabilitiesRejectsNonMacOSPlatform` | `macos/swift-ui/Tests/TailSyncTests/ApiClientCancellationTests.swift` |
| Windows 成品不监听旧 TCP API | 打包 smoke 内的 `PackageWindows` 端口断言（integration 门禁） | `windows/scripts/package-windows.ps1` |
| 配对：遗忘使旧会话和迟到ACK失效 | `revocation_invalidates_a_pending_session_before_a_late_ack` | `shared/rust-core/src/pairing/tests_pending.rs` |
| 配对：提交后关闭传输不反转成功 | `committed_pairing_is_terminal_while_transport_close_is_pending` | `shared/rust-core/src/pairing/tests_pending.rs` |
| 配对：pending 同认证 key 恢复 | `pending_recovery_requires_the_same_authenticated_key_and_is_never_active` | `shared/rust-core/src/pairing/tests_pending.rs` |
| 收藏：全部生产清理路径保护文件别名 | `every_history_cleanup_preserves_favorite_payload_case_aliases` | `shared/rust-core/src/db/tests.rs` |
| WAL：默认 busy_timeout 下删除即时返回/维护补偿 | `default_delete_checkpoint_is_non_waiting_and_restores_busy_timeout`、`deferred_delete_checkpoint_is_retried_after_reopen` | `shared/rust-core/src/db/tests.rs` |
| GC：已知 atomic temp 保留活跃 writer/回收退出残留 | `orphan_sweep_collects_owned_temps_and_preserves_live_writers`、`dead_writer_temp_is_collectible_after_a_process_restart` | `shared/rust-core/src/db/tests.rs`、`shared/rust-core/src/private_fs.rs` |
| 投递：真实文件 ACK 阻塞时文本独立交付 | `s2_f5_file_ack_does_not_block_text` | `shared/rust-core/src/peer/delivery/tests.rs` |
| 权限：实际 incoming 入口 | `actual_incoming_batch_creates_private_directories_and_manifest` | `shared/rust-core/src/sync/tests.rs` |
| 前端：cooldown 拒绝展示不确认 warning | History 与 useSyncWarningNotice 完整套件 | `windows/src/pages/History.test.tsx`、`windows/src/hooks/useSyncWarningNotice.test.tsx` |
| 台账自身：测试属性、执行命令与当前源码的实际结果 | `node --test scripts/check-remediation-ledger.test.mjs scripts/remediation-test-results.test.mjs` | `scripts` |

## 剩余边界（2026-10-01）

| 路径 | 现状 |
|---|---|
| 配对单边信任（`S3-P1-2`） | 已有 pending/active、撤销/提交/故障/旧配置夹具及两端恢复UI；真实双设备重启/恢复仍待验 |
| 配额淘汰扫描与锁等待（`S4-P1-3`） | 已有扫描次数、物理账目、WAL 抵扣与并发准入（锁覆盖/串行/记账）测试；实际准入与维护入口观察 receipt/quota/GC 工作线程，坏引用使配额准入在删行前停止 |
| 进度回调锁范围（`S4-P1-4`） | 无每分块进度顺序测试 |
| 通知 gap/实例标识（`S6-P2-1`） | 实际认证 Unix 路由与同一锁 helper 的 entries/earliest/dropped 确定性 overflow gate；Swift gap 提示走现有通知设置，原生 slow-client/restart 体验待验 |
| Windows 后台警告不被吞（`S6-P2-4`） | Core peek/ack 与实际 History cooldown/隐藏/ack重试 gate 通过；Windows 原生窗口待验 |
| 文件/即时队列隔离（`S2-F5`） | 独立认证 worker 已实现，ACK 阻塞 gate/九轮生产 Core 基线通过；原生 Windows 与设备性能待验 |
| Windows 发现（`S1-P1-5`） | 本条仅 Windows：历史原生 CI 已通过；当前原生 CI 及断网/VPN 场景待验。macOS 等价工厂 gate 单独待补 |
| 孤儿文件对账 GC（`S5-P2-1`） | 现有周期维护已接线，补全部 unlink 文件身份及已知 writer temp gate；无 PID 的旧名仍保守保留 |

`.github/workflows/coverage.yml` 每日/手动采集 shared core 与 runtime 的 LCOV、JSON 汇总和源码 SHA，保存 30 天，不进入 PR 必需门禁；平台 UI、Windows 分支和真机路径尚不包含在这份覆盖率内。工具固定为 [cargo-llvm-cov 0.6.21](https://docs.rs/crate/cargo-llvm-cov/0.6.21)，插桩测试串行执行，`report` 只转换同一轮结果，不再执行测试。工作流上线后的首份托管产物仍待验证，本清单不设全仓库百分比门槛。

耗时采样仅统计 `ci.yml` 的已完成运行，避免把采样工作流自身或发布流水线混入均值。台账执行记录绑定提交和源码摘要，不能复用另一版本的通过结果；忽略、缺失、仅编译或失败均不能通过台账门禁。
