# TailSync 未闭环修复与外部验收执行方案（Luna Runbook）

日期：2026-09-23
已验收的产品代码基线：`2.2.2`、wire `v4`、数据库 schema `v11`、源码提交 `ba7bbd5701b176c3179bbffbd0fcbb9e061e4edf`。后续文档提交只使仓库 HEAD 前进，不代表重新构建产品。本文件第 2、4、5 节保留原始实施步骤；执行前先对照本页的状态更新和同日外部验收报告，不要重复已完成的提交。

本文件不是通过声明。当前候选仍为 **NO-GO**：Windows unsigned 包 smoke、打包后及同 SHA portable 存活期间 TCP 19889 空端口、系统级忙碌文件剪贴板读取三档已通过。隔离数据目录误探测账户旧历史的代码缺陷已由 `ba7bbd5` 修复。正式签名、干净安装生命周期、真实双/三设备、产品剪贴板监视器端到端广播、产品进程级阶段崩溃验收和 WebView2 长时内存证据仍未闭环。`ReceivedBatchCommit` core 六阶段终止/恢复矩阵和按 dispatch 共享源校验已实现并测试；Phase 0A 新增了合成历史基线和 Windows 诊断模式 IPC 计数入口，但真实 IPC/DB 锁/连接/内存曲线仍缺。2026-09-24 的 O08 代码状态见同日实现记录：Windows 74 个 Tauri 命令均使用稳定错误边界，wire v5 协商、4 MiB 文件滑窗和压缩图片分片已实现并通过本机测试；真实 N-1、跨设备和发行包验证仍未完成。

## 0. 执行纪律

每个 Luna 子任务都必须：

1. 在仓库根目录记录 `git status --short`、`git branch --show-current`、`git log -1`、时间和工具链版本。
2. 只使用 `audit/` 下按日期隔离的合成数据、临时数据目录和测试身份；不读取真实历史、剪贴板正文、文件路径、令牌或私钥。
3. 先添加最小复现或失败测试，再做最小代码修改；一个原子提交只解决一个工作包，不顺手重构其他模块。
4. 不使用 `git reset --hard`、`git clean`、`git checkout --`；不把 `SkipChecks` 或 `SkipSmokeTest` 当作验收选项。
5. 每个命令保存原始输出、退出码、开始/结束时间和日志路径；证据中的未执行项目只能写 `BLOCKED`，失败只能写 `FAIL`。
6. 任何跨平台变更完成后依次执行 Core、Runtime、Themes、Windows Rust、Windows 前端、跨平台契约检查；macOS/Swift 必须在真实 macOS 主机或 macOS CI 上执行。

## 1. 当前最高优先级阻断项

| 优先级 | 未闭环项 | 事实 | 解除条件 |
|---|---|---|---|
| P0 | Windows 干净安装 | 当前 HEAD 的 unsigned 包 smoke/深链及同 SHA portable 存活期 TCP 19889 空端口已通过；干净 VM/测试账户中的安装态未验证 | 在可销毁环境用同一 SHA 的安装包完成安装、正常退出、升级、卸载、重启、自启动注册与安装态运行时端口检查，并归档截图/日志 |
| P0 | 正式包 | 当前只有 `development`、`NotSigned` unsigned 包 | 在隔离签名环境注入 updater 私钥及 Authenticode 证书，生成并验签同一 SHA 的 community/trusted 包；日志只记录布尔存在性和公钥/产物 hash |
| P0 | 双/三设备 | 没有受控第二台/第三台设备，因此 direct、Tailscale、Iroh direct/relay、glare、N-1、睡眠唤醒都不能 PASS | 两台真实成品包设备；多 Peer/多路径再增加第三台设备，记录实际 route、RTT、吞吐和双方 SHA |
| P1 | ReceivedBatchCommit 外部验收 | `01330fa` 已实现显式状态机与 debug-only 注入；六阶段 core 子进程退出/重启矩阵已通过。产品进程、真实 `.part` 和长时维护尚未测 | 在 Windows/macOS 产品进程逐阶段注入终止，重启核对 manifest、ACK、history、`.part`、清理和幂等；第 2 节仅作原始设计参考 |
| P1 | O08 稳定错误 | Windows 74 个 Tauri 命令已使用稳定错误边界，Windows capability 为 `true`；macOS capability 仍为 `false` | 在真实 macOS 上完成命令迁移、Swift 测试和旧客户端回退验证 |
| P1 | wire v5 | Windows v5 与两项能力已实现，本机 v4/v5 回退、恶意输入及延迟吞吐测试通过；真实 N-1 和跨设备证据仍缺 | 用旧版与新版成品包完成双向互通、断线恢复、长文件/图片及多路径验收 |
| P1 | 忙碌剪贴板/PDF | 独立 Win32 owner 已验证 40/200/500ms 各 20 次文件读取及 900ms 类型化 `Busy`；产品广播链路和 WebView2 长时采样仍缺 | 在隔离产品会话观察无文本路径广播，并采样 100/500 页 PDF 的 RSS/page-count 曲线 |

