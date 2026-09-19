# TailSync 系统设计与工程流程优化方案

- 编制日期：2026-09-05
- 状态：建议实施方案；本文完成不代表优化已实施或验收通过。
- 实现基线：codex/reliability-hardening，HEAD fd92576，包含编制时已有的未提交修改。
- 产品基线：2.2.2；设备间线协议 v4；数据库 schema v11。
- 适用范围：共享 Rust、macOS SwiftUI/daemon、Windows React/Tauri、本地通信、依赖、CI、打包与更新。
- 本轮交付：方案与文档索引。应用代码、依赖、流水线和远端规则均未在本轮修改。

## 1. 优化目标与实施原则

本轮优化的核心目标，是让应用行为、资源管理和验证规则有明确归属，降低两端长期维护成本，并提高测试结果对实际发货产品的代表性。

优先完成以下结果：

1. 每次发布都能证明同一提交通过必需检查，并能追溯到最终产物。
2. History/Storage 的阻塞执行、排队、变更通知由共享运行时统一管理。
3. 相同发现与探测输入在两端产生一致的 Peer 刷新语义。
4. 本地通信有可验证的类型、错误、取消和资源释放契约。
5. 核心测试与产品构建使用的公共运行时依赖差异可见、可解释。
6. 增加真实程序入口、真实双系统及跨版本验收，覆盖现有 probe 不能覆盖的链路。

### 1.1 继续沿用的技术选择

| 技术或设计 | 后续定位 |
|---|---|
| Rust + Tokio | 网络、加密、存储与共享运行时的主实现 |
| SwiftUI/AppKit | macOS 窗口、菜单栏、交互及系统能力接入 |
| React/TypeScript/Tauri 2 | Windows 界面、窗口管理和本地调用 |
| SQLite + 加密文件容器 | 本地历史及批次事务；先优化执行方式和锁范围 |
| TCP + Iroh QUIC，LAN/Tailscale 发现与选路 | 保留多路径能力，统一应用策略和验收 |
| Noise、固定设备身份、现有 updater 信任锚 | 沿用现有安全模型 |
| 独立 Core/protocol/classifier/themes crate | 保留已有领域分工 |
| 三个 Cargo workspace | 第一阶段保留，先治理公共依赖解析差异 |
| macOS 独立预览窗口、Windows 原始预览响应 | 延续 ADR-002 的生命周期和内容安全策略 |

macOS 正常 daemon 使用 headless 路径；更新操作由短生命周期 Tauri helper 执行。不能仅因 Cargo.toml 依赖 Tauri，就推断正常 daemon 长期运行 WebView。是否进一步分拆 updater 依赖，应先测构建时间、产物组成和实际进程成本，单独立项。

### 1.2 必须保持的产品不变量

- 文件收藏、取消收藏和收藏删除仍以 batch_id 对应的逻辑条目原子执行。
- 普通历史删除、清空与配额清理继续保护收藏。
- 文件批次只在文件历史事务和持久接收回执成功后发送最终 ACK。
- 续传仍根据 .part 实际长度和校验结果恢复；保留期维持 24 小时。
- 配对继续使用固定身份、Noise、验证码和双端确认；模式切换不能绕过信任校验。
- 预览保持 64 MiB 内容上限、私有临时目录、既有 SVG/Markdown 策略。
- 长按保持 220 ms 宽限期 + 420 ms 充能，以及点击/双击抑制。
- schema v11、现有 JSON/Tauri 命令与 wire v4 在基础重构中保持兼容。
- 测试和验收使用独立数据目录、测试身份及可销毁环境。

这些约束来自 [领域上下文](../CONTEXT.md)、[ADR-001](adr/ADR-001-peer-directory-delivery.md)、[ADR-002](adr/ADR-002-independent-history-preview-window.md)、[ADR-003](adr/ADR-003-history-favorites.md) 和 [续传规格](features/resumable-file-transfer.md)。

## 2. 当前证据与问题分级

优先级说明：P1 表示首批应解决的交付或可靠性问题；P2 表示随后收敛的架构和性能问题；P3 表示满足触发条件后再做的结构调整。这里的优先级不等于漏洞等级。

| 编号 | 当前证据 | 判断 | 优先级 |
|---|---|---|---|
| F01 | Release 由 tag 独立触发，完整 CI 未进入发布依赖图 | 存在完整验收与发布脱节的流程缺口 | P1 |
| F02 | History 异步调用持 DB 锁执行同步查询；搜索逐条解密文本 | 阻塞路径确定；真实延迟和影响范围待测 | P1 |
| F03 | Windows TCP API 的 JoinSet 仅在退出时回收，macOS 定时回收 | 重复本地 TCP 请求会积累完成任务记录；不等同于所有 Windows UI 调用泄漏 | P1，修复范围小 |
| F04 | Peer 刷新对 mode/generation、remembered 候选的处理不同 | 应用策略尚未统一；不能将所有平台差异都判为错误 | P2 |
| F05 | Core 测试与产品锁解析到不同 rustls、tokio-util 版本 | 测试代表性下降；尚无证据直接判为漏洞 | P1/P2 |
| F06 | Swift 手工字典解码、TS 手写类型、命令正则检查并存 | 契约维护分散，缺少完整行为验证 | P2 |
| F07 | interop 在同一 host 上运行两份当前代码的 loopback probe | 能验证部分协议；不能代替双系统、跨版本和真实落盘验收 | P1/P2 |
| F08 | macOS 大预览走 Base64/JSON；Task 取消未传到底层 socket | 序列化开销确定；收益与峰值内存待实测 | P2 |
| F09 | canonical include 通过父模块隐式导入依赖；Storage 实例与全局根混用 | 独立理解和测试成本高；适合后续定向治理 | P3 |

源码入口与编制时行号：

