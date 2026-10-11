# Iroh 测试波动诊断与产品影响

日期：2026-10-04。范围：查明此前 Iroh 测试失败的机制、核对生产路径并更新证据；未修改原项目或复合候选的产品/测试源码，未提交、推送或发布。

## 结论与证据边界

1. **确认是基线已有的偶发失败。** 原项目与复合候选第一次串行运行分别为 11/12、10/12；第二次均为 12/12。本轮未修改的基线普通配对和邀请配对各连续运行 100 次，200/200 通过；基线诊断副本的完整 12 项 Iroh 测试连续 32 轮通过。
2. **确认关窗失败的机制。** 两个测试在 QUIC 建连前开启仅 5 秒的配对窗口。窗口到期后，状态机发送 `false`，服务端在处理 Noise 请求前拒绝，随后客户端因服务端退出看到 `ConnectionLost`。回环地址上注入 6 秒延迟可以稳定复现相同错误；仅将窗口改为产品默认的 120 秒，两者均通过。这不是状态机无故关窗。
3. **确认本机存在不可达候选，但尚未锁定最初自然延迟的触发源。** 默认测试地址同时包含 Wi-Fi、Tailscale 和代理隧道。只使用隧道 `198.18.0.1`，连接约 30 秒后超时；其余三个单地址实验均成功。但完整地址集合在连续复测中通过，所以不能把第一次完整地址集合下的失败直接归因于该隧道，更不能声称已经证明 Iroh 的选路缺陷。
4. **产品默认 120 秒窗口不会被同样的 6 秒延迟耗尽；另有既存连接预算风险。** 普通 Iroh 配对及已配对重连对整个建连/握手只给 5 秒，远程邀请建连则给 30 秒。调用未修改的生产 Core 连接竞速函数，在真实回环 Iroh 建连前注入 6 秒等待，5 秒预算取消、30 秒预算成功。此结果证明超时策略边界；真实 N0 地址发现、relay、Windows 和跨设备故障仍未复现，不能登记为已确认的实机产品缺陷。
5. **候选完整 Core 已取得一次通过。** 使用三个协议扩展、串行运行、不排除 Iroh，结果为 **552 passed、0 failed、4 ignored、0 filtered**，22.25 秒。不能据一次通过宣称网络测试的波动已修复，也不能替代公网/双设备验收。

## 基线与候选的可比性

- 原目录：`/Users/monet/TailSync/TailSync-rc-phase35`。
- 候选目录：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`。
- 基线：`f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。
- 候选源码指纹：`68b500c60d2add0947b07126bc5318ff46c662492ddb9a93bc1e3c4e8003d0b3`；本轮核对全部 713 个登记源码文件，未改变。
- Iroh 固定版本为 `1.1.0`；两侧 Cargo.lock 对应 entry 相同。
- 两侧 `iroh_transport.rs`、`pairing/manager.rs`、`pairing/commit.rs`、隔离器及双平台 `network/mod.rs` 内容相同。`secure/handshake.rs` 因复合能力协商有差异，不能笼统声称网络代码全部相同。
- 所有运行使用隔离数据，网络实验串行执行；未停用 VPN/Tailscale、未停止原 daemon、未启动候选 App、未修改日常剪贴板。

逐文件 SHA-256 见 `artifacts/composite-evidence/iroh-diagnosis-2026-10-04/source-comparison.json`。

## 复现和分组实验