## 2. ReceivedBatchCommit 最小实现方案（先 12a，再 12b）

**状态更新：** 12a/12b 的 core 实现及六阶段注入矩阵已经完成并通过；以下是设计记录。下一执行任务是产品进程级终止、重启、`.part` 与维护队列验收，不要重新实现状态枚举。

目标文件：`shared/rust-core/src/sync/batches.rs`、`shared/rust-core/src/sync/resume.rs`、`shared/rust-core/src/sync.rs`、`windows/src-tauri/src/sync_adapter.rs`、`macos/src-tauri/src/sync_adapter.rs`。

### 2.1 12a：只引入状态和事务边界

新增 `ReceivedBatchCommitState`（建议以 sidecar manifest 字段保存，保持 schema v11）：

```text
Receiving -> HashVerified -> ClipboardStaged -> HistoryPersisted
          -> ReceiptPersisted -> Acked -> Cleaned
```

规则必须集中在一个 `transition_commit_state` 函数，禁止平台 adapter 自行跳跃：

- `Receiving -> HashVerified`：所有文件完整写入并通过 BLAKE3；hash 不符时删除该文件和断点，不推进状态。
- `HashVerified -> ClipboardStaged`：将待提交文件放入 `clipboard-files/` 临时 staging，原子写入并 flush；`incoming/` 与 staging 不在同一物理卷时使用有界流式复制，不能盲目 `rename`。
- `ClipboardStaged -> HistoryPersisted`：单个 SQLite 事务写入完整批次；事务提交前不得激活系统剪贴板。
- `HistoryPersisted -> ReceiptPersisted`：以 `(source_device_id, batch_id, manifest_hash)` 唯一键写 durable receipt；重复执行返回已有 receipt，不重复历史行。
- `ReceiptPersisted -> Acked`：只有 receipt 成功后网络层才发送 batch ACK；ACK 丢失时允许按 receipt 重发。
- `Acked -> Cleaned`：删除 manifest、`.part`、断点 sidecar；清理失败保留可维护队列，不回滚已确认历史。

新批次抢占时，`local_generation` 较旧的批次只能归档，不得反向激活系统剪贴板。所有恢复路径先读 manifest 状态，再决定撤销 staging、重发 ACK 或清理。

### 2.2 测试专用阶段终止注入

不要把终止开关放进正式发行包。新增 test-only 注入点（例如 `cfg(any(test, feature = "acceptance-injection"))`），由环境变量 `TAILSYNC_RECEIVED_BATCH_ABORT_STAGE` 选择 `hash_verified`、`clipboard_staged`、`history_persisted`、`receipt_persisted`、`acked`、`cleaned`；命中后以固定退出码 `70` 终止子进程。默认关闭，发布构建拒绝该 feature。