| 证据 | 文件 |
|---|---|
| F01：tag 触发、needs 关系 | [release.yml](../.github/workflows/release.yml)，3、58、119、206 行；[ci.yml](../.github/workflows/ci.yml)，3 行 |
| F02：异步命令内同步查询 | [Windows history](../windows/src-tauri/src/commands/history.rs)，54–64 行；[macOS history](../macos/src-tauri/src/api/routes/history.rs)，31–42、71–82 行 |
| F02：关键词扫描解密 | [queries.rs](../shared/rust-core/src/db/queries.rs)，163–240 行 |
| F02：调用者管理锁和 revision | [runtime history](../shared/tailsync-runtime/src/history.rs)，56–61、112–142 行 |
| F03：请求任务回收 | [Windows transport](../windows/src-tauri/src/api/transport.rs)，14、44、83 行；[macOS transport](../macos/src-tauri/src/api/transport.rs)，92–106 行 |
| F04：刷新语义 | [macOS peer_cache](../macos/src-tauri/src/network/peer_cache.rs)，107–122、137–170 行；[Windows peer_cache](../windows/src-tauri/src/network/peer_cache.rs)，33–39、92–110、141–182 行 |
| F05：公共依赖解析 | [根 Cargo.lock](../Cargo.lock)、[macOS Cargo.lock](../macos/src-tauri/Cargo.lock)、[Windows Cargo.lock](../windows/src-tauri/Cargo.lock) |
| F06：源码命令提取 | [跨平台检查](../windows/scripts/check_cross_platform_sync.mjs)，328–338 行 |
| F07：同 host、loopback | [interop 脚本](../windows/scripts/test_cross_project_interop.ps1)，56、108–117 行 |
| F08：大响应与 socket 请求 | [Swift preview](../macos/swift-ui/Sources/TailSync/Services/ApiClientHistory.swift)，22–69 行；[Swift transport](../macos/swift-ui/Sources/TailSync/Services/ApiClientTransport.swift)，80–173 行 |
| F09：共享 include 与全局存储 | [共享 pool](../shared/platform-network-pool.rs)，1 行；[storage](../shared/rust-core/src/db/storage.rs)、[paths](../shared/rust-core/src/db/paths.rs) |

编制前的同一会话曾只读查询远端：rulesets 为空、environments 数量为 0、main protection 返回 Branch not protected。这是当时快照，实施 O02 前必须重新查询，不能把它当作长期不变的仓库设置。

已经完成的修复不应重新立项：部分网络/更新实现已 canonical include；文件续传已有持久回执和批次约束；文本/图片入站历史写入已有部分 spawn_blocking 隔离；发布已有 publisher 串行化、签名验包和指定 tag 的 feed 校验。

## 3. 目标架构与职责分配

### 3.1 目标依赖关系

以下是逐步迁移的目标，不是对当前代码完成度的描述。

~~~mermaid
flowchart TB
    Mac["SwiftUI / AppKit"] --> UDS["macOS Unix socket Adapter"]
    Win["React / TypeScript"] --> Tauri["Windows Tauri Adapter"]
    UDS --> Runtime["tailsync-runtime：应用用例、执行与生命周期"]
    Tauri --> Runtime
    Runtime --> Core["tailsync-core：领域规则、事务、同步状态机"]
    Core --> Protocol["tailsync-protocol"]
    Core --> Classifier["history-classifier"]
    Core --> Themes["tailsync-themes"]
    Runtime --> Ports["显式系统能力 Interface"]
    Platform["平台 Adapter：剪贴板、发现、探测、通知"] -. 实现 .-> Ports
~~~

已有 Core 内网络执行代码不为“图上整齐”而一次性搬迁。先在新增用例上形成清晰职责，再按实际变更热点迁移。

### 3.2 Module 归属

| Module | 对调用者提供的能力 | 负责的 Implementation | 留给 Adapter 的事项 |
|---|---|---|---|
| History | 分页、搜索、收藏、删除、预览准备 | 执行排队、取消、Core 调用、提交结果、revision 发布 | 参数解码、响应编码、窗口动作 |
| Storage | 存储状态、配额操作、迁移协调 | 路径归属、阻塞工作、活动传输协调、失败回滚 | 目录选择、平台通知 |
| Peer refresh | 请求刷新、读取快照、等待对应轮次 | mode/epoch、候选合并、探测计划、失败回退、完成语义 | mDNS/Tailscale/UDP I/O |
| Local contract | 类型化请求、响应、错误与能力声明 | schema、生成、兼容 fixtures | UDS/Tauri 编码差异 |
| Task lifecycle | 接收、运行、取消、回收请求 | 有界任务集合、关闭行为、统计 | TCP/Unix accept 与鉴权 |
| Preview transport | 有界元数据与二进制内容 | 长度检查、请求标识、取消、数据分片传输 | AppKit/WebView2/PDFKit 等渲染 |
| Verification | 对一个固定提交执行检查并输出证据 | 必需检查集合、失败汇总、产物清单 | 平台构建工具和签名能力 |

Interface 指调用者必须了解的完整使用契约，包含顺序、取消、错误和资源限制。目标是增加 Depth：让这些事实集中在 Module 内部，而不是只给现有代码增加转发层。

### 3.3 建议目录演进

以下新增路径均为计划项；先按单个用例建文件，达到独立职责后再拆子目录。

~~~text
shared/
  tailsync-runtime/src/
    lib.rs
    history.rs                 逐步承担异步应用用例
    execution.rs               有界阻塞执行与取消
    peer_refresh.rs            轮次与模式编排
    runtime_events.rs          提交结果与 revision 发布
    contracts/                 本地 DTO、错误、能力声明
  rust-core/src/
    db/                        同步事务与加密存储规则
    peer/                      Directory/Health/Delivery 规则
    sync/                      批次、恢复、发送 journal
  schema/
    local-contract.schema.json 建议新增
  local-contract-fixtures/     建议新增

scripts/
  check-shared-resolution.mjs  建议新增：依赖图比较
  check-local-contracts.mjs    建议新增：生成与行为契约入口

