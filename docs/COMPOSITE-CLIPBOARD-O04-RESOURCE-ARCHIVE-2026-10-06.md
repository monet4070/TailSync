# O04 公开图片资源归档与位置绑定

日期：2026-10-06，Asia/Shanghai。状态：**LOCAL_COMPLETE（本机实现与下述自动／接缝部分）**。用户授权一个 DeepSeek/high 子代理完成下一阶段，主代理等待全部交接后独立审查、二次补修并复验；本阶段结束后停止，不进入 O05。

唯一工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，分支 `codex/composite-clipboard`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。输入源码 `8ea8cb276d20195d6e62e03259ea89045e5b6c7083af8f158da68120f9414ad2`，783 文件，与前轮源码封存一致。

执行范围依据：[逐项方案 O04](COMPOSITE-CLIPBOARD-IMPROVEMENT-PLAN-2026-10-06.md#o04来源端资源归档与位置绑定)、A13/A14 及 ADR-004/005。

## 1. 本轮范围与验收

来源端的明确用户动作取得当前历史对应 HTML 节点中的允许公开 HTTP(S) 图片，绑定到具体字节位置，提交加密资源与归档版本，随后通过现有 O06 计划自包含恢复。优先复用已有字节和内嵌资源；URL 只表示获取位置，内容摘要表示资源身份。

| 必须验证的边界 | 本轮退出门 |
|---|---|
| 原件与位置 | 双图的不同颜色、顺序与具体 img/src 节点对应；原 HTML/CNA1、capture/content identity、旧 hash 保持 |
| 持久与恢复 | 归档后 URL 失效、DB 关闭重开、离线时计划仍物化双图；没有当前 clipboard 写入或自动发送 |
| 显式联网 | 仅来源端用户动作进入 resolver；采集、预览、接收与完整内嵌内容不新增网络读取 |
| 网络边界 | 协议、user-info、DNS、每跳重定向、实际连接 IP、代理、编码、数量/大小/并发/截止均受门限制 |
| 提交与迟到 | 完整 HistoryIdentity CAS；删除、取消、版本变化和迁移竞态不复活或覆盖不相干内容 |
| 失败与生命周期 | 额度/SQL 写拒绝后原件和清单保持；新外部载荷无孤儿，收藏与已有 GC 保持 |
| 兼容性 | 旧 v2 Manifest 摘要、协议预算、恢复资源闭合门、原件域和旧 retry/hash 回归通过 |

公开静态 PNG 是当前便携模型能直接验证的首批网络图片范围；实际实现支持范围须由交接与复验填写。登录/blob、私有文件、来源上下文适配仍由 O05 处理。CSS/srcset/SVG/未知对象若未取得完整闭合证明仍 withheld。

## 2. 委派与主代理复核分工

唯一子代理 `01a11129-e27c-7830-a1fd-2df12642df33`，`cline-pass/deepseek-v4.1-flash` / high。兼容开关实读为 true；首先准确回显任务 ID 与唯一绝对目录，再通过同一代理递送完整明文任务。

子代理修改产品/测试及必要生成契约。主代理先保存完整输入源码与历史 seal 校验，准备独立网络/迟到/恢复复查，待其完成全部写入后冻结交接增量，再改产品和运行最终门禁。双方写入范围在交接前分开；主代理独立证据写入 `artifacts/clipboard-improvement/2026-10-06-o04-resource-archive/review/`。

冷构建使用单一 `target/o04-shared`，关闭 Rust debug info 与增量缓存，限制四个编译任务，避免重新生成多份平台缓存。

## 3. 交接审查与二次补修

子代理完整交接后关闭，源码冻结为 `e891c0373c7f92c0f5a3c1fee34258d437c71e54a6faadbfa04545e94ba070c5`，784 文件、相对输入 30 文件改变。主代理独立探针在该交接源码上得到 **0 PASS／8 FAIL**，没有边审边让子代理继续写入。

| 独立确认的问题 | 二次修复与回归 |
|---|---|
| mapped／reserved IPv6 被放行（两项） | 保守公开地址集合；混合 DNS 全拒绝、结果数上限、单 endpoint pin、实际 remote_addr 精确相等 |
| 重复或未闭合 img/src 仍可下载 | 复用现有 HTML inventory，拒绝歧义／未闭合属性，不另建宽松 parser |
| PNG 字节配 text/html 仍归档 | MIME、encoding、length 与实际 stream 大小门；测试直接调用私有 pinned transport，不开放生产 loopback 绕过 |
| pending transport 绕过整体 deadline | 队列／DNS／body 均受 deadline 和取消约束；10 秒单请求、30 秒资源阶段、45 秒 Runtime 操作 |
| 资源闭合擦除 MissingFormats | 缺格式事实不可升级；报告额外核对 HTML inventory，不凭资源数假称完整 |
| 再次归档从 base 重下载／增版本 | 从已有层继续；0 新资源无写入，partial retry 只下载尚缺图片 |
| 取消仍阻塞 DB-lock 等待 | cancellable admission／worker DB lock；payload 准备后事务前再查 token；事务开始后原子提交 |

主代理进一步补修：

- 新资源语义明确使用 **Manifest 3** 与 `portable-cbp1+resource-archive`。`archived_info` 是尾部可选字段；旧 v2 序列化／digest 不变，v1／v2 不允许新 role，旧 reader 对版本 3 fail-closed。
- commit/read 均证明派生只替换原精确 img/src URL，其他表示／原资源不变、旧 bindings 精确移位、新资源闭合；不能用正确 identity 提交另一篇内容。
- SQL CAS 包含完整 version／capture_id／content_id／dedupe_key／archive_revision／archive_digest，并检查原加密 manifest 的 data CAS。触发器模拟三类事务内变更，全部回滚；新 layer 不以 trim 驱逐原记录／收藏，pending guard 负责 SQL 失败后的孤儿清理。
- Runtime 下载中关闭重开 DB 后，authority 失效，迟到提交返回 archive_superseded；另一次正常 copy 不误取消归档。不以 degraded RAM 成功冒充持久成功。
- 真正文件 DB＋CNA1 往返发现 `get_composite_archive` 只接受版本 2，修为 2／3，保护 CNA1、data_hash、capture/content/dedupe、source 与 favorite。
- 正式列表／缩略图使用派生 metadata 与 owned payload worker，不在 DB prepare 锁内解密／解码大 layer。
- report 生成契约贯穿 Rust／schema／Swift／TS／decoder，保持 35 DTO 一致；Windows Tauri 返回 typed report，不使用松散 JSON 类型。
- Swift 原 3 秒 timeout 改为 60 秒；真实 socket 回归发现 request_id 缺失，补入 opt-in 断连取消。两端说明联网并按原图片索引显示失败；Swift task 随 session 取消，Windows 隔离重开 session 的迟到提示／刷新。
- Clippy 六项规范错误直接修正，不新增 allow／ignore／skip，不提高产品预算。

### 3.1 实际生产路径

Mac 菜单 → authenticated socket／request_id → history route；Windows 菜单 → generated decoder／Tauri command。两端进入 `HistoryOperations::archive_entry_images`：job admission → cancellable DB prepare（identity＋authority＋已有层或 base）→ 无 DB 锁的 public resolver → cancellable DB commit／authority 校验／完整 CAS → typed report → 本地列表刷新。

只有显式用户动作进入此 resolver，采集／预览／接收没有新增自动联网。归档不写 clipboard、不增加 intent generation、不自动同步。后续明确恢复通过现有 RestorePlan 内嵌 PNG 到原 HTML 节点；CNA1 原件保留，不借其不完整 HTML 假称闭合。

### 3.2 支持范围与失败边界

- 仅保守公开 HTTP(S) **静态 PNG**；不携 cookies／Authorization，不用代理，不自动重定向／解压。每跳重新校验 URL、DNS、pin 与 remote_addr；不是浏览器登录态抓取器。
- 8 个待获取引用，8 MiB／资源、16 MiB 总读取额度、3 redirect，最多两个网络任务和两个 Runtime 作业。未知读取失败保守扣本次允许额度，坏图解码不能绕过总量；默认上限只能收紧。
- 同请求内按字节摘要去重，不用 URL 当身份。已绑定／完整内嵌内容不下载；新捕获可从同 URL 得到新字节，已归档历史不作为“刷新线上图片”功能。
- entity／反斜线／控制字符 URL 保守拒绝，不猜测 `&amp;` 地址；JPEG／GIF／WebP／SVG、CSS URL／srcset／对象、blob／file／登录态尚不支持联网归档。保留原件与 withheld，O05 未开始。
- partial failures 只有稳定码和原 img 索引，不返回 query／凭据／响应正文。complete 表示便携文稿闭合，不证明未知原生语义、目标 App 成功或 degraded RAM 已持久化。
- schema 13、HistoryIdentity 1、CNA1 1、网络／helper 版本不变。旧 reader 对 Manifest 3 fail-closed；回滚先备份完整库与外部载荷，不静默丢层。旧 v2 记录仍按原门读取。

### 3.3 不覆盖的失败记录

子代理 Python 源码改写、raw Cargo＋tail 丢失 exit、并行 budget 冲突与探索性过滤如实保留，不认证最终源码。主代理保留 core-v1 的 fixture 编译错误、core-v2 的 TLS provider／favorite 删除 fixture 错误、Swift-v1 缺 request_id、end-to-end-v1/v2 的 probe API／隔离 root 错误、v3 的真实 CNA1 reader 版本错误、Clippy 首轮六项 lint、runtime-v3 的 probe API 可见性编译错误。重跑使用新 ID，最终只采用同一冻结源码的新矩阵。

## 4. 最终验收状态

最终源码 `d1ffeaf807db1df5159bd3b16a2a46cf94f768c3523eb50a2de56a7a3b0c2f86`，786 文件；输入→最终 45 文件改变，交接→二次最终 27 文件改变。**23 条最终命令全部 exit 0，前后同源码**，canonical runner／argv／最低命中／raw log hash／既有 skip 与 warning 数已独立复算，`verified-final-v1.json` 无 failures。各套测试存在重叠，不合计唯一测试数。

| 最终矩阵组 | 实际结果 |
|---|---|
| Protocol | 51 PASS |
| Core enabled 全 lib | 692 PASS，4 既有 ignored |
| Core integration | 14 PASS |
| Core DB all-features | 158 PASS，2 既有 ignored |
| Runtime 全组 | 51 PASS，含 6 项资源操作回归 |
| 独立八项 RED 对应探针 | 8 PASS，0 ignored；交接源码原始 8 FAIL 保留 |
| 文件 DB／公开 HTTPS／私有 native board | 2 PASS，0 ignored |
| macOS Rust | 118 PASS，3 既有 ignored |
| Windows Rust（macOS host） | 127 PASS，2 既有 ignored；既有 Win32／InvalidData dead-code warning 1 条 |
| React History／生成 decoder | 58 PASS；tsc PASS |
| 完整 Swift 包 | 267 PASS，4 既有 skipped；资源反馈与实际延迟 socket／request_id 回归命中 |
| Policy 负向／生成检查 | 5 PASS；policy／contracts／canonical／features／visibility PASS |
| workspace Clippy／三份 fmt／diff | 全部 PASS；未增加 allow／ignore／skip 或放宽预算 |

Rust／Swift／Vitest 用 `scripts/run-isolated-tests.mjs`；Rust 套件串行，单一共享 target、debug info／incremental 关闭。agent 早期并行 budget 冲突没有通过跳过测试隐藏，仍是已披露的独立限制。

矩阵后补跑默认 feature 的完整 Core lib：**679 PASS，4 既有 ignored，0 filtered**，exit 0 且同源码；文档完成后另跑 diff-check PASS。两条补充记录单独保存，不改写已冻结 23 命令矩阵。最终共享 target 约 3.7 GiB，本轮证据约 17 MiB，不再生成多份平台 target。

### 4.1 持久与本机原生接缝

两个最终探针都经过实际文件 DB、完整 identity、原 CNA1、收藏、关闭重开、离线 RestorePlan、由当前源码新编译的 helper、**私有命名粘贴板**发布／回读；不触碰 general clipboard。双颜色 fixture 检查位置与次序，public HTTPS 使用生产 Reqwest resolver 取得两 URL 相同字节并去重。最终目录：

- `review/smoke-colours-2102-1791294508377353000/`
- `review/smoke-public-https-2102-1791294508464806000/`

两个 `result.json` 均确认原 hash／identity／CNA1／favorite 不变、reopen 后 0 网络 no-op 与 private-board roundtrip。public 样本为 `raw.githubusercontent.com/monet4070/TailSync/main/assets/tailsync-icon.png` 与同内容 query 变体；不是 curl HEAD 或 DNS-only 结果冒充生产读取。所有失败目录保留。

helper 源码 SHA256 `a7d82341cbd9c3ab1e0817d7fb35a004f56bc1ddb62826252a4d10d8005f6e50`，本轮编译 binary SHA256 `6f96925dfbf0cbea95e82841cc9a9d9fdf7a8301c4639219fb1603c5de7c31a4`。本轮未改 helper 源码；与最终 inventory 对照并单独封存，不用旧 App 包认证新源码。

### 4.2 判定与保护

**O04 本机实现及以上自动／接缝部分 LOCAL_COMPLETE。** A13/A14 的本机位置、去重、资源持久与 offline 计划、生命周期自动部分 PASS；完整 GUI、实际 Word／QQ 等来源和目标 App、Windows 原生、Mac↔Windows／双设备、hosted CI 仍 **UNKNOWN**。合成 board 发布不等于目标 App 真实粘贴，额度拒绝／SQL 注入不等于真实 ENOSPC 设备故障证明；Release **NO-GO**。

12 份历史 seal、11,193 文件复算 **0 mismatch**，仅沿用 S0 既有 `core/Cargo.toml.txt` 映射。当前源码／本轮文档／所有命令和失败日志／独立 probes／私有板回读证据以本轮 `evidence-sha256.json` 封存。保留前序脏改动、日常 App、用户库和既有包，不 stage／commit／reset／clean、不自动发布；下一阶段需用户再授权。