为每个阶段写一个最小 harness：

1. 创建隔离 `TAILSYNC_DATA_DIR`，写入合成小文件和唯一 batch id。
2. 启动接收子进程，等待日志中的阶段事件（日志只含 batch hash 前缀，不含文件名/正文）。
3. 终止进程并重新启动同一数据目录。
4. 断言：manifest 状态、历史行数、receipt 数、ACK 次数、`.part`/staging/最终文件清理顺序和剪贴板激活次数。
5. 重复提交同一 manifest，断言历史仍为一份、ACK 可重发、不会覆盖更新一代剪贴板。

运行命令（在实现后才可执行）：

```powershell
cargo test --manifest-path shared/rust-core/Cargo.toml sync::tests -- --test-threads=1
cargo test --manifest-path shared/rust-core/Cargo.toml --features acceptance-injection --test received_batch_crash_matrix -- --test-threads=1
```

只有六个阶段全部通过，并在 Windows 与 macOS 产品进程各完成一次，才能把该项从 `BLOCKED` 改为 `PASS`。

### 2.3 12b：只在 12a 绿灯后消除重复 I/O

把已验证的 plaintext staging 句柄/路径作为不可变 `VerifiedStaging` 传给 history adapter；禁止再次从远端或原始 `.part` 解密。保留接收端最终 hash 校验。新增对比测试确认 12b 不改变状态跃迁、ACK 时序和跨卷 fallback。

## 3. O08 稳定错误契约

目标文件：`shared/tailsync-runtime/src/contracts.rs`、`shared/schema/local-contract.schema.json`、`shared/schema/fixtures/local-contracts.json`、`windows/src/types/localContracts.generated.ts`、macOS 生成 Swift model 和所有 `ApiError`/Tauri command 入口。

实现顺序：

1. 定义版本化 `StableErrorEnvelope { schema_version, code, retryable, message_key, detail_class }`；`code` 使用固定枚举，`detail_class` 只能是长度/计数等脱敏值，禁止路径、正文和令牌。
2. 为每个命令建立 code 表：参数错误、未找到、暂时忙、存储不可用、未授权、协议不兼容、内部错误；明确是否可重试及 UI 本地化 key。
3. 让 Rust adapter 同时支持旧文本和 envelope：客户端 capability 未声明时返回旧文本；双方 capability 都为 true 时返回 envelope。
4. 生成并提交 TypeScript/Swift 解码器和有效、缺字段、未知 code、错误类型 fixture；未知 code 必须安全映射为 `internal_error`，不能崩溃。
5. 全部命令迁移并通过跨平台检查后，才把 `supports_stable_errors` 改为 `true`；wire v4/schema v11 不因本地 IPC envelope 改变。

门禁：`node scripts/check-local-contracts.mjs --root .`、Windows 前端测试、macOS Swift tests、真实旧客户端 fallback；任何一个命令仍返回未分类文本时保持 `false`。

## 4. 按 dispatch 共享源校验（Commit 14）

**状态更新：** `69489aa` 已完成一次 dispatch 共享校验，并以 1、2、8 Peer 回归测试覆盖；以下为原始实施步骤。真实多 Peer 大文件/重复哈希性能仍待第三设备。

当前 `shared/platform-clipboard-transfer.rs::send_batch_to_peer` 在每个 peer 内调用 `revalidate_prepared_file`，多 Peer 会重复读取和 hash。改为：

1. `prepare_file_batch` 完成后，在 `deliver_prepared_batch_to_peers` 前对每个文件执行一次 metadata + BLAKE3 校验，生成不可变 `ValidatedPreparedFileBatch`。
2. 将校验结果和大小/hash 传给每个 peer；每个 peer 仍必须重新打开文件，打开失败分类为 `SourceUnavailable` 或可重试 I/O。
3. 为避免校验后源文件被修改，优先创建隔离 immutable staging；若不做 staging，发送端必须在每个 peer 完成后做 bounded post-send size/hash 检查，发现变化则取消未完成 peers 并从新快照重试。
4. 接收端始终保留完整 hash 校验，不能因为发送端共享结果而放宽安全边界。