.github/workflows/
  verify.yml                  建议新增：workflow_call
  ci.yml                      CI 触发与 verify 调用
  release.yml                 验收、签名构建、验包、发布
~~~

本地 DTO 初期放在 tailsync-runtime；仅当 Swift/TS 生成器需要独立编译且带来可测收益时，再考虑独立轻量 crate。设备间线协议继续留在 tailsync-protocol，避免把本地 UI 变化和 wire v4 绑定。

## 4. 分阶段工作包

### 4.1 任务总表

估算以一名熟悉项目的工程师为基准，包含实现、针对性测试和修订；不包含等待设备、签名账号或 CI 排队的时间。O00 完成后重新估算。

| ID | 任务 | 依赖 | 估算人日 | 风险 |
|---|---|---|---:|---|
| O00 | 冻结可复现基线与证据 | 无 | 1–2 | 低 |
| O01 | Windows 请求任务回收 | O00 | 0.5–1 | 低 |
| O02 | 统一验收与发布依赖图 | O00 | 2–4 | 中 |
| O03 | 三锁公共依赖解析门禁 | O00 | 2–3 | 中 |
| O04 | 应用路径性能与生命周期基线 | O00 | 1–2 | 低 |
| O05 | History 共享执行与提交通知 | O04 | 3–5 | 中 |
| O06 | 搜索/预览缩短持锁与合作式取消 | O05 | 3–5 | 中高 |
| O07 | Peer refresh 编排共享化 | O00、O04 | 4–6 | 中高 |
| O08 | 本地类型与错误契约 | O05；Peer 部分接 O07 | 3–5 | 中 |
| O09 | 真实产品与跨版本验收 | O02、O03；框架可提前准备 | 4–7 | 中 |
| O10 | macOS 预览二进制与取消 | O06、O08 | 2–4 | 中 |
| O11 | canonical include 显式化 | O05、O07 | 3–5，可选 | 中 |
| O12 | Storage 根目录实例化 | O05、O09 | 2–4，可选 | 中高 |

基础范围 O00–O10 约 26–44 人日；O11–O12 约增加 5–9 人日。它们是任务拆分预算，不是未经验证的交付承诺。

### 4.2 阶段 A：先让现状可验证

完成 O00–O04。退出条件：

- 当前未提交工作形成可复现快照；每个测试结果能对应源码与工具链。
- 发布工作流有明确的必需检查依赖。
- 公共依赖差异被消除或有针对性例外。
- 请求任务回收问题有回归测试。
- 后续性能优化具有统一测量方法。

### 4.3 阶段 B：收敛应用用例

完成 O05–O08。退出条件：

- 两端 History 用例使用共享执行策略。
- 耗时查询与预览可测量、可取消；写事务语义完整。
- Peer 刷新模式、轮次和候选处理策略一致。
- 已迁移本地用例拥有共同 DTO/错误 fixtures，平台只负责编码与 I/O。

### 4.4 阶段 C：验证交付并优化大数据通信

完成 O09–O10。退出条件：

- 真实 Windows/macOS 程序完成配对、文本、图片、文件与恢复矩阵。
- 至少一个可追溯旧版本参与兼容或明确拒绝测试。
- 更新安装有实际产物证据。
- 预览优化通过性能和取消验收。

O11–O12 按维护收益择机推进，不作为阶段 A、B 全部工作的前置条件。

## 5. O00：冻结基线和证据

### 工作步骤

1. 记录 HEAD、分支、工具链、三个 Cargo.lock、两份 package-lock 及工作区差异。
2. 当前有大量未提交实现，基线必须包含这些文件；只记录 HEAD 无法复现现状。
3. 实施前通过经审阅的提交或独立快照保存相关工作，不执行清理、重置或批量覆盖。
4. 为测试创建独立数据目录、临时 socket/端口和测试身份，记录环境隔离方式。
5. 运行现有完整平台门禁，区分“静态”“单测”“host 编译”“原生打包”“真实设备”五类结果。
6. 使用 `node scripts/capture-optimization-baseline.mjs --root . --output artifacts/optimization/<commit-or-snapshot>/<run-id>/baseline.json` 建立证据目录，输出提交、工具链、锁文件摘要和验证分类；脚本不读取剪贴板正文、能力令牌或私钥。

### 完成标准

任一失败都能由另一名开发者使用同一快照和命令重现。源码未提交时，清单必须包含补丁及相关新增文件的摘要，不能引用此前 commit 的 CI 成功来代表当前快照。

### 回滚

O00 只建立验证材料；不涉及应用数据和产品行为。

## 6. O01：请求生命周期修复

### 涉及文件

- windows/src-tauri/src/api/transport.rs
- macos/src-tauri/src/api/transport.rs，作为行为参照
- 两端对应 transport 测试

### 工作步骤

1. 在 Windows accept 循环中增加完成任务回收，可使用非空 JoinSet 的 join_next 分支或有界周期回收。
2. 保留连接 semaphore；区分“当前并发连接数”与“尚未回收的完成任务数”。
3. 记录任务异常并保持原有拒绝、鉴权和关闭行为。
4. 先修这一处，不把 TCP/Unix Listener 统一重写作为前提。

### 回归场景

- 连续完成 10,000 次小请求，停止发送后任务集合回到空或仅保留真实在途任务。
- 无鉴权、格式错误、超时请求均释放 permit 和任务记录。
- shutdown 到来时，接收停止、已完成任务回收、超时任务按原策略结束。
- 测试绑定临时 loopback 端口，不连接正在运行的用户实例。

### 验收与回滚

任务集合不随历史请求数单调增长；原有响应契约不变。可单独回退此提交，回退不涉及数据格式。

Tokio 的 JoinSet 通过 join_next/try_join_next 取出完成任务；不能用连接 semaphore 替代任务回收。[官方文档](https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html)

## 7. O02：统一 CI 与发布验收

### 目标流程

