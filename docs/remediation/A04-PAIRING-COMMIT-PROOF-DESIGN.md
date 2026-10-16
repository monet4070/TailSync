# A04 — 能力门控的配对提交证明与认证对账

状态：已实现；本机隔离验收通过；改动未提交。审查基线：TailSync-remediation，HEAD c6c1420 加用户既有 A01/A02/A03/A05 修复。

本文件描述最终实现，替代初稿中“收到 ACK 后删除全部收据”和“下一次任意认证连接保证收敛”等简化表述。用户默认决策保持：两个命令、一个能力位；只挂配对会话；16 字节事务 nonce；不增加 DTO 或准入事实源。

## 1. 设计思路

把“我认为配对完成”拆成三个事实：用户曾同意这一笔配对、本机已保存信任、对端也已保存并证明同一笔提交。协议允许短暂的一端 active、一端 pending；保存可重放的收据，让下次认证配对会话补齐。收据不授予业务权限，Settings.trusted_peer_keys 仍是信任来源。

提交证明是 Noise 认证后的对端断言。本端诚实实现只在 Settings 保存成功后发送；它不是远程磁盘取证，无法证明恶意对端真的写盘。

收敛有条件：双方保持原身份和匹配收据、存储恢复可写、能力开启，并再次打开配对窗口、建立同钥认证会话且完成交换。普通业务连接、启动、永久断网、丢失收据或降级运行不包含在保证里。

## 2. 两条路径必须区分

### 首次配对：启动提交

双方用户确认验证码 → 各自写 pending nonce → 交换 PairingPersisted(nonce) → 本端同时拥有双方确认与双方持久准备证据 → 保存 active trust → 更新 committed_awaiting_ack → 发送 PairingCommitted(nonce)。

这是首次提交的必要启动条件。不能要求首次本地提交也先等对端 Committed，否则双方互等。初稿“pending 只能由 Committed 晋升”的安全约束适用于恢复路径；首次提交仍要求双方用户确认和持久准备，不能凭超时自行完成。

### 恢复配对：认证对账

双方重新打开配对窗口、重新 Noise 认证 → 核对 hostname、静态公钥与原 nonce。已 active 一方还须确认 Settings 中同钥且未禁用，才可重放旧 Committed。尚未 active 一方只能由匹配其持久 consent 的认证 Committed 晋升。

恢复不需要重新点击原验证码确认。没有匹配 consent、错误密钥/nonce、已禁用或已成功 forget 的设备不推进。新连接的 session ID 不能覆盖原 nonce。

两条路径最终都要求 local_committed && remote_committed && acknowledged，才进入 PairingPhase::Paired。ACK 单独不会授信或完成配对。

## 3. 持久状态

```text
Pending（用户同意与原 nonce，未必 active）
   │ 首次双端准备，或恢复时收到认证提交证明
   ▼
CommittedAwaitingAck（Settings 已 active，交换仍未完成）
   │ 双方提交证明 + 本端证明的 ACK
   ▼
Completed（隐藏于 pending UI，保留用于答复重放）
   │ 成功忘记设备，或新一轮明确确认替换
   ▼
删除/替换
```

每 hostname 保留一份记录，新增 phase 与 commit_nonce，仍用 pairing-pending.json，format_version=1、serde 默认值兼容旧记录。completed 不能立即删除：否则最后一个 ACK 丢失时，已经完成的一方无从证明原事务，另一方无法恢复。

落盘顺序：保存 pending nonce → 再保存 Settings active → 更新 awaiting phase。Settings 保存成功、phase 保存失败时，旧 Pending 仍在，后续检查真实 Settings 再重放。写入失败不误报 Paired；损坏/未知格式原文件不覆盖。原记录与新记录均不参与 admission。

## 4. 要改哪里

| 模块/文件 | 改动 | 保持的边界 |
|---|---|---|
| shared/tailsync-protocol/src/lib.rs、tests.rs | Committed=0x0011、Ack=0x0012；codec、命令映射与 payload 限制；flags.bits | wire 4/5 均可用，不改既有编号 |
| shared/rust-core/src/secure.rs | pairing_commit_proof 能力与开关；发送新命令前检查协商 | 双端 AND；能力关闭时禁止写新命令 |
| shared/rust-core/src/secure/handshake.rs | 实际 pairing connector/acceptor 广播能力 | 原生产 connect_pairing 并非自动广播，已接到真实入口 |
| shared/rust-core/src/pairing/manager.rs | 协商分支、隐藏 Completed、记录 nonce、forget 生命周期 | 原 legacy run_session 顺序保留，锁序 state → settings → pending |
| shared/rust-core/src/pairing/commit.rs | 首次提交、恢复、帧校验、证明/ACK、终态 | 只在 PairingManager 配对会话中运行 |
| shared/rust-core/src/pairing/pending.rs | phase、nonce、不可用存储重载 | 不增加 Settings 字段或数据库 schema |
| pairing/tests_commit.rs、tests_pending.rs、secure/tests.rs | 正常、失败、重启、认证、兼容回归 | 不写真实用户配置或剪贴板 |
| windows/src/pages/settings/SettingsConnectionsSection.tsx、macos/swift-ui/Sources/TailSync/Views/ConnectionsView.swift | 已有 pending 对应的设备徽标显示待完成 | 不改 DTO、trusted 字段或同步准入 |
| secure/peer/platform 既有测试 fixture | 新能力字段显式 false | 冻结旧行为，不改平台生产网络逻辑 |

peer/admission.rs 未改；普通连接生命周期未挂对账；数据库、投递、剪贴板 freshness 不在本次实现范围。