| 实验 | 控制条件 | 结果 | 能说明什么 |
|---|---|---|---|
| 原基线首次 Iroh 串行运行 | 原源码、12 项、隔离数据 | 11 passed / 1 failed，12.24 秒；邀请配对关窗 | 新功能不是出现此类失败的必要条件 |
| 复合候选首次 Iroh 串行运行 | 同样 12 项、加复合 feature | 10 passed / 2 failed，50.31 秒；建连超时、普通配对关窗 | 两种错误均存在，需分开判断 |
| 原基线 / 候选第二次 | 不改源码、重跑 | 各 12 passed；3.61 / 5.16 秒 | 能通过，首次失败不能标为固定不兼容 |
| 原基线两个测试重复 | 未插入任何日志或等待；每次独立隔离，串行 | 普通配对 100/100，邀请 100/100 | 本轮未再自然复现慢连接；不是零失败率保证 |
| 基线仪器化完整 Iroh | 仅临时副本增加地址/时间/状态记录 | 32 × 12 全通过；64 次配对建连 27.20–77.23ms，中位 40.51ms | 提供正常时序；不能把副本结果计入正式候选门禁 |
| 固定单个地址 | 保留服务器实际端口，其余逻辑不变 | 回环约 10ms、Wi-Fi约 46ms、Tailscale约 46ms 成功；隧道约 30 秒超时 | 虚拟接口地址并不都可用；未证明它导致完整地址集合失败 |
| 5 秒窗口 + 6 秒等待 | 回环地址；等待放在开窗后、建连前 | 两个原测试均失败，服务端 `Pairing window is closed`，客户端 `ConnectionLost` | 稳定复现原关窗错误的因果链 |
| 120 秒窗口 + 同样等待 | 只改变窗口长度 | 两个原测试均通过 | 产品默认窗口可以容纳这段等待；未执行实际产品邀请入口 |
| 生产 Core 竞速预算 | 真实回环 Iroh；建连 future 前等待 6 秒；只改变预算 | 5 秒组约 5.005 秒取消；30 秒组约 6.019 秒建连；两组断言通过 | 生产 5 秒策略会拒绝这类慢连接；不证明公网自然延迟 |
| 正式候选完整 Core | 原候选源码、三个 feature、串行、不 skip Iroh | 552 passed / 0 failed / 4 ignored / 0 filtered；22.25 秒 | 补齐本轮全量 Core 通过证据 |

最初失败日志没有记录具体候选地址、选中路径、QUIC 各阶段时刻、CPU 调度或接口变化。后续重复通过使这些历史信息无法追溯；**最初自然延迟究竟来自接口/代理、Endpoint 启动时序、丢包或当时负载，仍为 UNCONFIRMED**。不得用故障注入代替这部分观测。

初始排名的假设及当前判断：

- 网络建连延迟耗尽测试窗口：关窗机制已证实，环境存在不可达候选；自然慢连接的具体诱因未锁定。
- 绑定后地址尚未稳定：没有取得导致失败的地址更新记录；保留未证实状态，不能因重复通过而排除。
- 配对状态机提前关窗/复合协商引入延迟：受控实验显示按策略到期；基线也有相同失败，服务端拒绝发生在解析 Noise 能力 payload 之前。现有证据不支持归因于复合能力改动，但并不证明新功能不存在任何网络回归。

## 生产调用链与产品判定

| 路径 | 当前实现 | 本轮判定 |
|---|---|---|
| 测试 Endpoint | `iroh_transport.rs` 两个测试使用 `presets::Minimal` 与立即取得的 `endpoint.addr()`；没有 N0 默认 DNS/relay；测试窗口 5 秒 | 测试受到本机网络地址和短时限影响，属于测试稳定性问题 |
| 产品 Endpoint | `IrohEndpoint::bind()` 使用 `presets::N0`、持久 Endpoint identity、三种 ALPN | 与测试环境有实质差异；本机 Minimal 通过不能证明 DNS/relay 正常 |
| 产品配对窗口 | `pairing.rs` 的 `DEFAULT_PAIRING_WINDOW = 120s`，`PairingManager::new()` 使用该值 | 不是 5 秒测试窗口；按期限拒绝属于正确安全行为 |
| 创建邀请 | 双平台 `network/mod.rs` 的 `create_remote_pairing_invite()`：启动 Endpoint、创建 120 秒邀请、调用 `pairing.enable()` | 创建时即开始计时；没有“收到有效 hello 后重新延长窗口”的实现 |
| 使用邀请 | `start_remote_pairing()`：本端开窗、30 秒建连、preface 各 5 秒、Noise 握手；窗口关闭会取消；失败写入配对状态 | 存在有界失败处理；6 秒注入不会耗尽默认窗口；临近邀请过期仍可能正确拒绝 |
| 普通地址配对 | `start_pairing()` 外层 `timeout(HANDSHAKE_TIMEOUT=5s, ...)` 包含 Endpoint、Iroh 建连和 Noise | 冷启动 DNS/relay或弱网可能超过预算；属于既存策略风险，尚无本轮实机故障证据 |
| 已配对同步重连 | canonical `platform-network-pool.rs::race_connect_and_handshake()` 将同一个 5 秒预算传给 Core `race_connections()` | auto 模式其他路由可先成功；仅 Iroh 可用时更依赖该预算。Core 使用 JoinSet 取消未胜出的连接尝试，不无限挂住 |