~~~mermaid
flowchart LR
    Trigger["PR / main / release tag"] --> Verify["固定提交的统一 verify"]
    Verify --> Gate["汇总必需检查：全部成功"]
    Gate --> Build["平台 release 构建与签名"]
    Build --> Smoke["最终包烟测、升级与签名验证"]
    Smoke --> Publish["串行发布与 feed 校验"]
~~~

### 具体调整

1. 新增可复用 verify workflow，完整列出当前 frontend、共享 crate、平台 crate、Swift、契约、脚本、production-features 和 advisory 检查。
2. ci.yml 与 release.yml 调用相同实现；通过仓库内相对路径引用，使工作流定义与调用提交一致。
3. Release 从 tag 解析并固定提交，构建、检查和证据中的 SHA 必须一致；不取“main 最近一次绿灯”代替。
4. 新增必需检查汇总。矩阵中任一必需 job 失败、取消或意外跳过，汇总均失败；有意跳过的平台无关项必须在集合定义中明确。
5. 普通验证不接收签名密钥。平台签名构建等待 verify 成功，最终 publish 仅在其 job 取得内容写权限。
6. 保留现有 release-publisher 全局串行化、版本校验、签名验包和指定 tag 的 feed 验证。
7. 打包脚本可保留开发者自验入口；workflow 复用时消除无意义的重复运行。Swift SVG 渲染开关必须保留到唯一有效测试入口。
8. 将代码 SHA、锁摘要、编译目标、工具链、构建配置、验证 run ID、最终产物 hash 写入证据清单。
9. 在最终签名产物上再做烟测；重新构建出的正式包不能只引用早先未签名包的测试结果。
10. 按仓库权限和实际协作方式配置 main/tag ruleset。先验证规则不会锁死合法发布，再启用强制规则；本方案本身不修改远端设置。

GitHub reusable workflow 使用 workflow_call，仓库内调用可绑定调用者所在提交。该机制适合集中维护检查集合。[GitHub 官方说明](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)

### 验收实验

在隔离测试仓库或安全的 release rehearsal 中分别制造：

- tailsync-runtime 单测失败；
- interop 失败；
- advisory 检查失败；
- 必需平台 job 被取消；
- 上传产物所属 SHA 与待发布 SHA 不一致；
- 最终包签名或 hash 不匹配。

上述情况签名后续阶段或发布阶段必须按依赖关系停止，不能产生正式更新 feed。成功场景保存完整证据链。

### 回滚

先以并行比对方式验证新旧检查集合，再切换发布依赖。切换失败时暂停发布并恢复已验证工作流；不通过移除必需检查临时“修绿”。远端规则变更保存原配置，必要时仅回退本次新增规则。

## 8. O03：公共依赖解析一致性

### 当前差异

| 依赖 | 根 workspace | macOS | Windows |
|---|---:|---:|---:|
| rustls | 0.23.43 | 0.23.42 | 0.23.42 |
| tokio-util | 0.7.19 | 0.7.18 | 0.7.18 |

当前版本为编制时结果；实施时重跑依赖图，不盲目升级到编制时或届时最新版本。

### 检查设计

1. 用 Cargo 解析结果建立图，比较 tailsync-core / tailsync-runtime 的 normal+build 依赖闭包。
2. 以包名、版本、来源和目标平台为比较键；支持同名包的多个版本，不能只取 lockfile 首次匹配。
3. 区分 macOS arm64、macOS x86_64、Windows x86_64 的正常差异；在同一 target 下比较根测试上下文与对应产品上下文。
4. 记录 features 的来源。Core 单测上下文与 release 的 dev feature 差异必须单独标识；平台 feature 合并不能通过“只看版本相同”掩盖。
5. 对公共非平台依赖默认要求相同解析；有意例外必须注明目标、理由、负责人或维护角色、复查触发条件。
6. 错误输出包含两边版本、来源及最短引入链。
7. 先用现有 update-cargo-locks.sh 做针对性三锁更新，再执行原生检查和网络相关验收。
8. 将图比较接入 verify；保持每日 advisory 检查。

### 验收

只变更根 lock 的 rustls，门禁应失败并指出 Iroh 引入链。平台专属库不应产生误报。构造同名多版本和仅 feature 差异 fixture，检查不能把它们误判为一致。

### 回滚

同一变更中的相关 manifest 和 lock 必须整体回退。若旧版本已不可接受，采用向前修复；不能为通过一致性检查而全局屏蔽相关依赖。

## 9. O04–O06：History/Storage 运行时与性能

### 9.1 先建立测量

为以下阶段添加时间与计数观测：

- 请求进入、排队、取得执行许可；
- DB 锁等待、锁持有、SQL、解密、文件读取；
- 事务提交、revision 发布、响应编码；
- 取消发出、底层停止、资源释放；
- 同期心跳与 ACK 延迟。

记录请求类别、匿名关联 ID、大小区间和耗时，不记录剪贴板正文、预览内容、能力令牌或私钥。使用现有 tracing/observability 扩展，避免另起一套日志体系。

### 9.2 O05：有界执行与提交通知

第一步不改变数据库格式和查询语义：

1. History 用例返回异步结果；平台调用者不再决定是否 spawn_blocking。
2. 在提交阻塞任务前取得有界许可，避免大量 blocking task 同时等待同一个数据库锁。
3. 阻塞闭包内再取得数据库访问权；不把持有的异步 guard 跨线程搬运。
4. 错误转成用例层类型，暂由旧 Adapter 映射回兼容响应。
5. 成功数据库 mutation 返回已影响的条目及变更结果；由共享用例统一发布 revision。
6. 数据库提交完成后，即使原客户端已经取消，也必须发布相应变更，不能依赖调用者收到结果才通知其他窗口。
7. 只读请求在开始前可取消；已开始写事务按原子提交/回滚规则结束，客户端取消不等于数据操作已撤销。
8. 迁移 get_history/page、favorite、delete、clear，再迁移 restore 的数据准备；实际剪贴板写入仍交给 Adapter，并明确该步骤失败的返回语义。