## 5. 最容易踩坑的地方

1. **把首次提交与恢复混成同一规则会死锁。** 首次双方持久准备启动提交；恢复必须拿到匹配旧 consent 的认证证明。
2. **只改 CapabilitySet 不够。** 真实 pairing 握手入口以前把能力全部禁用；必须同时修改连接发起/接受入口。新命令发送还在 SecureConnection 加保护，旧端不收到未知编号。
3. **最后 ACK 丢失。** Completed 收据必须保留，否则双方信任虽可能已保存，一端 UI/恢复仍无法闭合。重复证明只重发 ACK，不无限互相发送证明。
4. **跨会话 nonce 不等于身份。** 原 nonce 来自首次 Noise session ID 的带域 BLAKE3 派生；16 字节仅标识事务。重新认证和静态密钥匹配才提供身份边界。
5. **取消、超时与撤销不是同一件事。** 提交后 cancel/timeout 保留 active 和收据，不回滚；forget 必须使恢复 consent 失效。
6. **forget 不能误报成功。** proof-capable 收据清理失败，或记录不可读而无法排除 proof consent 时，持久化 existing enabled=false 并返回错误：设备禁用、forget 未完成，存储恢复后重试。成功删除 consent 才删除 key 并报成功。已知 legacy note 的清理失败仍遵循原 key 撤销成功、清理告警路径。Settings 写失败也返回错误。这个 fail-closed 行为是新增可授信恢复记录的必要配套。
7. **不可读记录要保留。** 不以空记录覆盖损坏或未来格式；新会话/forget 可重试加载恢复后的存储。
8. **UI trusted 不等于本轮完成。** 原设备徽标只看 trusted，会过早显示已配对；现在用现有 pending summary 显示等待对端完成，成功回调仍只依赖 phase=paired。设备与配对状态是独立异步快照；本次未增加原子快照契约，尚未实测各平台每一帧的显示时序。
9. **kill switch 不是会话中途热切换。** 设置 TAILSYNC_DISABLE_PAIRING_COMMIT_PROOF（变量存在即禁用）并重启进程，新握手走旧路径；已建立会话的能力不变。禁用不撤销已保存的 trust，旧 pending 新协议对账暂停。

## 6. 兼容与回滚

| 双方能力 | 路径 |
|---|---|
| true / true | Committed + Ack；Persisted/Committed/Ack payload 都是 nonce16 |
| true / false、false / true、false / false | 原 Confirm → 空 payload Persisted → promotion 路径；不发新编号 |
| 旧端无能力字段 | serde(default) 视为 false；未知握手 JSON 字段被旧解析忽略 |
| 旧 sidecar 无 phase/nonce | Pending、None，保持旧记录语义 |

保持 FrozenLegacyPeer 双向协议互操作。新增 field 为旧解析可忽略字段，不修改 deny_unknown_fields 的 Settings。实际运行旧版本可能重写 sidecar 并丢掉它不认识的 phase/nonce；磁盘降级并不保证新协议恢复能力仍在。环境回退与旧二进制降级分别验证，不等价。

## 7. 锁与错误

state → settings → pending 统一顺序，检查 session/window、保存 trust 与更新收据在状态锁内。cancel/revoke 不可插在有效性检查与 Settings 写入之间。网络发送不持久化业务事实；发送失败返回 Waiting/记录错误并保留恢复材料。

协议头必须 sequence=0、flags=0，nonce 恰好16字节。错误帧/身份/nonce 不推进；ACK 不授权；接收明文帧不建立认证配对。终态先保存 Completed，再置 Paired；连接关闭最多等待现有 finalize timeout。

## 8. 测试与证据

仓库新增14项 commit 状态机测试、2项 secure 测试、1项 protocol codec 测试、1项 UI 渲染测试；另补已知 legacy note 清理失败兼容测试。原不可读存储 revoke 测试按新增安全语义改为明确断言 error、key 保留但 disabled、原字节保留，未直接删除验收断言。

测试覆盖：正常双端、丢 Persisted/Committed/Ack、最后 ACK 收据重放、ACK 单独不授权、错误密钥/nonce/无 consent、重复与 ACK 乱序、cancel/timeout/revoke、损坏/不可写/恢复可写、check/write 锁交错、能力双向关闭、新命令 gate、未认证明文、wire4/5。

仓库外验收 probe 使用真实 TCP/Noise、两台逻辑设备的独立进程/identity/settings/sidecar，注入 B 的 Settings 写失败；分别正常结束与 SIGKILL 杀死 A，随后双方重启/重新认证且不再 confirm，断言最终双 active、双 Paired、pending 列表为空。第三次独立进程读取确认 identity/trust 持久化、运行时 phase 仍为 Disabled。

证据目录：/Users/monet/TailSync/audits/acceptance-A04-2026-10-01/，实际命令与退出码见 JSON，日志见同名 log。probe 在此目录而非生产 workspace，避免 test-support 进入发布依赖。

这些是本机隔离验证，不等于 Windows 原生、两台真实机器 Iroh 配对、安装包/签名/当前 GitHub CI 验收。

## 9. 与其他 finding 的边界

A01 原 staging 窗口测试通过。A02 的 receiver 重建后重复旧事件、A03 的显式 reopen 丢 history 与外部 image payload 跨 root 无法读取、A05 的 silent supersede 导致 loading 卡住，验收仍复现。本次未混入这些修复，也不将现有 RAM freshness 机制用作配对事务或 durable receipt。

完整验收结论及最终测试统计见仓库外 ACCEPTANCE.zh-CN.md。