添加 1、2、8 peer 的计时/读取次数测试，以及源文件在校验后变化、消失、重复 hash 的测试。只有在正确性不变且重复 hash 明显下降后提交。

## 5. Wire v5 细粒度能力与后续能力

**状态更新（2026-09-24）：** Windows 已实现 wire v5 会话协商、4 MiB 滑窗与压缩图片分片，本机回退、畸形输入和 80/150 ms 模拟延迟测试通过。以下为原始实施门禁；真实 N-1 成品包及多设备验收仍待执行。

当前发行基线继续锁定 v4。先实现 capability foundation，再实现两个独立能力：

1. 在已认证会话的 hello/negotiation 中加入 `CapabilitySet`：`file_sliding_window`、`image_compressed_chunks`；未知能力按不支持处理。
2. 协商结果绑定连接/session epoch，断线或重新认证后重新协商；不得跨连接缓存。
3. 每项能力有编译期/运行期 kill switch；任何一项关闭都回退现有 stop-and-wait/raw RGBA v4 语义。
4. 先写 v4↔v4、v4↔v5、v5↔v5 的协议 fixture，再写 malformed capability、伪造尺寸、解压炸弹、乱序/重复 chunk 测试。
5. `file_sliding_window` 使用 4–8 MiB 有界窗口、累计 ACK/选择性重传和背压；延迟 80/150ms 下与 stop-and-wait 做同素材吞吐对比。
6. `image_compressed_chunks` 限制压缩后/解压后大小、像素乘积和分片数量；解码失败必须关闭当前能力并返回类型化协议错误，不能降级为路径文本。
7. 两项能力分别灰度开启；没有真实 N-1 证据前不得修改默认能力或把 wire 版本标成 v5。

## 6. Windows 安装包与签名验收

### 6.1 清理旧进程后重跑

当前没有已知占用进程；每次重跑前仍须确认设备所有者已正常退出任何 TailSync 实例，不要由验收脚本强杀未知用户进程。确认：

```powershell
Get-Process -Name tailsync -ErrorAction SilentlyContinue
Get-NetTCPConnection -LocalPort 19889 -ErrorAction SilentlyContinue
Get-NetTCPConnection -LocalPort 19890 -ErrorAction SilentlyContinue
```

三个结果均为空后，重新运行不带 skip 参数的 `package-windows.ps1`，并使用全新 `TAILSYNC_DATA_DIR`。smoke 必须证明主实例存活、第二次深链 handoff、退出后无残留端口。

### 6.2 干净 VM/账户矩阵

在可销毁 Windows VM 或全新管理员账户：安装 unsigned community 本地包，记录安装路径、注册表 autostart、进程树和端口；启动/正常 UI 退出；安装 N-1 后升级到当前；重启后检查恢复；卸载后确认程序、autostart 和端口清除。若做正式 release，再对同一 SHA 的 signed package 重复矩阵。历史数据必须来自测试账户，不能触碰日常账户。

## 7. Windows 忙碌剪贴板 40/200/500ms

**状态更新：** `23bb3c9` 已加入仅用于测试的独立隐藏窗口 Win32 clipboard-owner helper 和显式 opt-in 忽略测试。2026-09-23 在用户允许覆盖当前账户剪贴板后，以合成 `CF_HDROP` 运行 40/200/500ms 各 20 次，均返回文件类型并发生多次尝试；900ms 返回类型化 `Busy`。原始日志、失败夹具的历史记录与统计见 `audit/frozen-spec-clipboard-2026-09-23/run-20260923-120000/`。这不证明产品剪贴板监视器不会广播文本路径，后者仍需隔离产品会话。