Tokio 的 spawn_blocking 一旦开始执行，不能靠 abort 强制停止；取消需要排队阶段拦截或 Implementation 合作检查。[官方说明](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)

这一阶段主要保护异步 worker，不承诺消除单数据库长操作的排队。

### 9.3 O06：缩短重操作持锁范围

在测量证明的热点上执行：

1. 搜索先按稳定顺序取有界候选片段；解密和关键词计算尽量移到 DB 锁外。
2. 候选片段大小按测量选择，起始评估 128/256 条；不能固定大批量后宣称性能改善。
3. 内部扫描使用稳定游标或查询视图，明确数据变更时的处理。外部已有 offset/limit 语义不能悄然改变。
4. 采用有界快照或可重试 revision 策略，定义重复/缺失条目的处理；禁止在高频写入下无限重试。
5. 预览先读取并校验元数据，取得稳定的加密内容引用或受控文件句柄，再在锁外做耗时读取与解密。
6. 对“读取期间删除/迁移文件”建立明确生命周期契约：保证读取有效，或返回类型化的过期/不可用错误；不能返回拼接或截断内容。
7. 查询与解密片段之间检查取消。写事务、批次回执和配额提交继续保持原子性。
8. 对存储目录扫描、配额预留等剩余阻塞路径采用同样归属；缓存统计时明确失效规则，配额硬检查不能只信过期缓存。
9. 索引调整先通过 EXPLAIN QUERY PLAN 证明收益。全文搜索保持现有加密目标，明文 FTS 不在本轮范围。

### 9.4 必测场景

- 无匹配全文搜索与连续输入新关键词；
- 搜索期间新增、删除、收藏、清空历史；
- 预览期间删除条目或迁移存储；
- 文件接收与历史检索同时发生；
- 写事务提交后客户端关闭；
- 执行队列饱和、取消、关闭和错误传播；
- favorites 窗口与 history 窗口同时存在；
- 迁移保存设置失败后的回滚。

### 9.5 回滚与停损

按用例分批迁移，禁止让新旧 writer 同时对同一数据库执行双写。若缩短锁范围改变分页/删除语义，保留 O05 执行隔离，单独回退 O06。专用 DB worker 仅在有界执行和缩锁后仍有实测瓶颈时再决策。

## 10. O07：Peer refresh 共享编排

### 目标行为

- 刷新请求绑定连接模式与刷新 epoch；调用者知道等待的是哪一轮结果。
- 旧模式完成不能被新模式刷新当作成功。
- remembered Route 必须经过真实探测或认证 Session 才获得在线状态。
- 发现失败时，可以保留同模式最后可用候选，但候选缓存和健康证据分别处理。
- 认证 Session 的有效性继续优先于普通探测结果。

### 实施步骤

1. 收集双方现有测试，形成差异表，逐项确定产品语义。
2. 在共享运行时建立 PeerRefresh Module，持有轮次、mode、缓存上下文、取消信号与完成结果。
3. 将发现、地址探测、时间来源与快照通知作为显式 Seam；两端 Adapter 提供真实 I/O，测试 Adapter 提供确定性输入。
4. 合并流程固定为：读取一致设置快照 → 发现/同模式回退 → Directory 合并记忆 → 建立探测计划 → Health 更新 → 预连接 → 发布带 epoch 的结果。
5. 预连接与探测有各自时间预算，预连接慢不能无限阻塞“发现刷新已完成”；在结果中区分发现/健康完成与后台连接继续。
6. 模式变更递增 epoch，旧轮次即使 I/O 无法立即取消，也不能覆盖当前快照、缓存或完成信号。
7. 保留 Iroh 常驻 Endpoint 复用和 RTT 的现有生命周期。
8. 每次接入一端并跑相同 fixtures，双方一致后删除重复编排。

### 测试矩阵

| 场景 | 必须观察到的结果 |
|---|---|
| A 轮未完成，切换 B | A 结果不覆盖 B；等待 B 的请求不被 A 唤醒为成功 |
| discovery 完全失败 | 按同模式回退规则处理；不直接恢复过期 online |
| 仅 remembered Peer | 进入探测/认证流程，不凭记忆标 online |
| 首次探测从未成功 | 保留 discovered 等产品定义状态 |
| 连续失联两轮 | 按现有 Health 语义转 offline |
| 存在认证 Session | 保持 connected，使用 session latency 优先规则 |
| 唤醒和缓存清空 | 旧轮次隔离，新轮次可完成 |
| 用户停用/取消信任设备 | 预连接和待投递不能继续使用失效信任 |
| 多次手动刷新 | 请求合并规则明确，等待队列有界 |

### 回滚

先通过回放模式比较快照，避免两套实现同时探测和建立连接。切换正式实现后保留单一活动 worker。必要时按一端回退，但继续保留统一行为 fixtures。此项是完成 ADR-001 的未决工作；语义确定后再补充 ADR，不提前把建议记为“已接受”。

## 11. O08：本地通信契约

### 迁移顺序

History → RuntimeSnapshot → Peer → 稳定错误 → 能力声明。Settings 继续复用当前 schema 机制，避免同一模型出现两套生成源。

### 实施步骤

1. 用 Rust 的类型化 DTO 定义本地请求、响应和错误。Core 已有合适类型的复用或组合，不按平台再复制。
2. 为 Swift 生成 Codable 类型，为 TypeScript 生成声明和必要的运行时校验；两者来自相同 schema 或导出模型。
3. 区分字段缺失、null、默认值、枚举未知值、数字越界及错误类型。
4. 制定整数规则：当前 JS 接收的数字必须满足 safe integer 检查；需要完整 u64 时，用新增版本契约中的十进制字符串，不静默转换旧字段。
5. 传输 Adapter 负责旧字段名、命令别名及旧错误文本映射；用户提示在 UI 层本地化，稳定错误 code 不依赖提示文本。
6. 逐项替换 Swift Any 字典读取及手写 TS 类型，保留兼容 decoder 的 fixtures。
7. 由同一命令注册源生成 dispatcher 或其测试清单，覆盖真实 handles/handle 分发和 Tauri 注册，而不仅检查字符串出现在源码。
8. 新增 local capabilities 或版本声明用于兼容协商；名称与字段在实现前记录，不与 wire v4 混用。

