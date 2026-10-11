# R2-C：下载图片补全同步验收

日期：2026-10-08。状态：**R2-C LOCAL_COMPLETE；真实设备 UNKNOWN；Release NO-GO**。实现、父代理独立二修及最终本地门禁完成。实际工作树 `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。这是工作树源码，不是已安装 App 或真实两设备的验收。

## 用户能得到什么

一条图文复制先通过现有同步发给可信设备；发送端之后自动或手动保存的公开 PNG、JPEG、静态 WebP 图片，可以继续补到接收端的同一条历史。接收端不重新下载网络图片，不产生一条新的“补全”记录，也不会因为后台补图而写入当前剪贴板。图片仍使用已有的预览和恢复入口，没有新增一排格式按钮或新的高级设置。

自动下载仍默认关闭，且只处理符合既有条件的新本机复制；旧记录仍由用户手动保存。未登录可公开访问的常见图片在范围内，登录、blob、私有网络地址、动画、SVG、复杂网页排版不因此新增支持。普通剪贴板同步与后台历史补全是两个不同通道，后台失败不会误报成刚才的复制丢失。

## 生产链与边界

1. 原始本机历史保存后，从真正待发送的 `SharedEvent` 取得原事件 ID、时间戳和确切便携载荷；先校验 DB authority、capture/content/dedupe 身份和 base digest，再绑定本机事件。不会靠显示名、内容 hash 或相似文本寻找记录。
2. 原始事件先进入各可信 peer 的发送队列，之后再启动该条目的自动下载。手动下载恰好先于原始入队完成时，绑定读取新的 archive stamp，并安排一次补全；不会因为 archive revision 已变而丢掉这次补全。
3. 自动下载和 macOS 本地 API、Windows Tauri 命令及 Windows 本地 API 都使用同一 typed commit hook。SQLite 保存成功后提交小型身份描述符到有界队列，最多 8 个等待项，同条目合并；不把正文或原图副本永久放在此队列中。
4. 发送时重新验证记录仍存在、authority 和 archive 身份仍有效。队列复用当前公开图片预算、全局 pipeline/decode admission；失去准入、队列满或发送失败不会回滚本机已保存图片。它不是无限历史回填服务，不承诺跨过既有可靠传输窗口后仍自动补齐。
5. 新独立能力 `archive_resource_updates` 必须双边支持，并依赖健康 composite 服务及 wire-v5；旧客户端缺字段即不支持。新 `ArchiveUpdatePayload/Chunk/Ack` 命令和 ARC1 chunk 类型与普通复制独立；不支持时绝不退化成另一条 Text/Image/Composite 复制。
6. 每个 peer 的发送帧在压缩前和每个 chunk 写出前重查 sync、启用状态、连接模式与原始 pinned key。已经发出去的字节不能被撤回；检查到撤销后不发送后续块。授权检查与网络写不是绝对瞬时的分布式原子操作。
7. 入站以认证 source、原事件 ID、base digest 关联既有历史，再生成 PNG 派生层及保留原始编码。只有 DB 同事务提交成功才通知刷新；此路径没有调用 native publisher、restore、engine activation 或网络 transport。

ARU1 只携带原始 PNG/JPEG/WebP body 和源 HTML 图片序号，不额外发送另一份派生 PNG、URL、来源授权或 NativeArchive。数量最多 8，单图 8 MiB、原图总量 16 MiB，wire 最大 `16 MiB + 128`；4 Mi 单图/8 Mi 累计像素，16 Mi 单 PNG/24 Mi 累计派生。沿用现有安全预算，未扩大限额。

## DB 正确性与二次修复

不新增 schema/manifest 字段，不改旧 data_hash、capture/content/dedupe 身份。原始事件映射和一个最新 revision tracker 复用既有元数据表；活历史的关联不按短 replay 窗口清掉，过期且 history_id 为 NULL 的 tombstone 才清理，总 cap 仍为 4096。容量耗尽是明确失败，不覆盖老关联或无限增加元数据。

| 独立复核发现 | 最终处理与证据 |
| --- | --- |
| tracker-only 分支没有在 derive 后再次检查取消和当前历史身份 | 写前/commit 前检查取消，事务内验证观察的 manifest/identity 与认证 base 映射；SQL 注入身份变化回归真实 RED 后修复，推进 tracker 失败整笔回滚 |
| 后台补全被误计为剪贴板丢失 | 从普通 clipboard warning 分类移出 ArchiveUpdate；真实 RED 后通过；发送队列堵塞也不生成普通复制丢失告警 |
| 已保存 receipt 的重复快速路径可对损坏原图误报成功 | 真实损坏 stored 原图回归 RED 后修复；重复 ACK 前读回、验证已保存原图及其派生对应。拒绝损坏，不偷偷重写文件或声称自动修复损坏资料 |
| DB 首交付新增不允许的 doc comments | 最终删除本轮新增注释，保留 baseline 旧注释；未新增 inline/doc comments、allow 或 ignored |
| 仅有 bind/export API，没有出站调用 | 接入两个平台真正的捕获广播、自动 worker 和全部手动保存入口；增加生产接缝集成回归 |
| 接收历史可能被内部误绑为本机原创 | bind 拒绝带远端事件映射的历史，export 不把远端记录作为原创转发；新增对应回归 |

重复同包不增加本地 archive revision 或重复通知；同远端 revision 的不同载荷拒绝，旧 revision 不覆盖新内容。接收端已存在的本机补全与 source proof 不抹掉。不存在的 base 不创建历史，返回既有可靠窗口内可重试的 BaseMissing；已删除/清空的 tombstone 返回终态 Superseded。SQL、配额、文件、取消和身份失败不提交一半 tracker/一半归档。

## 本地证据究竟证明什么

- Codec 24 项：编码、格式、序号、原字节、每个截断边界、重复/reserved/超限、危险来源、动画、像素和派生预算。
- DB 18 项：原始事件绑定/导出、认证 source/event/digest、缺失/删除、重复/倒序/碰撞、来源 proof、重开、配额/SQL/取消、live 映射/cap、二修的身份与损坏回归。
- Core 收发 5 项：实际可靠 delivery、内存二进制帧、archive assembler、认证 receiver、DB 同条历史；无能力不发送/不 fallback；错误 source/授权撤销/删除不复活；多 chunk/跨类型/身份共享准入/释放；逐块发送授权检查。
- Runtime 集成 3 项：同一原始 SharedEvent 绑定后，自动和手动真实下载操作 hook → 发送队列 export → 可靠 ArchiveUpdate 帧 → receiver → 同一条历史，部分失败再手动重试，三种原始 body 保持，重复只通知一次，关闭旧接收 DB 后重开能读取离线预览资产和恢复载荷；另覆盖有界合并、删除/DB authority 替换及下载先于绑定的竞争。

Runtime 用受控 transport 提供真实可解码图片，不做真实 HTTP/DNS。原始 base 经过同一二进制 frame/assembler，但原始 native 接收效果以 fixture DB 入站模拟；补全阶段使用实际 archive receiver 和 DB。这里没有真实 Noise/TCP 双设备、目标应用粘贴或原生 GUI 证据，不能把模拟收发称为整机端到端验收。

局部 pipeline lease 和像素限制不等于全进程峰值内存测量。大载荷在同进程模拟发送/接收时可能同时占用同一全局 ledger，并被正确拒绝；小样本通过不认证全容量 loopback 或任意真实网络压力。

## 最终门禁与保护

最终产品指纹 `c4a0e2ee989f55b61e91296fec9c7b90f15598fedefdcb50076d4db99cc4848d`，854 个产品/测试/工具/配置文件。以下 16 个父代理命令执行前后均为同一指纹、exit 0；另有 33 个改动 Rust 文件只读 rustfmt 检查与 `git diff --check` 通过，共 18 项最终门禁。不用零执行测试的 check 命令替代测试通过。

| 最终门禁 | 结果 |
| --- | --- |
| Core lib，全 features | 887 PASS / 4 既有 ignored |
| Core 两组 integration | 14 PASS |
| Runtime，全 features | 119 PASS，含本轮集成 3 项 |
| Protocol，全 features | 63 PASS |
| workspace Clippy，全 targets/features，`-D warnings` | PASS |
| macOS adapter/test，composite feature 编译 | PASS，零编译警告；未执行 GUI |
| Windows adapter/test 在 macOS 宿主编译 | PASS，4 条既有 unique 警告，无新增；不是原生 Windows |
| Settings 生成、本地契约、canonical include、production feature 检查 | 4 项 PASS |
| canonical include 回归 | 27 PASS |
| Windows UI 测试 | 397 PASS |
| Windows tsc + Vite 构建 / lint | 2 项 PASS；lint 22 条既有警告，诊断集合未增加 |
| Swift 整包 | 333 PASS / 4 既有 skipped；未认证四项 native GUI 门禁 |

表中日志采用不可覆盖的 `parent-*-final-04`；通过索引为 `parent-validation-final-04.json`。不累加早期重跑的测试数量。删除了只被 macOS 测试调用、在 Windows 中造成新增 dead-code 警告的共享工厂；两个 macOS fixture 改用既有 runtime 构造器，没有扩大生产 API。

两名内置 DeepSeek/high：Carver 负责 codec，Dewey 负责 DB；完整明文、独立 ACK 后 START、互斥写集，已完整交付并关闭，未另开第三名或后台 CLI agent。父代理冻结首交付、独立复核、二修和最终门禁。

派发审计保留执行偏差：codec 两次临时源码/日志操作在 `/tmp` 和 `/tmp/r2c`；DB 的 101 次命令使用立即 `cd` 到确切绝对工作树，而不是要求的显式 workdir。产品有效目录核查无混用，但不宣称形式约束全部遵守。codec attempt2 存在 Cargo file-lock 日志，原派发 `no_parallel_cargo` 是计划要求，不是已认证执行事实；父代理最终 Rust 全部串行。

两次早期全功能 Core 及一次专项重跑出现两项自然接口 Iroh 连接超时，根因未证明；没有修改相关连接代码、删测试或增加 skip。之后两次完整全功能 Core 复跑通过，最终一次绑定本节指纹；这不证明偶发网络问题已根治，稳定性仍需真实网络/设备观察。父代理一次新测试用了不可见 `HistoryDB::open_at`，另一次尝试 clone 未实现 Clone 的 Frame；修正测试 fixture 后重新验证，失败日志不覆盖。

1193 个非 artifacts baseline 文件均逐项复核：本轮 34 个产品/测试/工具文件新增或变化，其中新增 7 个，零删除；另更新本执行记录并新增两份 R2-C 文档。28 个既有 baseline 文件定向修改，其余 1165 个不变；HEAD/暂存区不变。R2-A/R2-B 封存记录和其 baseline 再按摘要复核；旧 App/候选包没有执行替换或启动操作。不 stage/commit/reset/clean。复用 `target/rich-text-r1` 和既有 Swift scratch，没有新增缓存副本、读取用户 DB/general 剪贴板或停止日常 App。门禁前约 81 GiB 可用，交付收尾查询约 78 GiB；以实时磁盘查询为准。此容量时点补记不修改已封存源码、门禁或文档快照，另记于 `artifacts/rich-text-repair/2026-10-08-r2c-closeout.json`。

证据根 `artifacts/rich-text-repair/2026-10-08-r2c-archive-sync/`：baseline/首交付、delegation/audit、真实 RED、父代理不可覆盖命令日志、最终索引与保护记录。`sources/<最终指纹>.json` 是完整源码清单，`parent-source-final-04.json` 指向它。`final-delta-v2/` 保存全部 34 份相对本轮 baseline 的定向补丁，已经只读逆向 `git apply --check` 核验；不要对 HEAD 整体 reset。首次生成的 draft diff 尾空白裁剪造成 corrupt patch，保留为 `final-delta/`，不得用于回退；已修正的 v2 才是最终补丁。最终记录由 `parent-evidence-seal-final-04.json` 封存。

## 尚未完成

- 真实公开网站 HTTP/DNS、登录/来源限制与下载行为；真实启动 App 后的开关、预览刷新/焦点及辅助功能。
- 原生 Windows 采集/恢复，Mac↔Windows/两台真实设备补全、断线、撤销、重启和离线目标应用粘贴。
- 托管 CI、新候选安装包及发布认证；没有更新当前安装 App。
- 后续 R3 阅读语义与 R4 真实场景验收。

整个富文本方案保持 active；Release NO-GO。不把本轮源码与本地自动回归推广为整方案或真实设备完成。
