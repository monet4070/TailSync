# TailSync 整改独立复核与改进方案

审查日期：2026-09-30。审查对象：`codex/remediation-2026-09-29`，固定提交 **`914638d125e651c9d08052d20a8cc9ad77deda9a`**。工作树：`/Users/monet/TailSync/TailSync-remediation`。比较基线：`origin/main = b61a0a04b88f5828707f7ef64781389944471be8`。

本次只审查、执行验证和保存报告/复现材料；没有修改产品实现、原测试、台账状态，没有提交、推送或合并。以下结论针对上述提交，不沿用历史报告的完成状态。

## 1. 总体判断

**这轮做了大量真实且有价值的修复，两平台 CI 全绿及具名门禁实际执行的说法成立；“28 项因此全面完成”“单机能做的已全部完成”不成立。**

比较好的改动包括：ACK 超时后换认证连接再重试、同一 hostname 保留活跃 worker、匿名异常不消耗锁定预算、接收配额扣除已有 `.part`、图片读取先查类型、GC 按物理文件身份保留存活载荷、真实执行结果与台账对账。多数是沿既有 Core/适配器边界完成的小改动，不需要推倒重写。

但是，现有门禁通过仍可同时出现以下行为：

| 优先级 | 本次发现 | 证据层次 | 关联条目 |
|---|---|---|---|
| P1 | 删除普通历史记录，实际删除另一条收藏记录仍引用的文件 | 当前 macOS/APFS 生产 API 实测 | S5-P2-1；同时影响配额淘汰/重复替换 |
| P1 | 遗忘设备并清除 pending 后，旧配对会话的迟到确认重新写回信任 | 当前 Core、真实 Noise 会话、落盘实测 | S3-P1-2 |
| P1 | 信任已经落盘，收尾期间取消却返回 `Cancelled`，信任仍有效 | 当前 Core、延迟传输关闭实测 | S3-P1-2 |
| P1 | 已知同名不同公钥冲突仍先发送 `PairingPersisted`，之后才拒绝 promote | 当前 Core/Noise 实测；平台配对路由可达 | S3-P1-2 / S3-P1-3 |
| P1（证据缺口） | S2-F5 的性能关闭依据没有运行生产投递器，也没有文件 ACK 等待 | 基线代码与生产调用链确认 | S2-F5 |
| P2 | 提示槽拒绝展示警告，前端仍确认并消费它 | 当前前端代码副本、组件实测 | S6-P2-4 |
| P2 | 通知游标元数据和通知列表不在同一锁内取快照，溢出仍可能被漏报 | 代码可达交错；未执行确定性竞态注入 | S6-P2-1 |
| P2 | 普通删除在长读事务下仍同步等待约 5 秒 | 默认数据库配置实测，约 5205 ms | S5-P2-2 / S4-P1-3 |
| P2 | pending 写失败仍改变内存记录；未知格式可能被后续写入覆盖 | 写失败实测；未知格式为代码推导 | S3-P1-2 |
| P2 | LAN 自地址选择仍接受 Tailscale ULA、无 scope 的链路本地 IPv6；文档错误声称拒绝 APIPA | 选择器实测；平台端到端影响有边界 | S1-P0-1 / S1-P1-4 |

这里没有把所有缺口都判成新引入的回归：例如普通删除的大小写别名问题属于**已有生命周期路径未被本轮 GC 修复覆盖**。本次报告评价的是当前实现和完成声明，不将遗留问题一律归罪于这轮改动。

**建议重新打开 S3-P1-2、S2-F5、S6-P2-1、S6-P2-4 的完整关闭结论；为外部载荷的全删除路径另建 P1 修复项。** S5-P2-1 的“周期 GC 已接线且避免误扫存活文件”本身成立，但不能扩大成“所有载荷删除都安全”。S4-P1-3 应拆成“重复扫描已修”和“阻塞/锁等待仍待补”。这比机械地重新统计一个 `fixed_gated` 总数更准确。

## 2. 证据、基线与验证范围

### 2.1 阅读与核对对象

阅读了 `CONTEXT.md`、原审计/整改方案、`FINAL-REPORT.md`、`PHASE-4-REPORT.md`、`REVIEW-FIXES-2026-09-30.md`、性能/匿名字段裁定、验收环境、31 个台账条目，以及实际实现、平台调用点、具名门禁、结果记录/核对脚本和 CI 配置。历史文档只作为审计输入，判断依据是当前代码。

### 2.2 GitHub 与真实 CI 产物

