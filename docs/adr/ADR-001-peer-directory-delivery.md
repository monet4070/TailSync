# ADR-001：Peer Directory & Delivery 深模块

- 状态：实施中（2026-08；全部门禁通过并形成提交后改为"已接受"）
- 范围：`shared/rust-core/src/peer/` 与两端 `network/*`

## 背景

macOS 与 Windows 的 `network/` 层各自实现了一整套 peer 编排：发现合并、候选补全、健康状态机、
可靠投递与连接竞速。两端实现**实质漂移**（Windows 的合并逻辑是超集：别名重关联、LocalInfo 合并、
trusted 全接口补全；健康状态机对"从未响应"的判定不同；类型字段集不同）。漂移检查只能通过
"允许差异"清单容忍这种重复，无法防止它继续扩大。

## 决策

1. **共享逻辑进 core，平台只留接线与适配器。** 迁移顺序：类型 → 发现合并 → 健康状态机 →
   投递记账 → 投递执行 → 竞速策略。每个 PR 纯搬移 + 行为测试先行（两端测试并集移植进 core，
   作为行为规格，Windows 的行为测试在 macOS 本机即可运行）。
2. **core 是深模块：接口小、不变量集中。** `peer/` 的公共面只有类型、规则函数与状态机；
   I/O（socket、mDNS、Tailscale 命令、剪贴板）全部留在平台适配器。
3. **平台包装绑定平台能力，签名对外不变。** 例如 `merge_paired_peers` 的 rtt-capable 闭包、
   `peer_socket_addr` 的端口、`race_connections` 的执行器，都由平台 mod.rs 的薄包装注入，
   外部调用点（api/commands/clipboard）零改动。
4. **行为统一以 README 语义为准，冲突时以测试为证。** 例如"连续两轮失败 = offline"（Windows
   语义，有测试钉住）覆盖"从未响应永远 discovered"（macOS 旧行为，无测试）；会话注册联动
   探测成功（Windows 语义）。
5. **契约面收窄。** 类型与 `server.rs` 移出漂移豁免清单，改为强制逐字节一致；漂移检查新增
   shim 钉扎（平台文件必须 re-export core 类型）。

## 影响

- 合并/健康/投递/竞速规则只有一份实现与一份测试（core 183+ 测试）；平台测试专注真实 I/O 回归。
- 平台 network 层累计净删约 3000 行重复实现（`pool.rs` 减至约 700 行量级，精确行数随时间变化不再记录）。
- JSON 契约：Windows `PeerInfo` 新增 `status`/`current_address`（React 类型本就声明可选字段）；
  Swift `PeerSnapshot` 字段集不变（漂移检查逐字验证）。
- 风险：Windows crate 无法在 macOS 本机编译，依赖镜像修改 + 漂移检查 + CI。

## 未决

- `connection_task`（连接生命周期循环：心跳、keep-frame、队列选择）仍为平台编排，两端允许
  漂移；候选迁入 core 需引入 DeliveryTransport trait 抽象。
- `peer_cache.rs` 探活循环与 `health.rs` 的两种喂入方式（轮式 vs 逐路由）暂保留为平台差异。