复跑时先取得用户对当前剪贴板覆盖的明确许可，设置 `TAILSYNC_ALLOW_CLIPBOARD_OVERWRITE=1` 和独立 `TAILSYNC_CLIPBOARD_TEST_DIR`，运行：

```powershell
cargo test --locked --manifest-path windows/src-tauri/Cargo.toml --lib clipboard_file::tests::busy_file_clipboard_system_acceptance -- --ignored --exact --nocapture --test-threads=1
```

以下保留原始验收规则，供新环境复测与产品端到端检查：

验收断言：

- 发生占用时走有界重试，不立即降级为文本路径；
- 成功时接收类型仍为文件；
- 超过预算时返回 `ClipboardFileReadError` 等类型化错误，UI 可重试；
- 不记录真实路径和剪贴板正文。

运行 Windows Rust tests、helper integration tests、`npm test -- --run` 和完整 package smoke；三档任一失败均为 `FAIL`。

## 8. 双设备/三设备真实路径矩阵

每一场景记录双方产品版本、完整 SHA、OS/CPU、wire/schema、真实 route（LAN/Tailscale/Iroh direct/Iroh relay）、连接耗时、握手 RTT、吞吐、匿名 batch/message id 和结果。不得用测试名替代实际 route。

必须逐项执行：

1. 文本、图片、单/多文件双向复制；大文件和相同 hash 重复发送。
2. TCP 同时发起配对至少 100 次；确认只保留一个 session，`failed_attempts` 不增加。
3. 配对超时、错误验证码、断线、恢复、睡眠、唤醒；确认退避可被新任务打断。
4. 旧 N-1 与当前互通，验证 v4 fallback 和升级后数据保留。
5. 物理剪贴板写入 receipt：用户随后复制相同文本/图片必须传播，TailSync 自己的写入不能回音。
6. LAN、Tailscale、Iroh direct、强制 relay 各测连接时间、RTT、吞吐；路径不可达时必须有界失败。
7. 第三设备加入后测多 Peer 并发、不同大小文件和重复 hash 去重。

证据不足的组合保持 `BLOCKED`，不能用同 host probe 替代。

## 9. 崩溃维护与 PDF 长时验收

### 9.1 崩溃维护

使用第 2 节的 test-only killpoint，对每一阶段执行 5 次重复；重启后检查 manifest、ACK、历史唯一性、staging/`.part`、过期维护队列。将 `cleanup_expired_transfers` 的 30 分钟周期缩短只允许在测试配置中做，不能修改生产间隔来“加速通过”。

### 9.2 WebView2

在隔离产品进程打开合成 100 页和 500 页 PDF，连续翻页、缩放、跳页至少 15 分钟。每秒记录 WebView2 子进程 PID、RSS、页面序列、当前页和导航错误；稳定窗口后比较线性回归斜率。确认旧 `PDFPageProxy.cleanup()` 在页面替换/文档切换/卸载触发，缩放不重复请求 operator list。任何只有 jsdom/PDF fixture 的结果都不能写成长时 PASS。

## 10. 最终 Exit Gate

Luna 只有在以下条件全部满足时才可把结论改为 **GO**：同一完整 HEAD、同一签名产物和 build manifest；Windows smoke/19889 为空；clean VM 安装/升级/卸载/重启/autostart；Swift/macOS 构建绿；两台设备四种路径及第三设备多 Peer 证据；N-1；100 次 glare；busy clipboard 三档；ReceivedBatchCommit 全阶段 crash/restart；WebView2 RSS 稳定；所有日志、截图、SHA 和人工/设备限制归档到 `audit/external-acceptance-YYYY-MM-DD/`。

在此之前，报告必须继续写 `NO-GO`，并对每个未完成项目写 `FAIL` 或 `BLOCKED`，绝不能用单元测试、静态检查或同 host 互操作替代真实验收。