### fixtures 内容

- 正常请求和响应、错误响应；
- 缺少必需字段、未知字段、null、未知枚举；
- 空列表、完整文件批次、受保护收藏；
- 旧响应增加新可选字段；
- u64/Int64 边界、JS safe integer 边界；
- 取消、重试和不可重试错误；
- 真 dispatcher 调用成功及未知命令拒绝。

### 完成标准

生成检查无差异；Rust 输出可由 Swift/TS fixtures 解码；两种 Adapter 对同一用例返回相同语义。现有客户调用形状在兼容范围内保持稳定。

### 回滚

按用例保留旧编码 Adapter，切换使用静态配置或构建选择，避免线上双写。schema 和 wire 变化需要单独版本化；基础 DTO 重构不顺带修改数据库版本。

## 12. O09：真实产品与跨版本验收

### 12.1 分层证据

| 层 | 执行内容 | 能证明什么 |
|---|---|---|
| L0 | Core/协议属性测试 | 规则、编码、状态机不变量 |
| L1 | 共享运行时 + 测试 Adapter | 用例排序、错误、并发、取消 |
| L2 | 同 host interop probe | 两个项目解析下的协议与握手交互 |
| L3 | 当前平台真实应用/daemon | 实际接线、存储、剪贴板、关闭与恢复 |
| L4 | macOS ↔ Windows 真机或受控桌面 VM | 原生双系统、网络路径、系统权限 |
| L5 | 最终安装包与更新路径 | 安装、升级、签名包、数据保留、发布产物 |

L0–L2 全绿不能在报告中写成 L4/L5 已通过。自动化不足的 L4/L5 先使用可复现、保存证据的发布检查，再逐步自动化。

### 12.2 推荐先实现的产品测试

1. 用专用测试身份启动两端真实程序，完成配对，再经系统剪贴板写入文本和图片。
2. 验证对端系统剪贴板内容、历史条目及来源；不能只检查进程存活。
3. 经真实文件入口传输批次，验证落盘内容 hash、逻辑批次数、接收回执和发送 journal 完成状态。
4. 在测试环境中终止发送端、接收端、双方先后终止，再启动并验证恢复。
5. 注入 ACK 丢失或断线，不通过私自改用户 journal 构造场景。
6. 验证系统深链接到主实例的实际配对状态变化，不能只断言第二进程退出。
7. 在可销毁 Windows 环境安装 NSIS；在 macOS 环境安装完整 bundle，分别从旧版本升级。
8. 测试辅助能力保持 test-only 或外部 harness，生产构建不得暴露绕过鉴权的测试入口。

### 12.3 最低跨版本与路径矩阵

| 组合 | LAN | Tailscale | Iroh direct | Iroh relay |
|---|---|---|---|---|
| 当前 macOS → 当前 Windows | 每个候选版本必测 | 网络相关发布必测 | 网络相关发布必测 | 网络相关发布必测 |
| 当前 Windows → 当前 macOS | 每个候选版本必测 | 同上 | 同上 | 同上 |
| 当前 ↔ 上一可验证版本 | 同协议要求兼容；异协议要求明确拒绝 | 网络依赖变更时补测 | Iroh 变更时补测 | Iroh 变更时补测 |

N−1 必须对应可追溯的旧二进制、tag 或构建快照。若不存在上一公开版本，应使用最近可验证快照并如实标注，不能生成一个“旧版本”标签代表当前代码。

路径必须从运行结果确认；测试名叫 relay 不代表流量确实走了 relay。确定性故障测试使用受控 Adapter，公网连通验收单独标记，避免把外部服务暂时不可达误归因于状态机。

### 12.4 故障矩阵

| 故障 | 期望结果 |
|---|---|
| 配对窗口过期/指纹不符 | 明确失败，未建立信任 |
| 接收中断、重启 | 从实际 .part 校验位置恢复 |
| 最终 ACK 丢失 | 重试后只形成一个完整逻辑批次 |
| 数据库提交失败 | 不提前最终 ACK；保留安全重试状态 |
| 磁盘满/存储拔出 | 返回可识别错误，恢复后按规则继续 |
| 源文件修改 | 拒绝拼接不同内容，要求重新复制 |
| 收藏与清空并发 | 收藏保护及批次原子性保持 |
| 升级后重启 | 历史、收藏、身份和设置保持；协议状态明确 |
| 两条认证连接同时重放同一事件 | 先验证是否发生重复应用，再决定去重修复 |

最后一项目前是静态风险核验项：可靠事件的 seen 检查与 record 之间存在异步副作用。应先用屏障构造重现，若确认再增加按稳定身份与 message_id 的处理中状态。不能未经重现就宣称已发生用户故障，也不能把文件批次已有幂等机制误判为缺失。

### 12.5 证据清单

每次保存：双方产品/wire/schema 版本、完整 SHA、锁摘要、OS/CPU、真实 Route、场景 ID、匿名事件或批次 ID、期望/实际结果、内容 hash、退出状态、日志和产物 hash。测试输出不包含用户真实剪贴板内容。

## 13. O10：macOS 二进制预览与取消

### 目标

减少 Base64/JSON 的大对象分配，保证快速切换预览时旧请求能停止占用资源。继续使用现有私有 Unix socket 的 PID/能力令牌约束。

### 协议草案

复用 Windows TSPV 信封设计经验，但独立标识本地 transport 能力：

~~~text
请求：经过认证的 JSON 控制消息，包含新能力/命令与 request_id
响应：magic | local_version | metadata_length | metadata_json | payload
~~~