| 对象 | 当前核对结果 |
|---|---|
| 远端整改分支 | 指向 `914638d`，与审查提交一致 |
| 相对 main 的提交数 | **56**；用户提供总结的 54 已过时 |
| [run 36707387508](https://github.com/monet4070/TailSync/actions/runs/36707387508) | `b632b7c`，success |
| [run 36709267509](https://github.com/monet4070/TailSync/actions/runs/36709267509) | `046b2a0`，11 个 job 全部 success；两平台 `Verify remediation gate execution` 均 success |
| 第二次运行的下载产物 | `remediation-tests-macos`、`remediation-tests-windows`，每个平台 4 份 JSON/原始日志；记录的 SHA 均为 `046b2a0`，退出码 0 |
| 当前台账在上述产物中的覆盖 | macOS 28 个、Windows 27 个适用 gate 均有通过证据；计数含适用的 accepted/partial 门禁与 Windows 成品集成断言，并非“每个平台都执行全部 28 个 fixed 条目” |
| `b632b7c..914638d` | 仅 3 个文档/台账文件改变，没有产品实现改变；因此上述 CI 可支持当前生产代码，但不是当前 HEAD 的逐字文档认证 |
| [PR #68](https://github.com/monet4070/TailSync/pull/68) | 核对时仍 OPEN、mergeStateStatus=CLEAN；本次没有合并 |

下载的 Windows 产物明确包含 S1-P1-5 的具名门禁通过。它的条目却仍写“尚未在原生 rust-windows CI 上执行”，最终报告又将 partial 的理由写成“macOS 无对应门禁”。**这两种关闭口径不一致，且前一种已经被真实 CI 证据更新。** 应先统一平台范围，再更新状态；不能仅凭计数需要改变它。

“全部做过变异验证”属于既有报告的历史声明。本次确认了门禁存在、执行通过和所测边界，并阅读了部分变异记录，**没有重新执行全部 28 项历史变异，也没有据此给全量变异认证**。变异能证明被选中的坏行为会被抓住，不能证明一个修复不存在其他坏行为。

### 2.3 本次重新执行

| 检查 | 本次结果 |
|---|---|
| Core 完整测试，串行、隔离数据目录 | 单元 472 passed / 2 ignored；两个集成套件各 1 passed，另有 1 ignored；无失败 |
| tailsync-runtime | 23 passed，无失败 |
| macOS Rust `--all-targets`，串行 | 109 passed / 1 ignored，无失败 |
| Swift 完整测试 | 209 executed，3 skipped，0 failures，即 206 passed |
| 当前 HEAD 的 macOS 实际执行结果/台账核对 | `remediation ledger OK: 31 entries` |
| 台账、执行记录、CI scope 三组 Node 测试 | 62/62 passed |
| 跨平台同步检查 | passed |
| History、useHistoryNotice、useRuntimeSnapshots 现有前端测试 | 32/32 passed |
| 独立 Rust 复现程序 | 复现配对、pending、删除等待、收藏载荷丢失、LAN 选择边界，退出码 0 |
| 前端独立反例 | 1 个针对实际 History 组件的反例测试通过，证明未展示的警告仍被 ack |

复现材料保存于 [review-evidence-2026-09-30](review-evidence-2026-09-30/README.md)，包含源码、补丁、日志及 [验证摘要](review-evidence-2026-09-30/verification-summary.json)。反例程序的“通过”表示**确认坏行为可达**，不是修复验证通过。

本次没有重新执行全部 Clippy/发布构建，没有操作最终 `.app` 做 VoiceOver、托盘交互，没有 Windows 真机或双设备验收。已有 CI 证明托管平台构建/门禁，不能替代这些验收。

## 3. 31 项逐项裁定

“成立”表示原问题的对应修复和现有测试边界相符；“有限成立”表示核心改动有效，但完成措辞或生产边界需要收窄；“不能完整关闭”表示仍有相关的可达缺陷或关键证据缺口。详细问题见第 4 节。

### 切片 1：地址、模式和发现

| ID / 台账状态 | 当前代码裁定 | 轻量性、覆盖与改进 |
|---|---|---|
| S1-P0-1 / fixed_gated | **有限成立**。平台已不用 `8.8.8.8` 探针，使用 up/非 loopback/非 p2p 接口及共享选择器，空地址不再包装成 `0.0.0.0`。 | 结构小且合理。但选择器可返回 Tailscale ULA、APIPA、无 scope IPv6；不能声称排除了所有 VPN 或绝不返回 APIPA。见 R10。 |
| S1-P0-2 / fixed_gated | **成立**。Tailscale 地址判定在 LAN ULA 判断之前，lan_only 拒绝 Tailscale，其他 ULA 仍按 LAN 规则处理。 | 共享纯函数足够，不需要扩大成整套路由框架。模式准入正确不意味着本机自地址选择也一致。 |
| S1-P1-3 / fixed_gated | **成立于排序边界**。候选按状态、延迟与地址级别排序，私网 IPv4 优先于 APIPA/loopback，避免纯字符串排序。 | 小改动合理。它是排序而非可达性认证；真实路线仍需认证连接建立与失败回退。 |
| S1-P1-4 / fixed_gated | **有限成立**。peer socket、候选和配对输入对无 scope 的链路本地 IPv6 作明确拒绝，避免 scope=0 硬拨。 | 当前选择“明确拒绝”比半实现 scope 支持更轻量。但本机 LAN 选择器仍可产生这种地址，发送/展示边界不一致，见 R10。不应声称完整支持 IPv6 LAN。 |
| S1-P1-5 / partial | **Windows 核心修复成立；台账理由过时**。生产 discover 与可注入发送路径共用，过滤接口，区分成功发探测但 0 peers 与全发送失败；Windows 原生 CI gate 已通过。 | 保持返回接口、补生产 seam 合理。macOS 也有接口过滤/发送结果处理，应补同目标门禁，或明确原条目仅 Windows；UDP send 成功不能证明物理链路连通。 |

### 切片 2：投递、连接池和性能

| ID / 台账状态 | 当前代码裁定 | 轻量性、覆盖与改进 |
|---|---|---|
| S2-F1 / fixed_gated | **成立**。事件、文件/窗口及批次 ACK 超时结束本次尝试，worker 经新认证流再投递；ACK 序列/ID 校验保留。 | 根因对应且比同流 drain 猜测更稳健。现有确定性门禁有价值，真实抖动/两端恢复另验。 |
| S2-F2 / fixed_gated | **成立**。投递失败进入退避，退避可被 shutdown 和重试唤醒打断。 | 很小的生命周期修复；无需增加另一个重试调度系统。 |
| S2-F3 / fixed_gated | **成立于实际复用行为**。按 hostname 寻找活跃 sender，路线变化通过 watch 更新，保留 worker/待投递帧。 | 底层 Map 仍以 `(target, hostname)` 为键并查找复用，但小规模 peer 集合下不值得仅为形式统一重写。不要把它描述成存储结构完全改为 hostname。 |
| S2-F4 / fixed_gated | **成立**。首选快速失败会唤醒延迟候选，成功后 abort 其他竞速任务。 | 保留原竞速设计的最小补充，故障注入测试覆盖明确。 |
| S2-F5 / fixed_gated | **不能据现有基线关闭性能问题**。生产仍共用 priority，且同一 worker 等待文件 ACK 时不调度文本。 | 背压和生产容量门禁有效；200 µs 调度模型不能支持真实文件传输预算裁定。先重测生产路径，再决定是否需要独立认证文件连接。见 R5。 |

### 切片 3：配对、信任和协议

| ID / 台账状态 | 当前代码裁定 | 轻量性、覆盖与改进 |
|---|---|---|
| S3-P0-1 / fixed_gated | **成立于锁定预算和槽位策略**。本地未确认时匿名异常不消耗失败预算，未确认会话可被替换，确认后限制抢占。 | 行为有界、测试针对原因；这不能扩大成“匿名客户端完全无法造成拒绝服务”。 |
| S3-P1-1 / accepted | **接受边界已记录**。保留主动匿名握手可见字段，身份字段门禁存在；Noise Message 2 的加密不能阻止主动握手对端看到字段。 | 不为已接受的信息披露改协议是合理选择。本次核对了决策文档存在，未独立重新认证历史签署授权。建议标题移除“待裁定”，并保持整个字段矩阵/广播面与契约一致。 |
| S3-P1-2 / fixed_gated | **重要核心修复成立，但不能完整关闭**。pending 不参与信任准入，peer completion 前不写 active，旧版 Settings 兼容夹具和关键锁窗口门禁有效。 | sidecar 和不新增命令的兼容性考虑合理；撤销、提交点、冲突预检、pending 失败原子性和用户可恢复出口不完整。见 R2/R3/R4/R9。 |
| S3-P1-3 / fixed_gated | **信任锚保护成立**。Settings 不覆盖不同公钥；发现适配器 remember 空操作阻止匿名广播持久化可信路线，共享平台 gate 覆盖该向量。 | 防线轻量且位置合理。配对流程直到 promote 才碰到公钥冲突，造成先报 persisted 后失败；不是 pin 被覆盖，见 R4。 |
| S3-P1-4 / fixed_gated | **成立**。固定同一读取 future，window 变化不会重启半读帧，避免 read_exact 取消导致流错位。 | 正确利用异步生命周期，无需额外帧缓存协议。 |

### 切片 4：文件、配额和权限

| ID / 台账状态 | 当前代码裁定 | 轻量性、覆盖与改进 |
|---|---|---|
| S4-P1-1 / fixed_gated | **成立**。peer 接收并发额度对 active、pending、inflight 的 transfer 集合计数并去重。 | 在既有状态中修正计数，比另设独立计数器轻量；仍要保留并发 admit 和失败释放测试。 |
| S4-P1-2 / fixed_gated | **成立**。匹配批次/来源的 `.part` 长度作为已占物理空间，从尚需接收量扣除；加密提交副本预算独立计算。 | 修复了双记账，没有把 SQLite DELETE 当作空间释放；真实中断/重启续传仍是设备验收。 |
| S4-P1-3 / fixed_gated | **重复扫描修复成立，阻塞目标未全面解决**。一次物理基线加实际 freed 与 db/wal/shm 净变化，准入锁覆盖并发状态读取和预留。 | 账目方案合理，不必先上持久字节计数器。但初次扫描、逐次删除 checkpoint 和 GC 仍可同步占 Tokio/DB 锁；R8 的 5 秒等待就在生产默认路径。 |
| S4-P1-4 / unfixed | **如实未修**。平台进度回调仍在引擎锁内，20 ms 是暂定值。 | 不应为了未测的预算盲目重构。但 callback/锁的本机故障注入和 loopback 测量可先做，不能把所有工作都归因于缺第二台机器。 |
| S4-P1-5 / fixed_gated | **共享校验修复成立**。多 peer 复用准备/预校验结果，restart 批次复用条目哈希；完成后的源变更验证仍保留。 | 没有为减读盘去掉完成语义，方向合理。现有具名 gate 主要测试共享 helper，需补真实 fanout/restart 调用次数，见 R11。 |
| S4-P2-1 / fixed_gated | **成立于权限 helper 及其调用**。新建目录时限制权限，Unix 0700/Windows 权限分支有门禁。 | 避免先默认创建再 chmod 的窗口；门禁还应从实际 incoming 创建入口检查，以防调用点改回裸 create_dir_all，见 R11。 |
| S4-P2-2 / fixed_gated | **成立**。常驻维护定时器清理超龄传输孤儿，保留近期/嵌套受保护文件。 | 既有任务中接线轻量；这不负责 payload atomic `.tmp`，不能与 GC 生命周期混为一谈。 |

### 切片 5：历史和外部载荷

| ID / 台账状态 | 当前代码裁定 | 轻量性、覆盖与改进 |
|---|---|---|
| S5-P1-1 / fixed_gated | **核心成立**。为物理配额只淘汰可释放的外部载荷，保留 inline 历史；没有通过无效 DELETE 假认领空间。 | 处理了根因，标题“不以文件系统尺寸作判据”不准确：实际仍依赖物理基线，这是应保留的行为。共享载荷误删边界见 R1。 |
| S5-P1-2 / fixed_gated | **成立**。共享 runtime 先读类型/元数据，非 image 在 prepare/decrypt 前拒绝，两平台走同一用例。 | 很轻且有效。可补损坏大 file 引用夹具证明无需解密，但没有必要为此改载荷格式。 |
| S5-P2-1 / fixed_gated | **GC 子目标成立，载荷生命周期不完整**。已接周期任务，按 dev/inode 或 Windows 文件 ID 对账，完整 live set 失败时不盲删。 | 这部分很好；普通 delete 和 duplicate cleanup 仍以 reference 字节相等判断共享，实测可删除收藏载荷，见 R1。atomic `.tmp` 永久豁免另有回收边界，见 R12。 |
| S5-P2-2 / fixed_gated | **结果语义成立，等待预算欠缺**。读取 checkpoint 返回的 busy/log/checkpointed，删除提交成功后不再报业务失败，忙状态可观察。 | 普通删除仍继承 5 s busy timeout；门禁将其设成 50 ms，只验证结果，没有验证生产延迟。迁移 v9 已有短重试，不能据此称所有 checkpoint 都有界，见 R8。 |

### 切片 6：本地 API、通知和客户端

| ID / 台账状态 | 当前代码裁定 | 轻量性、覆盖与改进 |
|---|---|---|
| S6-P0-1 / fixed_gated | **成立**。Windows 编译条件阻止启动旧 TCP JSON API；原生成品 job 有端口/进程归属断言，真实 CI 产物包含成功标记。 | 编译边界 + 成品运行是恰当双层保护。INDEX 显示 gate 名 `undefined` 是展示问题，不代表没有集成 gate；应修生成器显示 marker。 |
| S6-P2-1 / fixed_gated | **不能完整关闭**。新增 earliest/dropped/service_instance，Swift gap/restart 策略正确，但快照两次读取不是一个原子窗口。 | 字段设计合理，不需要新消息总线；合并通知快照 getter 即可修竞态，见 R7。当前 gap 只 print 日志，如产品要求用户知道漏通知，应提供一次可见提示。 |
| S6-P2-3 / fixed_gated | **成立**。Swift capabilities 明确拒绝 Windows/非 macOS；对应协议夹具门禁通过。 | 在解码/平台边界拒绝，比通过 watchdog 猜错重启更直接。 |
| S6-P2-4 / fixed_gated | **后台守卫成立，展示/确认契约不完整**。Core peek/ack(id) 正确，隐藏文档不 ack；可见文档中的 notice helper 可拒绝展示，仍被 ack。 | ID 化和生成命令契约合理；须用实际展示回执消费，见 R6。原生 hide/minimize 与 document.visibilityState 的关系仍待 Windows UI 验收。 |
| S6-P2-6 / fixed_gated | **实现成立、门禁只覆盖辅助函数**。wait_runtime_snapshot 纳入 until_disconnect，断线 drop future 可释放持有许可。 | 轻量 RAII 方案很好；当前测试直接调用 until_disconnect，不能证明实际命令持续留在取消分支，需补 route/semaphore 集成 gate，见 R11。 |

## 4. 仍不到位的问题与最小修复

### R1 — P1：共享外部载荷的普通删除仍可损坏收藏

**已实测。** 在本机大小写不敏感 APFS 上，使用公开生产 API：

1. `add_file("report.txt", bytes)`，将第一条设为收藏。
2. 用相同 bytes 添加 `REPORT.TXT`，保留第二条普通历史记录。
3. 两个返回路径对应同一物理文件；这是现有内容哈希/文件名命名和复用允许的状态，不需要手工污染 DB。
4. 删除第二条后，收藏行仍在，但 `get_data(first_id)` 失败，其路径已经不存在。

本次输出：`delete_case_alias same_file=true favorite_still_readable=false favorite_path_exists=false`。

根因位于 [lifecycle.rs](../../shared/rust-core/src/db/lifecycle.rs)：删除提交后，389 行按 `history.data = stored` 的**字节相等**查询剩余引用，417 行 unlink；大小写别名的 reference 序列化不相同，于是错误判定无引用。410 行 preserve_path 也仅比较路径拼写。`entries.rs::cleanup_external_payloads` 有同类判断。GC 新增的 `referenced_payload_identities` 已解决**扫描器**的同一问题，但没有被所有 unlink 路径复用。

**最小修复方案：** 把“提交后的存活外部引用是否仍指向此物理文件”收敛成一个 DB 层操作，并让普通删除、批次删除、配额淘汰、重复替换、清空及 GC 共用。以现有 `PayloadFileIdentity` 判断物理同一性；preserve_path 也转换为身份。解析 live set/读取身份失败时保留候选、留下诊断，不以猜测 unlink。批量操作只构造一次 live set、按身份去重释放计量，避免每条扫描全库。无需先做文件名迁移或新增表。

**必要门禁：** APFS/NTFS 大小写别名与 Unicode 等价名；收藏与普通记录共用后删除普通记录；同一载荷两普通行，删第一行仍可读、删最后行恰好释放一次；quota eviction / duplicate replacement / preserve_path 都经实际生产入口；解析失败不误删。敏感文件系统上同时确认独立 payload 能正常删除，不能把全部删除豁免。

**风险与边界：** 本机已确认；Windows 文件系统需原生执行相同门禁。P1 的原因是静默丢失用户明确收藏的数据，应先于性能深化修复。

### R2 — P1：遗忘设备没有使正在进行的配对失效

**已实测 Core；平台调用顺序已核对。** 双方 Confirm 后、本方 pending 落盘、尚未收到对端 `PairingPersisted`：调用 `Settings::forget_peer` 和 `PairingManager::forget_pending`，确认 trust=false、pending=0。随后同一个已认证旧会话发来 completion：状态变 Paired，trust=true。

macOS Unix 路由 [peers.rs:125](../../macos/src-tauri/src/api/routes/peers.rs) 以及两平台 Tauri `commands/peers.rs` 正是先 forget settings、断 pool、再 forget_pending。它们**没有取消/失效 PairingManager 的该 peer 会话**。pool 的数据 worker 与正在配对的会话不是同一个生命周期。`promote_pairing` 只检查配对窗口/会话 ID，pending 已被删不构成阻止条件。

**最小修复方案：** 在现有配对/信任用例层增加统一 `revoke_peer(hostname)`：协调 state 与 settings 锁，使该 peer 的活动 session 先失效、控制通道结束，再持久删除 active/address/pending；返回后任何旧 session 不得 promote。所有 Unix/Tauri 入口调用同一用例，断 pool 作为撤销的后续动作。明确并保持锁序（当前 promote 是 state → settings；不能新增 settings → state 的反向嵌套）。若删除落盘失败，应如实返回失败，不能报告完成。新配对必须由之后明确的新操作创建新 session；不引入永久禁配黑名单。

**必要门禁：** 真实生产撤销用例，延迟 ack 在撤销前/后到达、record_pending 写入中撤销、重启后查看 settings+sidecar；跨三个实际适配器调用点检查一致。预期撤销成功返回后 trust 永远不被旧会话恢复，直到用户创建新配对。

### R3 — P1：持久提交点与 `Paired` 状态不是一个原子转变

**已实测。** `promote_pairing` 在 [manager.rs:697](../../shared/rust-core/src/pairing/manager.rs) 持 state 锁写 trust，修好了先前“检查后解锁”的窗口。但 717 行释放 state 时还没有将会话置为不可取消的成功状态。随后进入 `Finalizing`，等待传输 shutdown（597–608 行），最终 `finish_success` 才写 Paired。`cancel()` 在 297 行无条件把状态变 Cancelled、增加 session ID。

复现让 transport shutdown 等待，进入 Finalizing 后调用 cancel，得到：`phase=Cancelled trust=true disk_trust=true`。旧 session 的 `finish_success` 因 ID 改变不再更新状态。超时/新窗口等生命周期操作也需要按提交点审查。

这是**用户取消结果与权威信任相矛盾**，不是“双方已 Confirm 所以安全”能够消除的问题；也不是两将军限制。应用可以在本机给出一致的提交/撤销语义。

**最小修复方案：** 将 trust 成功持久化与“会话已提交，不再受 pairing cancel/expire 回滚”放在同一次 state 临界区完成。公开状态在此置 Paired，或内部设置 committed 并让状态/取消响应统一返回已提交结果；移除 control、关闭窗口、清 deadline。传输关闭在之后执行，不参与成功裁定。真正要撤销已提交 trust 必须走 R2 的 revoke 用例，不把取消收尾误当撤销。不要为修 UI 结果而在 close error 时回滚持久 trust。

**必要门禁：** promote 成功后、pending 清理前/后、shutdown 挂起/错误/超时、cancel/expire/new-enable 并发插入；内存状态、settings 磁盘、两端公开结果一致。现有 promotion-gate 测的是提交前竞争，不替代提交后门禁。

### R4 — P1：已知无法 promote 的公钥冲突仍先报告 persisted

**已实测。** 预置 hostname=`client` 的旧 pin，用另一个 key 建立配对会话并双方 Confirm。对端已经收到本方 `PairingPersisted`，本方收到对端 completion 后才因 Settings 的 key 冲突拒绝保存，退回 Waiting；旧 pin 保留。

当前 key guard 在 [crypto.rs:302](../../shared/rust-core/src/crypto.rs) 是正确的，不能删除。问题是 run_session 先写 pending、发 completion，直到 `promote_pairing` 才发现**在发帧之前就存在且已知**的冲突。平台 `HandshakePurpose::Pairing` 路径 [platform-network-server.rs:440](../../shared/platform-network-server.rs) 直接安装 pairing session，不走普通已配对 peer 的准入检查；该情形不是仅测试程序捏造的不可达入口。

**最小修复方案：** 在安装会话或双方确认后、发送 completion 前，检查 hostname 当前 pin 是否不存在或与当前认证 key 一致；不同 key 明确结束并要求用户先撤销旧设备。同样的检查在 promote 保留，以覆盖检查后变化。结合 R2 的同 peer 撤销/会话协调。预检无法保证磁盘永不失败，也不能消除分布式不一致，但能避免这个确定、已知的失败。

**必要门禁：** 同名同 key 重配成功；同名不同 key 在任何 `PairingPersisted` 发出前失败，旧 pin 不变；预检之后 pin/revoke 并发变化不能覆盖或复活。不要恢复旧版本静默覆盖策略。

### R5 — P1 证据缺口：S2-F5 的“达标关闭”测量对象不正确

生产 [pool.rs:67](../../shared/rust-core/src/peer/pool.rs) 只将 FileChunk 分到 bulk，FileMeta/其他控制与即时事件仍走 priority；[worker.rs:490](../../shared/rust-core/src/peer/delivery/worker.rs) 等 `deliver_pending_frame` 完成才继续取下一帧。文件 ACK 的默认 timeout 是 **10 秒**（不是旧审计写的 40 秒）。因此已经开始的文件等待不能靠下次 priority 优先调度抢占。

基线 [tests.rs:2175](../../shared/rust-core/src/peer/delivery/tests.rs) 存在以下限制，不能支持当前关闭结论：

1. 实际消费者只调用 `receive_scheduled_frame`，然后固定 sleep 200 µs。没有生产 executor、认证传输、接收端、FileMeta/FileBatch ACK 等待。
2. 256 个所谓 1 MiB FileChunk 实际都是 `vec![0;64]`，64 个 FileMeta 也不是合法 manifest；单批最大文件数是 20，64 文件“单批”不符合生产形态。
3. 一次文件突发很快结束，之后继续发文本到 60 秒；大部分是文件结束后的样本，整体 p99 会掩盖短暂文件活动阶段的延迟。
4. peak 每 50 ms 从文本生产循环取样，可能漏过真实队列峰值。`dropped` 只统计 try_send 失败，没有硬断言 attempted=enqueued=实际 ACKed；入队后未交付不在这个零丢数定义里。
5. CI 的容量/背压门禁只证明 64 容量与“满时不静默丢弃”。发出 delivery_stalled 警告不等于“永久丢事件为 0”，更不等于 p99 ≤100 ms。

报告坦率写出服务时间是模型参数，这一点很好；但是“模型边界有披露”不能让未进入模型的原故障获得关闭证据。**不建议立即照旧审计上独立连接；先将状态改为性能尚待裁定，测对对象。**

**测量方案：** 用生产 worker/executor 和合法 ≤20 文件的 ≥256 MiB 批次，在单机认证 loopback/duplex 驱动真实帧与接收逻辑；故障 seam 注入 FileMeta/批次/窗口 ACK 延迟（正常、50 ms、500 ms、2 s、超时）。每 50 ms 的文本贯穿文件活动阶段，记录 enqueue、发送、ACK 三时间点，核对 attempted/enqueued/acked/rejected/expired，enqueue 钩子记录峰值。分别报告文件活动期间 p99/max 与全程值；跑≥3轮并记录环境。模型调度测试继续保留，但重新命名并收窄其说明。

**若生产路径确实不达标：** 最小有效隔离是同 peer 的即时事件 worker 与文件 worker 各持已认证连接，保持 pin/模式/限流/撤销/断线恢复不变量，批次串行器仍归文件路径。仅加第三条 channel 或调整 priority 比例不能抢占已开始的 ACK await。只有测量证明需要时才引入该复杂度。

### R6 — P2：文档可见不等于警告已展示

**已实测。** [History.tsx:375](../../windows/src/pages/History.tsx) 调用 `showHistoryNotice` 后无条件 `ackSyncWarning(id)`。但 [useHistoryNotice.ts:54](../../windows/src/hooks/useHistoryNotice.ts) 在 cooldown 中直接 return；56–62 行可因持续可见预算用尽清空 notice 并 return。

前端反例使用相同 History 实现，三条警告分别在 0/4/8.001 秒到达：101、102 被展示；103 导致 notice 槽清空，DOM 没有 103 的文本，却已调用 `ack_sync_warning({id:103})`。现有隐藏/可见门禁全部可以继续通过。

**最小修复方案：** notice API 返回接纳/延后结果，或在 React 实际可见 notice 提交后发包含 warning ID 的展示回执。只在可见且该 ID 进入实际展示槽时 ack；被 cooldown 拒绝的 warning 留在 daemon，安排 cooldown 后重试。用 ID 去重及 ack 重试状态避免未成功 ack 的同一 warning 被每次 poll 反复累计次数。不要延长提示到无限可见。

**必要门禁：** cooldown 拒绝、8 秒预算边界、被别的 error 占槽、ack 请求失败、连续同 ID、下一条同 kind/peer、隐藏到重新显示；每个被 ack 的 ID 必须有实际展示回执。Windows 最小化/托盘可见性的原生验收单独补。

### R7 — P2：通知窗口元数据必须与条目原子读取

**代码确认的可达竞态，未做本次确定性执行注入。** [routes.rs:142](../../macos/src-tauri/src/api/routes.rs) 先读取 earliest/dropped，151 行再读取 entries；两个 getter 分别锁住同一个 buffer，中间可以发生 push/overflow。

例如 cursor=0，第一次读 earliest=1；并发插入将 buffer 溢出，第二次取得的 entries 从 10 开始，响应仍携带 earliest=1。Swift [TailSyncApp.swift:91](../../macos/swift-ui/Sources/TailSync/TailSyncApp.swift) 判 contiguous，随后把 cursor 推到最新 ID，1–9 的丢失不再被下一轮检测。starting revision 让下次 wait 立刻返回，但不能撤回已经推进的 cursor。当前客户端也没有以 dropped_total 独立修复这个窗口。

**最小修复方案：** 一个 `notification_snapshot(since)` 在一次 buffer lock 中返回 entries、earliest、dropped；路由只消费这份快照。无需让整个 runtime snapshot 持全局大锁，其他字段保留现有 starting revision 机制。

**必要门禁：** 原快照的两读取边界之间强制 overflow，证明旧实现漏 gap；新 getter 的条目/earliest 永远对应同一窗口。再覆盖实例切换、空队列、刚好32条、游标已追平。若用户需要知道漏掉通知，gap 应有一次可见诊断，不能只依靠 print。

### R8 — P2：结果容错已修，但默认删除仍同步等待约 5 秒

**已实测。** 使用生产 `HistoryDB::new()` 默认配置，第二个 SQLite 连接持有读事务后调用普通 delete：成功返回，但耗时 **5205 ms**。`db/open.rs` 设置 5 s busy timeout；delete 430 行的 TRUNCATE 沿用此配置。现有 blocked-checkpoint gate 为快速造 busy，将 timeout 改为 50 ms，因此没有测默认延迟。v9 迁移的短重试是另一个路径。

配额 reserve 在 [platform-network-server.rs:389](../../shared/platform-network-server.rs) 的 async 路径持 DB 锁同步执行淘汰；周期 GC 也在 async tick 内持 DB 锁直接扫盘。重复扫描降为一次确实有收益，但没有消除同步磁盘操作、checkpoint 等待对执行器和历史查询的影响。

**最小修复方案：** 分开两种 checkpoint 责任：安全迁移保留明确的完成/重试判定；用户删除的 post-commit checkpoint 设置短等待预算并恢复原配置，忙时登记待处理，维护任务再做 TRUNCATE。也可使用非等待式 PASSIVE 做推进，但不能称其已完成安全清除。通过既有 blocking DB 用例边界执行扫描/淘汰/GC，保留 file_batch_admission_lock 跨状态读取和预留；不要为减锁把一次基线移到锁外后继续用旧值认领空间。

**必要门禁：** 不修改默认数据库配置的长读事务删除；操作成功、忙诊断、等待预算可核对，释放读者后维护完成；短预算设置恢复；多条淘汰不累加 N×5 s；并发批次不超额、Tokio heartbeat 可继续推进。时间门槛应有宽容的端到端防回归与确定性的忙预算断言，不再依靠极小墙钟值。

### R9 — P2：pending sidecar 失败原子性和恢复出口欠缺

**写失败已实测。** [pending.rs:148](../../shared/rust-core/src/pairing/pending.rs) 先 retain/push 再 save；remove 同样先改内存。给出不存在的父目录时，upsert 返回 Err，内存仍出现 1 条记录，磁盘没有它。remove 写失败会反向产生“内存没有、磁盘仍有”。

非权威 pending 不会直接授予信任，因此这是恢复/诊断一致性缺陷，不应夸大成准入绕过。load 把未知版本当空，再写入可覆盖该文件，是另一项前向格式保留风险。

**最小修复方案：** 遵循 Settings 的既有模式：clone 更新后的 records → 原子私有文件写入成功 → 替换内存；失败保留旧状态。未知 format 保留原文件并显式标记不可写/需要处理，或备份隔离后再建新文件，不默默销毁未来版本内容。

pending 目前只有 Rust API 查询，未被 UI/IPC 恢复流程消费。一端 active、另一端 pending 的状态可能持续到用户重新配对或撤销，**不是有界的“瞬时”状态**。原审计 S3-P1-2 的危害包括静默不同步，而原实现也已经要求双方 Confirm；把残留改述为“没有未经双方确认的信任，因此不是原危害”没有完整回答可恢复性问题。

**恢复出口方案：** 先稳定 Core 提交/撤销语义，再经现有 contracts/生成器提供有限的 pending 摘要与明确状态说明，让用户选择继续同 key 重配或遗忘。active 侧接受相同 pin 的重新配对；pending 不能自动升格、不能按 hostname 单独授权。不要通过增加有限消息声称消除两将军不确定性。ConnectionsView 大规模状态所有权重构可以遵守 PR #68 的前置条件，Core 门禁和最小恢复说明不应一起被冻结。

**必要门禁：** pending upsert/remove 落盘失败、重开读取一致、未知版本不覆写、双方 key 匹配才可恢复、取消/撤销不复活、同 key 重配清理注记、UI 不把 pending 显示为 active。

### R10 — P2：LAN 自地址与准入/拨号规则不一致，断言文案失真

**选择器已实测。** `select_local_lan_ip([fd7a:115c:a1e0::1])`、`[fe80::1]`、`[169.254.1.1]` 均返回该地址。S1-P0-1 的 `gate.asserts` 却写“绝不选 APIPA”，与生产代码明确允许 link-local 不一致。

需要区分三个事实：

1. 8.8.8.8/VPN 默认出口误采已修，up/non-p2p 过滤有效。
2. 非 p2p 不能等价于“所有虚拟/VPN 接口都被排除”；本次纯函数结果不证明具体 OS 会把某个 Tailscale 接口传入。实际网卡场景尚未执行，不能再虚构本机 VPN 端到端复现。
3. 一旦输入集合仅有上述 IPv6，选择器会给出它；peer_socket_addr 又拒绝无 scope fe80，当前共享 TCP 服务监听 IPv4 `0.0.0.0`（platform-network-server.rs:19），macOS 本机路线只要地址非空就标 connected。自地址可展示为可用，但当前直连路径未承诺支持它。

**最小修复方案：** 明确当前产品可广播/可接入的 LAN 自地址策略，复用 Tailscale 排除函数；在未实现 IPv6 listener/scope 前，只广告当前能接入的自地址，或将不支持地址显示为 unavailable。APIPA 若产品有意支持，应如实测试/描述，而非先强制删除一条合法政策。单独保留未来 scope 支持的议题，不把全面 IPv6 改造混入这个修补。

**必要门禁：** Tailscale ULA-only、fe80-only、合法其他 ULA-only、APIPA-only、只有 down/虚拟接口、无地址；选择 → 本机路线 → 广播 → listener 支持的一致性。Windows/macOS 原生 VPN/断网验收仍需分别完成。

### R11 — P2：若干门禁验证 helper，仍未锁住实际生产入口

这不是说台账脚本无用；它已经成功阻止“未执行、旧 SHA、ignored、仅编译”冒充通过。当前差距是**已执行的测试测了什么**。

| 条目 | 现有 gate 边界 | 最小补强 |
|---|---|---|
| S6-P2-6 | 直接调用 until_disconnect，断线 drop 一个 pending future | 通过真实 Unix API 发送已认证 wait_runtime_snapshot，关闭 socket，验证 handle future 结束及连接 semaphore 许可可立即再取得；移除实际命令的取消接线须变红 |
| S4-P2-1 | create_private_dir_all helper 的权限测试 | 从实际 incoming/preparation 入口创建，检查全路径权限；调用点退回裸 create_dir_all 须变红 |
| S4-P1-5 | 共享校验 helper 的计数 | 从生产 fanout + restart orchestration 发到1/2/8 peers，分别计数预验证、restart、完成校验，证明不会按 peer/retry 重读整文件；保留完成后的源变化校验 |
| S2-F5 | 满队背压/容量 | 真实 worker 文件 ACK 期间文本交付，见 R5 |
| S6-P2-4 | Core peek/ack；可见性分支 | notice helper 拒绝展示仍不能 ack，见 R6 |

建议把变异证据结构化保存为条目/源码 SHA/变异点/失败测试/恢复后通过记录；新增的是有具体回归价值的生产 seam 测试，不需要再建一个大而泛化的测试框架。`fixed_gated` 可继续表示“限定断言已过门禁”，但报告必须显式保留未被该断言认证的子目标。

### R12 — P2：GC 对 atomic 临时文件是永久豁免

`sweep_orphan_payloads` 521–525 行不管年龄都跳过 `.tmp` 等原子写入名。活跃 writer 的保护是对的，但崩溃留下的 atomic temp 永远不会进入这条 GC。expired transfer 清 `.part` 不是 payload temp 的替代品。

**最小方案：** 先明确 atomic writer 的生命周期所有权；只对已知 writer 命名格式、超过宽限且不在活跃写入集合的 temp 回收，或启动时在确认无旧 writer 后做清理。不能仅按年龄对所有 `.tmp` 广泛删除。复用现有维护任务和少量活跃 lease，不要建立第二套存储 GC 服务。

**必要门禁：** writer 写入后崩溃遗留、重启后回收；超过宽限的活跃写入仍保留；非法/未知临时名不误删；正常原子 rename 和失败重试不受清理竞态影响。若暂时延后，应在 S5-P2-1 边界写明“最终 payload 对账，不含 atomic temp”。

## 5. 轻量性与整体设计评价

| 设计 | 评价 | 应保留/调整 |
|---|---|---|
| pending 独立 sidecar | 适合兼容 `deny_unknown_fields` 的旧 Settings；避免无谓的配置迁移 | 保留；补写失败原子性、版本保留、提交/撤销及用户恢复出口 |
| 继续使用 PairingPersisted | 少协议变动、旧新端能交流，但新端实际表示 pending 持久化，语义已弱化 | 诚实描述，不将它当双端 active 证明；先防已知冲突，未来只有必要时才通过明确版本/能力协商扩展语义 |
| promote 持锁写 settings | 为本机提交一致性付出有限同步写等待，有明确原因 | 保留关键锁序；把本机 terminal commit 也纳入，不只锁住 trust 写入 |
| quota 一次物理测量 + 实际释放 | 比重复扫盘有效，比新增常驻空间账本风险低 | 保留并发准入锁；补 blocking 边界和非累积 checkpoint 预算 |
| GC 按文件身份对账 | APFS/NTFS 正确抽象，完整 live set 的保守策略合理 | 抽给全部 unlink 路径使用，避免复制两个不同的引用判据 |
| warning ID + peek/ack | 比 destructive read 正确且可扩展，生成器接线符合工程边界 | ack 需要展示回执，不能依赖 void show 调用成功 |
| notification cursor + service instance | 不需要消息总线即可恢复，足够轻 | 必须一次锁取通知窗口；是否向用户呈现 gap 要明确 |
| gate 执行记录/CI scope | 对“真实运行”的证据质量提升明显 | 保留；不要将 gate 存在/通过升级成所有生产属性已证明 |

不建议现在做无关的广泛重构：没有必要为形式上的 Map key 改写连接池、为 GC 引入新数据库、为所有待验项新增总线，也没有证据要求立刻实现完整 IPv6。需要的是将**已有正确抽象用到所有相关生产入口**，并补少量关键状态/故障边界。

## 6. 可执行改进顺序与验收门槛

### 第一批：数据保全与信任生命周期

1. R1 统一所有 payload unlink 的文件身份/存活引用判断，先保证收藏不丢。
2. R2/R3/R4 联合修配对：统一撤销用例、原子提交点、发送 completion 前的 pin 冲突预检。
3. R9 为 sidecar 写入补失败原子性及版本保留。

**完成条件：** 上述本次反例改为“坏行为不可达”的生产回归门禁；每个真实适配器接线被覆盖；Core/runtime/macOS/Windows 原生 CI + 合同/跨平台检查通过。配对改动仍不宣称消除跨设备不确定性。

### 第二批：通知与可见消费

1. R6 接入真实展示回执，cooldown/ack 重试保留 warning。
2. R7 一次锁读取通知窗口，补确定性溢出交错。
3. R11 的 Unix long-poll/semaphore 实际入口 gate。

**完成条件：** 每次 ack 均对应有效展示；不存在“元数据旧、列表新”的窗口；断开的实际长轮询释放许可。原生窗口可见性、daemon 重启体验另做记录。

### 第三批：存储阻塞与性能裁定

1. R8 短预算 post-delete checkpoint + 后续维护，保持迁移安全语义，扫盘/淘汰进 blocking 边界。
2. R5 以生产传输形态重新采 S2-F5 基线，S4-P1-4 先补本机 callback 故障注入、锁等待/持有时长测量。
3. 只有生产基线证明不达标，才实施独立认证文件 worker/连接或移出进度回调；后者需按顺序发布进度，完成事件不可被普通进度覆盖。
4. R10 LAN 选择与支持范围统一；R12 明确临时文件回收责任。

**完成条件：** 不再用调度模型代替端到端交付；活动传输阶段与全程指标分列；零丢统计核对到 ACK/终态；Windows 与 macOS 的可用路由政策一致。真实双设备数据仍独立待验。

### 第四批：证据整理与设备验收

统一 S1-P1-5 的 platform/pass_condition/partial 理由；更新性能文档中“256 MiB 单批”的不合法负载说明、S1-P0-1 APIPA 断言、FINAL-REPORT “瞬时不一致/单机全部完成”、critical-path-tests 的旧 pending 描述、INDEX 的集成 gate 展示。历史阶段报告可以保留，但应标注历史快照与当前状态。

原验收环境文档已明确 VoiceOver、最终 macOS 包/托盘走查只需要这台 Mac。这些不应和双机项目一起称“本环境不可做”。完成可本机执行项后，再用两台真实设备验收：配对中断/双方重启/同 key 重配/遗忘、文件断点/网络切换、两端 UI 状态，以及 Windows 安装/窗口行为。

PR #68 未合并确实可以阻塞既定的大规模 ConnectionsView 状态所有权整理；它**不能阻塞**本报告第一、第二批的 Core 正确性修复和门禁。合并 PR 属于另一个明确授权动作，本次审查不替用户执行。

## 7. 最终裁定

可以认可“主体修复已落地、实际 CI/门禁证据显著改善”，不能认可“所有单机整改均已充分完成”。本次新反例在现有完整测试全绿的同时成立，说明差距集中在**跨用例生命周期、提交后竞争和测试边界**，不是再跑一遍相同测试就能解决。

将第一、第二批修复及反例门禁完成后，再据生产性能证据裁定 S2-F5/S4-P1-4，并完成本机与双机验收，才具备更新整体完成/发布结论的依据。