三个超时不能混淆：**120 秒允许发起配对的窗口、30 秒邀请 QUIC 建连截止、5 秒普通连接/握手截止**。延长测试窗口不能修复产品冷连接预算，放宽产品安全窗口也不能修复不可达地址。

现有证据的分类：

- **已确认的测试问题**：用实时多接口网络承担协议 round-trip 测试，再用 5 秒配对窗口覆盖整个过程，导致网络慢时被判为配对失败。
- **已确认的环境现象**：本机代理隧道单地址不能建立这条 Iroh 连接。
- **产品风险待验收**：5 秒普通 Iroh 配对/重连预算可能阻止本可稍后成功的冷连接。受控实验已验证预算边界，尚未确认真实用户影响程度。
- **未证明**：复合剪贴板引入这次故障、Iroh 库本身的选路 bug、默认邀请配对必然失败、已配对信任丢失、真实 Windows/公网行为。

## 建议的后续执行

1. 协议/配对 round-trip 测试使用明确回环 `EndpointAddr`，保留真实 QUIC + Noise；外层建连截止与配对窗口分开。窗口采用生产默认值或在连接准备好后才开始测试用的短窗口；短窗口安全拒绝继续由独立到期回归覆盖。无需调用 Minimal 的 `online()` 等待不存在的 relay。
2. 单独保留网络验收：真实多网卡、代理/Tailscale 开关、冷启动、不同出口、强制 relay、睡眠与切换；记录网络/源码/包版本及各阶段时间。下一次自然失败必须采集地址集合、路径状态、建连与握手耗时，才能确认最终触发源。
3. 在 N0/两设备环境测量普通配对和重连超过 5 秒的成功样本；据此决定是否为 Iroh 单独设置连接预算，保留 LAN 的快速竞速、取消、总操作截止和 120 秒安全窗口。本轮 30 秒实验只是对照，未作为生产改动建议的最终值。
4. 继续保持 Windows 原生、双设备、公网 direct/relay、目标应用图文粘贴为 UNKNOWN。不得把本地完整 Core 通过改写成发布门槛完成。

本轮没有应用上述测试或产品修复，因此状态是“诊断机制已确认、自然触发未完全锁定、风险已登记”，不是“网络波动已修复”。

## 命令与证据

正式候选完整 Core：

```bash
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  -p tailsync-core \
  --features 'protocol-composite-clipboard protocol-file-sliding-window protocol-image-compressed-chunks' \
  --lib -- --test-threads=1 --nocapture
```

证据目录：`artifacts/composite-evidence/iroh-diagnosis-2026-10-04/`。

- `summary.json`：结果、局限、每份日志 SHA-256 与本轮完整 Core 摘要。
- `source-comparison.json`：基线/候选关键源码、依赖的比较。
- `commands.json`、`pristine-runs.json`、`observed-*.json`、`address-runs.json`、`window-runs.json`、`budget-runs-v2.json`：执行命令与逐轮结果。
- `routes.json`：本机三条路径的只读路由快照；它不代表最初失败时的路由状态。
- `DEBUG-HARNESS.md`、`scratch-test-harness.patch`：明确标识的测试副本与复现方式；未应用到产品目录。
- `logs/`：初始基线/候选对照、250 份诊断/重复日志以及正式完整 Core 日志。故障注入的预期失败、第一版预算探针的清理失败均保留，正式比较采用 v2。

本轮只增加/修正文档和诊断证据。当前 daemon 与正式数据保持原状。