该格式为建议草案。正式实现必须定义大小端、失败响应格式、字段长度、总长度上限和 EOF 语义。metadata 应携带请求关联、条目身份、内容类型与精确 payload 长度；不把来自不同请求的响应混合解析。

Windows 已通过 tauri::ipc::Response 返回原始字节，其路径可以作为先例。[Tauri 官方说明](https://v2.tauri.app/develop/calling-rust/)

### 实施步骤

1. 先建立旧 JSON 路径的 1/16/64 MiB 测量。
2. 增加显式能力声明与新命令；旧命令保持兼容。
3. 请求仍使用现有鉴权。只对声明支持新协议的 daemon 使用新路径。
4. Swift 先验证 header 和元数据，再有界读取 payload；拒绝长度溢出、超限、截断和不一致内容。
5. 二进制内容分段读取，但先沿用当前有界完整 payload 渲染，不声称这一步实现了所有格式流式渲染。
6. 取消绑定 request_id 和 socket 生命周期，由单一连接所有者串行执行 close/shutdown，防止文件描述符复用竞态。
7. 服务端感知断开后停止尚未开始的工作；已经运行的解密按 O06 的合作式取消结束。
8. 限制单窗口活动预览与后台待完成请求，替换条目时优先取消旧请求。
9. 仅在明确“不支持新命令/能力”时回退 JSON；鉴权失败、协议损坏或超限不能触发盲目回退重试。
10. 回退时遵守旧响应上限，持续保留 generation 防止过期结果覆盖。

### 验收

64 MiB 边界及超限输入；连续切换至少 50 次大预览；关闭窗口取消；畸形 header；服务端提前断开；读取期间文件删除；旧 daemon 的能力缺失。比较整段进程树的增量 RSS，包含 Swift、daemon 和实际参与的渲染进程。

### 回滚

通过能力声明关闭新路径，保留旧命令。回滚不改变存储内容和设备间 wire 协议。达到兼容退役条件后再另行删除旧响应实现。

## 14. O11–O12：后续结构治理

### 14.1 canonical include 显式化

触发条件：某个 include 的新功能需修改多个父模块导入，或独立编译/测试长期需要完整平台 crate。

执行方式：

1. 列出该 include 使用的父作用域符号、全局状态、平台分支和副作用。
2. 优先使用已有的 SyncPlatform、ConnectionAdapter 等 Interface，避免重复抽象。
3. 把应用编排迁入 tailsync-runtime；纯领域规则留 Core，真正平台能力留 Adapter。
4. 每次迁移一个可独立验收的用例；删除相应隐式依赖与重复测试。
5. 同步更新漂移检查，使其验证共享导出/契约，而不是强迫平台专属文件形状相同。

完成标准是行为、依赖和测试更集中，不以文件变短或 crate 数增加为成绩。

### 14.2 Storage 根目录实例化

触发条件：并发测试需要全局串行锁，或迁移/多实例场景已被列为产品需求。

执行方式：

1. 把历史连接、加密文件目录、配额统计和迁移上下文绑定同一 Storage 实例。
2. 默认根目录只在应用组装时选择。
3. 迁移通过明确的排空、切换与回滚流程进行，保留已有维护锁和活动传输限制。
4. journal、incoming、history 路径按依赖顺序接入，禁止半数 Module 读取旧全局根、另一半使用新实例。
5. 同一测试进程建立两个隔离实例，迁移一方不能改变另一方路径、配额或结果。

回滚需整体恢复同一组路径依赖；不将数据库 schema 迁移混入这一项。

## 15. 性能基线、指标与验收方法

以下数值是建议的起始预算，尚未被当前机器实测证明。O04 需要记录硬件与基线后冻结预算；调整必须附数据和理由。正确性门禁始终是硬要求。

### 15.1 数据集与方法

| 数据集 | 用途 |
|---|---|
| 1,000 / 10,000 / 50,000 条加密历史 | 小规模、常用规模和压力规模；固定随机种子 |
| 文本平均 1 KiB，含少量大文本 | 匹配/不匹配搜索，避免只测短字符串 |
| 1 / 16 / 64 MiB 预览 | 覆盖序列化开销与内容上限 |
| 1 MiB 小文件、256 MiB 大文件、20 文件且总计不超 1 GiB 的批次 | 小任务、大任务和批次恢复 |
| 10,000 次请求与 30 分钟混合运行 | 任务回收、资源趋势和长期稳定性 |

分别记录冷启动与预热结果。相同硬件、相同构建配置、相同数据集至少跑 5 轮；延迟统计收集足够请求样本再报告 p95/p99，吞吐报告每轮结果及中位数。公网路径不作为纯本地性能回归基线。

### 15.2 建议预算

| 指标 | 建议目标或门禁 |
|---|---|
| 请求任务回收 | 请求结束后的一个回收周期内移除完成记录；不随总请求数增长 |
| 混合负载的本地轻量响应 | p95 ≤ 100 ms、p99 ≤ 250 ms，基线确认后冻结 |
| 排队只读任务取消 | 100 ms 内从可运行队列失效 |
| 已开始搜索的合作式取消 | 200 ms 内观察取消，特殊 I/O 阻塞单独记录 |
| 10,000 条无匹配搜索 | 初始目标 p95 ≤ 1 s；50,000 条先记录压力曲线 |
| 文件吞吐 | 相同 LAN 基线中位数回退不超过 5%，超出需定位解释 |
| ACK 延迟 | 混合负载相对基线不恶化超过 10%；区分控制 ACK 与持久提交 ACK |
| 64 MiB 预览增量峰值 RSS | O10 目标较旧路径降低至少 20%，以进程树总量衡量 |
| 预览切换/关闭 | 不展示旧条目；取消后不继续积累旧请求；不能只测 UI generation |
| 正确性 | 无额外逻辑批次、无受保护收藏丢失、无提前完成 ACK |

若基线本来已很好，应以“不回退和复杂度收益”评估，不为追求百分比引入高风险改写。若性能目标未达成，报告实际数字与下一步定位，不能用构建成功替代。

## 16. 验证命令与运行要求

以下为现有命令入口示例，在仓库根目录执行；完整结果和外部环境限制记录在 [`OPTIMIZATION-IMPLEMENTATION-STATUS-2026-09-06.md`](OPTIMIZATION-IMPLEMENTATION-STATUS-2026-09-06.md)。O03/O08 检查已接入 verify；真实双系统、签名包和性能命令仍按门禁条件执行。

### 16.1 静态、版本与契约

~~~bash
git diff --check
node scripts/validate-release-version.mjs --tag v2.2.2
node scripts/bump-version.mjs --root . --target 2.2.2 --check
node shared/schema/generate-settings.mjs --check
node scripts/check-production-features.mjs .
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
~~~

版本变化时用实际目标 tag/版本替换 2.2.2。

### 16.2 共享 Rust

~~~bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
~~~

根 workspace 检查不覆盖被 exclude 的两个平台 crate。不要把单测需要的 test-support 作为生产默认 feature。

### 16.3 macOS

~~~bash
bash macos/scripts/check_macos_sources.sh windows
TAILSYNC_WEB_SVG_RENDER_TESTS=1 swift test --package-path macos/swift-ui
bash scripts/check-windows-host.sh
~~~

最后一个命令仅验证 Windows 项目在 macOS host 的编译路径，不能代替 Windows 原生编译或运行。合并 workflow 后避免重复执行同一集合，同时保留实际 renderer 测试。

### 16.4 Windows 原生与前端

~~~powershell
cargo clippy --locked --manifest-path windows/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path windows/src-tauri/Cargo.toml --lib
./windows/scripts/test_cross_project_interop.ps1 -WinRoot ./windows -MacRoot ./macos
./windows/scripts/package-windows.ps1
~~~

在 windows 目录执行 npm ci、npm run lint、npm test、npm run build；涉及网站改动时，在 site 目录执行 npm ci、npm run lint、npm run build。

### 16.5 advisory 与真实运行

advisory 检查沿用 rustsec.yml 的三 manifest 矩阵和例外登记检查。实际打包、安装、升级、剪贴板操作及进程终止只在明确的测试环境执行；先检查运行中的 TailSync、固定端口和数据目录，避免把用户应用当作实验对象。

## 17. 合并、回滚与 Go/No-Go

### 17.1 每个变更的完成定义

- 问题证据、代码变化、行为测试能逐一对应。
- 明确本次修改的 Module 和不变量，不混入无关依赖升级。
- 相关平台的契约、编译、测试及真实 I/O 检查完成。
- 性能相关变更附相同基线下的测量结果。
- 必需检查无意外 skip；外部环境阻塞明确标记为“未完成”。
- 回滚不会删除用户数据、重置身份或覆盖其他未提交工作。
- 领域决策形成后更新 CONTEXT/ADR；用户行为变化更新对应 feature 文档。
- 仅提交该工作包涉及文件，原有用户改动单独保留。

### 17.2 阶段性 Go/No-Go

| 节点 | Go 条件 | No-Go 条件 |
|---|---|---|
| A 完成 | 发布依赖完整、公共依赖解释清楚、基线可复现 | 用旧 CI 代表新快照；跳过关键检查继续发布 |
| B 完成 | History 与 Peer 共享用例有一致行为和取消/失败测试 | 收藏/ACK/刷新模式语义改变但无明确决策 |
| C 完成 | 原生双系统与最终包证据齐全，性能有前后对比 | 仅 probe 成功就宣布真实设备验收通过 |
| 公开发布 | 同一 SHA、最终包、版本矩阵和更新链路可追溯 | 丢失数据、错误 ACK、兼容行为不明、签名/hash 不一致 |

### 17.3 回滚边界

- 代码回滚：按工作包 revert，保留证据和针对性回归。
- 本地通信：能力协商保留兼容路径；损坏/鉴权错误不作为降级理由。
- 数据层：本轮默认不引入新 schema；确需迁移时另开设计和旧库 fixture。
- 已发布更新：考虑现有降级保护，优先发布更高版本的修复包；不能默认用户可直接降级。
- 存储迁移：保存原有失败回滚能力，不通过删除旧目录实现“回滚完成”。

## 18. 推荐的首批提交顺序

1. O00：保存实施基线与验证分类。
2. O01：修复 Windows TCP 请求任务回收并加针对性回归。
3. O02：提取统一 verify，先证明检查集合等价，再接入 release。
4. O03：加入依赖解析门禁，使用独立提交对齐当前公共依赖。
5. O04：添加测量并保存初始数据。
6. O05：以一个 History 写用例和一个读用例贯穿两端，验证执行与通知归属，再扩大迁移。
7. O07：完成模式绑定的 Peer 刷新测试与共享实现。
8. O06/O08：分别推进缩锁与本地契约，不放在一个大提交里。
9. O09：完成真实产品和跨版本验收。
10. O10：用可比较数据验证二进制预览收益。

首批工作优先让后续变更更容易验证，再进行行为敏感的编排重构。O11/O12 根据实测维护成本决定是否进入下一轮。

## 19. 本方案的验证状态

编制所依据的前序审查已通过：

- 跨平台契约检查，覆盖 59 个 Swift API 命令及相关模型；
- Settings 生成检查；
- 2.2.2 产品/版本文件一致性检查；
- tailsync-protocol 的 18 项测试；
- tailsync-runtime 的 2 项测试；
- 工作区 diff 格式检查。

这些结果不覆盖完整原生打包、真实双机互联、性能压测或正式更新安装。当前实现与门禁状态见 [`OPTIMIZATION-IMPLEMENTATION-STATUS-2026-09-06.md`](OPTIMIZATION-IMPLEMENTATION-STATUS-2026-09-06.md)；O00 的基线脚本已提供，但每次实施快照仍需按实际运行生成证据。
