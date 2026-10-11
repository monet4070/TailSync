# O05 浏览器来源上下文增强

日期：2026-10-06，Asia/Shanghai。状态：O05 可选文件桥本机部分 LOCAL_COMPLETE。DeepSeek/high 已完整交接并关闭；主代理独立证实六项缺陷、二次修复并通过 29 条同源码最终门禁。完整 A15 仍 UNKNOWN，Release NO-GO。

唯一工作树 `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，分支 `codex/composite-clipboard`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。输入源码 `d1ffeaf807db1df5159bd3b16a2a46cf94f768c3523eb50a2de56a7a3b0c2f86`，786 文件。13 份历史 seal／12,226 文件复算一致，O04 证据不改写。

## 1. 范围和执行分工

下一阶段为逐项方案 O05／A15：在明确授权下取得来源浏览器当前页面实际可访问的同源登录态或 blob 静态 PNG 字节，补入指定历史的原 img/src 节点；保留原 HTML／CNA1、内容身份、收藏，不默认联网或自动写入／同步。QQ 私有资源、其他图片编码、O07／O09 与反馈 F01–F06 不在本轮范围。

唯一子代理 `01a11192-14ae-77d2-b429-f6d7e10a06ce`／`cline-pass/deepseek-v4.1-flash`／high。实际兼容开关 true，明文任务 ID／唯一目录／范围准确回执后递送完整任务。主代理先封存基线、准备独立授权／隐私／迟到结果探针；子代理只改产品与 focused tests／extension 文档。等完整交接并关闭后才二次修改产品。

## 2. 约束与实际交付路径

实际 macOS API token 由父子私有通道发放，Unix socket 限制 peer PID 为 Swift 父进程；两端 API line 上限 1 MiB。不能为扩展关闭 PID／token 认证、开放 CORS/localhost HTTP、共享长期 App token或提高 API 请求预算。

实际交付是显式 file-based bridge：TailSync 用户授予绑定完整 HistoryIdentity／DB authority／origin／原 src hash 的短期一次性授权；用户把小授权文件交给可选 Chromium MV3 扩展，扩展只在再次点击后凭 activeTab／隔离脚本读取当前页精确匹配图像；用户下载资源 packet，再从 TailSync 文件选择器显式导入。原 authenticated API 仅接收选定行 id 和文件路径，正文在 bounded worker 校验，不新增网页直接访问 daemon 的接口。

这不是 native messaging，也不声称扩展自动提交。授权文件不携长期 App token；资源 packet／provenance 不携 Cookie、会话、完整 URL query 或页面正文。来源 URL 只保留于既有原 HTML，授权侧可以使用原 src SHA256 和最少 origin。首批静态 PNG 支持不扩大 O03 的图像准入。

未安装扩展、没有权限、拒绝／撤销、页面关闭／跨 origin、坏图与歧义关联须明确失败或 partial，不丢弃原件。若新语义需要存储版本，必须显式版本化、旧 reader fail-closed、保留 v2／v3 原合同；不能把 O04 public-only 门直接放宽为任意本地文件／blob 获取。

## 3. 独立退出门

- 授权来自实际受认证用户操作，随机／TTL／one-use／revoke／完整身份和当前 DB authority；packet 自报 consent 不产生权限。
- 精确原 src hash／img index／跨度，拒绝重复／额外／无关资源、跨 origin、凭据字段和更改原内容；bytes／PNG／数量／像素／JSON／base64／deadline／并发均受限。
- read／commit 派生证明在重开后仍成立；原件与 hash／capture/content/dedupe／source／favorite 保持；O04 后续补图不丢来源证明。
- 撤销／过期／删除／supersede／reopen／取消阻止迟到提交，SQL／quota 拒绝不留半提交或孤儿载荷，不 trim 用户原件。
- 两端实际 UI／认证 API／生成契约接线；文件选择取消与 session 失效不泄漏授权、不回显 secret/path/body。
- 扩展只在明确动作下访问当前页，不申请 cookies／长期 host／clipboard／history 权限，不自动安装或操作用户日常页面。
- 完整本机门禁与真实浏览器／GUI／目标应用／Windows／设备／CI 分级；A15 未取得真实来源授权证据仍 UNKNOWN，Release NO-GO。

## 4. 审查、二次修复与最终结果

交接源码 `262324d72bf79e205caf5dcea2d456a41774b98cd8e4083b5ac0f985baf94475`（794 文件）；停止 agent 写入后独立构造六项探针：Rust **0 PASS／2 FAIL**，JS **0 PASS／4 FAIL**。原失败退出 101／1，不被子代理全绿结论覆盖。

| 已证实问题 | 二次修复 | 复验入口 |
|---|---|---|
| 已存 proof 的 origin 不对应原 reference 仍能过验证 | covered 节点必须 index＋原 src hash＋canonical origin 一致 | `review/probe/tests/source_review.rs`；Core 内置负向断言 |
| partial blob 第二次补图丢旧 proof，提交失败 | 对现有层证明单调新增，合并旧 proof，再对原件整体验证 | 原 partial→source 探针；source→source→public 文件 DB 探针 |
| 用 src A 的 hash 关联到 currentSrc B | 只使用对应 raw／resolved src；srcset drift 和异步 DOM 变化拒绝 | `review/collection-review.test.mjs` |
| popup 检查 origin 后，收集时 navigation 可突破来源绑定 | injected collector 入口、每项请求及成功回包前检查实际 location.origin | 原 navigation 探针 |
| 合计额度只在 popup 收集完成后检查 | 流读取累计入账，额度耗尽不再请求后续节点，硬夹紧用户预算 | 原 aggregate-read 探针 |
| fetch／body 挂起没有截止 | 独立 Promise deadline，不依赖 AbortController 被 transport 遵守；10 秒单请求／30 秒整体 | 原 pending-fetch 及追加 pending-body 探针 |

另外完成的二次修复：

- 文件打开 no-follow／nonblocking／regular-file descriptor 与路径身份／mtime／ctime／长度稳定性检查；symlink／FIFO／目录／变更身份负向测试。Windows reparse/share-mode 代码已有，但只能记 host 编译，原生验收 UNKNOWN。
- JSON／base64 解析有界 worker，base64 在真实导入链只解一次；来源解码／派生准备移出 DB-lock，admission／共享内存 lease 随实际 worker 存活。全局 256 MiB 不提高，大输入或并发可返回 `source_busy`；严格 commit 验证仍持锁，不宣称所有开销已消除。
- 授权完成释放 live capacity，但旧标识不再能消费；撤销／过期／取消／DB 重开等待竞态回归，载荷准备后事务入口再检查 expiry，完整 identity＋manifest CAS、非驱逐式 quota 与 pending guard 保持。
- 来源→public 补图保留 Manifest 4 旧 proof／资源／bindings；旧已绑定资源不可被换图或擦除。原 O04 测试中允许直接换已有资源的断言改为拒绝＋保留旧层，属于加强负向门，不是跳过测试。
- 两端导入 API 加 selected id，误选其他行的 packet 拒绝；dialog generation 在授权／导入前复查，late-created grant 主动撤销。Mac import request_id 和受管理 session task 接通，实际 socket 取消／目标参数与 partial report 回归；Windows late picker／grant 的 React 回归通过。
- ticket 严格形状、64 KiB cap、origin-only、expiry／budget／unique nodes 检查；export 有界 worker／create-new，Mac 不覆盖写入。popup 在点击和下载前复查 ticket，无可导入节点不下载空 packet；DOM／src 字节／PNG chunk 遍历也有硬上限。
- stable codes 不回显 source file／raw JSON／storage detail；移除新加代码注释、修正命名规范，重新生成契约，未添加 allow／ignore／skip。

## 5. 版本、授权与可信性

Manifest **4** 的 method 为 `portable-cbp1+resource-archive+source-context`，单一 archive_layer 与 `source_info { origin, nodes[index,src_sha256] }`。v2／v3 序列化／冻结 digest 不重定义；旧 reader fail-closed。schema 13、HistoryIdentity 1、CNA1 1、RestorePlan 1、helper／网络版本不变。重开后重新验证整体来源／公开派生关系，不继承一次内存授权结论。

最多 8 live grant／300 秒／一次消费，绑定 grant id＋history identity＋DB authority＋origin＋原 src hash。Ticket 只含最少 scope；packet 只含 scope 与 PNG，最多 24 MiB JSON、8 节点、8 MiB／图、16 MiB 合计。缺项是 partial，不把不支持 CSS/srcset/SVG/object 的文稿伪装完整。提交不写 clipboard、不自动同步。

**信任限制：**文件桥没有浏览器签名或 attestation。用户可以自行编辑 packet；src hash 和 stored proof 证明授权范围／精确派生，不密码学证明 PNG 来源于网页。PNG 正文／metadata 可能敏感，既有原 HTML 也可能含 URL；本功能不提供全内容脱敏。文件授权须保管并按需删除。

授权创建、文件保存和响应递送不是分布式原子事务；若 daemon 已创建 grant 而响应丢失，客户端拿不到 grant id 便无法立即撤销，其权限仍受 300 秒／8 live cap 限制。已返回的 late grant 与导出失败会主动撤销。Windows session close 目前验证 late UI 抑制，不宣称会取消所有已进入 Tauri 的后端导入；显式 revoke／45 秒截止／CAS 仍有效。完整 GUI 场景继续列 UNKNOWN。

## 6. 同源码最终门禁与证据

最终源码 `a996992fc3922c4fe37164f7241fee40780d751c3dde04042f396fb396d91096`（796 文件）；输入→最终 51 文件改变，交接→最终 25 文件改变。**29 条同源码最终命令 exit 0，验证器 0 errors**，见 `review/matrix-final-v2.json`／`review/verified-final-v2.json`。原 Rust 六项中的 2 项与 JS 4 项在该源码分别 **2 PASS／4 PASS**，不依赖早期 GREEN。

| 最终门 | 执行通过数 | 既有 ignored／skipped |
|---|---:|---:|
| Protocol all features | 51 | 0 |
| Core default／enabled | 688／701 | 各 4 |
| Core DB all features／repair integration | 161／14 | 2／0 |
| Runtime all features | 62 | 0 |
| O04 独立非回归／O05 原 Rust／JS 探针 | 8／2／4 | 0 |
| 文件 DB＋CNA1＋离线私有 board | 1 | 0 |
| macOS Rust／Windows-host Rust | 118／127 | 3／2 |
| React focused＋contracts／Swift | 63／269 | 0／4 |
| Extension collector＋popup／认证静态门／capture policy | 10／3／5 | 0 |

Clippy `-D warnings`、三组 fmt、TypeScript、canonical includes、contracts、feature 防漏、capture failure visibility 与 diff-check 同组 exit 0；Windows-host 保留一条既有 Win32／InvalidData dead-code warning，其余最终门无额外 warning。测试数字不跨 default／enabled 等重叠 suites 叠加为唯一用例总数。

`review/probe/tests/end_to_end.rs` 使用真实文件 DB／加密 CNA1／收藏，partial source→source→public 保存不同颜色三图，重开核对原 HTML／CNA1／data_hash／capture/content/dedupe／favorite，离线 materialize／RestorePlan，再用**本轮 freshly compiled helper** 写入仅 `tailsync-probe.o05.*` 私有 board 并读回。公开资源 transport 为可断言 fixture，不假装真实登录网页；不使用日常剪贴板或旧包作为新源码证据。source_peer 未在该探针单独断言；其保全依据 commit 只更新 data／size 的代码复核，不冒充额外执行证据。

共享 `target/o04-shared`，关闭 debug／incremental，Rust suites 串行，Rust／Swift／Vitest 使用隔离 runner。所有主代理尝试以独立 ID 保存 raw log、actual exit、source before／after 和 SHA256；早期边修边测记录可能混源，只作为诊断，不能充当最终验收。v1 29 条中 auth guard 因 Windows 原变量是 `st` 而非 `state` 误报；核对原代码后改为逐平台完整认证拒绝 block 与 dispatch 顺序比较，原 FAIL 保留，v2 全组重跑；未放宽认证检查。

子代理披露 log-21 与 log-53 的部分失败重用了文件并被后续文本覆盖，无法追溯的原失败 log 不被补造，也不作验收证据；完整说明保留在 `agent/HANDOFF.md`。其他 agent、主代理 RED／GREEN、各版源码／patch／失败矩阵与新 ID 重跑全部封存。13 份历史 seal／12,226 文件最终复算 **0 mismatch**，见 `review/prior-seals-final.json`，不改写 O04 证据。共享 target 当前约 4.2 GiB；未 stage／commit／reset／clean、替换运行 App 或既有测试包。

## 7. 验收边界与停止条件

本机实现与自动／持久化／私有 board 接缝，和完整 A15 **不是同一结论**。真实 Chrome／Edge 加载、真实登录态／blob 页面、扩展 permission／页面关闭、NSSavePanel／NSOpenPanel／Windows picker、目标 App 粘贴、Windows 原生、双设备与 hosted CI 仍 **UNKNOWN**。未安装扩展基础采集保持，但本轮没有真实“无扩展／撤销／关闭来源”现场验收。

只交付可选手动文件桥＋同源静态 PNG，不承诺任意登录网站，也不实现 QQ 专用集成、O07／O09 或 F01–F06。本阶段停止，等待用户复查；本机部分 **LOCAL_COMPLETE**，真实 A15 **UNKNOWN**，Release **NO-GO**，不自动发布或继续下一项。
