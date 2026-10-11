# 单 Mac 验收与补齐 — 2026-10-05

## 当前结论

当前阶段：**R1–R6 本地完成（LOCAL_COMPLETE）；R7 为 NATIVE_PARTIAL，发布 NO-GO。** 不把部分本机场景或旧候选的结果作为当前包的完整验收。

正式可写工作树 `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，分支 `codex/composite-clipboard`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。原 checkout 保持只读，保留既有修改、旧包与首次失败记录。没有提交、推送、发布或替换日常 App。

当前源码指纹 `cd9268e75f38d52431f985ef8dadd8c4c910eee3b09f638d289fa71a007ec1eb`，756 文件。当前 R6 run：`artifacts/composite-repair/2026-10-05-single-mac-r6-self-contained-final/`。它重新执行完整 44 项：**44/44 PASS、0 unexpected、1 个唯一 Windows host 编译警告**；所有包含 debug 已独立封存。只读验证检查当前源码、原始日志及全部包字节，没有复制旧 run 的 PASS。

此前 `c022bcb8…` 的 R6 记录有 44/44 PASS，但 debug 包引用构建缓存；用户清缓存后，旧包字节已不能完整重验。225 个封存记录本身未变不等于整个旧 R6 仍 VALID。`54582e18…` 的缓存/轮询修复矩阵也完成 44/44、0 unexpected，但因发现同一外部 debug 依赖，保留为未封存诊断 run。新的 run 才使用完整独立 debug 快照。

用户要求不要创建 Codex 界面不可见的子代理后，主代理停止剩余后台 DeepSeek 报告进程，直接接手统计、工具修复、复验与整合。后续后台任务仅为测试/构建进程。

## 本轮修复

| 缺陷 | 修改及行为 | 验证 |
|---|---|---|
| 原生历史窗口没有可靠的标准菜单入口 | TailSyncApp 增加 History 菜单与 Cmd+Shift+H | 真实菜单打开历史窗口、键盘恢复 |
| GUI 配置读取忽略隔离根 | Loc 优先使用 TAILSYNC_DATA_DIR，与 daemon 共用隔离配置 | 两项配置回归；实际 GUI 语言/主题与隔离配置一致 |
| RGBA 预算重复乘像素大小 | 指标明确命名 rgba_bytes；接收/恢复乘 3、旧协议准备乘 4，保留 256 MiB 总预算 | 精确最大 32 MiB RGBA 的接收、恢复、合法旧表示转绿；红日志保留 |
| 最大图旧协议回退未计完整帧 | 纳入 8 字节尺寸和 28 字节头，超旧帧永久拒绝并反馈，文字仍可合法回退 | 最大图拒绝和合法文字/图片回退回归 |
| 写后回执丢失在 socket 中变通用错误 | runtime 精确识别七个恢复状态；Swift 保留机器码，由 ApiError 本地化；显式重试入口可判断 Unknown/Partial | 真 send_json 七码回归和真实 Swift normalizer 两语言回归：先红后绿；最终独立故障包的真实 Unknown / Published 与显式点击计数通过，见末节 |
| Unknown/Partial 再次复制入口 3 秒消失 | 这两种结果的提示/按钮保持到下一次操作；开始新恢复时清掉旧反馈，防止成功后残留 Unknown | 原生首次超时红记录保留；最终包等待超过 6 秒按钮仍在，之后逐次点击恢复通过 |
| Unknown 重新采集去重后，显式重试仍引用被删除的 ID | Swift 与 React 保存原条目的完整 data_hash 与类型；仅重绑定到当前存在的相同内容，删除/变更后报 not_found 且不写；保留所选格式 | Swift 4 项规则回归；React 5 项新增 DOM 回归及既有显式重试；原生开启态旧 ID 红记录保留；最终实测 ID 1→3 后仍可重试，写次数 1→2→3 |
| Windows Unknown/Partial 恢复入口 6 秒消失 | 持续呈现需显式恢复的结果；同一呈现状态用于告警确认，隐藏的同步告警不会被提前 ack | 真实 DOM 等待 6.1 秒仍可重试；隐藏告警直到 dismiss 后真实显示才 ack |
| 格式菜单只有泛化“更多”名称 | 增加中英文 Copy format / 选择复制格式可访问性名称 | 最终包原生可访问性树可读到 Copy format，之前菜单动作观察按包 hash 单列；不由 AX 可读推导 VoiceOver PASS |
| 实际投递缓存少计内存 | 纳入 chunk header、真实 capacity、Vec/缓存/Arc 容器；先 reserve 完整上限再按真实占用缩减 | 真实投递回归覆盖压缩/不可压缩、重试复用、最后 owner 释放归零；保留红/绿日志 |
| Mac composite 分支空闲一直 50 ms | 无待处理 revision 250 ms；新 revision/待重试/wake 50 ms；helper 不可用 250 ms | 正式回归与真实采集的独立 debug/private-board 测量，两组都出现；不提前确认 revision |
| R6 debug 包依赖可被删除的构建缓存 | daemon/helper 复制至 run/debug-package；所有包必须在 run 内，拒绝外部路径与软链接依赖 | 22 项 R6 规则测试；实际 debug 二进制复制后改写/删除一次性输入缓存，仍通过 hash、self-check、生产 resolver |


产品修复之外，先前验收工具已修复：当前源码/完整封存 R6 绑定、逐场景覆盖、非法状态阻断、fault satisfied 断言、支持证据范围、配置指纹、封存不可变及原始证据变更检测。规则入口已进 CI 和 44 项 R6 矩阵；hosted CI 未执行仍为 UNKNOWN。

## 已实际执行的本机子场景

原生目录：`artifacts/composite-repair/2026-10-05-single-mac-native/`。
macOS 27.0.1 arm64，16 GiB；Edge 154.0.4258.37、TextEdit 1.21、Preview 11.0。
会话使用私有数据根和 socket；用户明确释放 TCP 19890。原剪贴板逐格式备份、原 daemon 日志安全移开；结束恢复记录单列。

| 子场景 | 真实观察与证据 | 范围 |
|---|---|---|
| 完整 App | 新候选 GUI/daemon/helper 接通，设置/历史可操作；精确包路径与 executable hash 记在 launch JSON | 原生本机；不是所有通知/故障形态全覆盖 |
| 双图 HTML 来源/整组恢复 | Edge 实际复制形成一条 composite，2 formats / 2 images；恢复后的 HTML/text hash 与源相同 | Edge 不代替指定 Chrome/Word |
| 富文本消费者 | TextEdit 实际粘贴中文 emoji、红字/粗体、红蓝双图；回读 RTFD 有 2 attachments | final-textedit-target-rich.json；浏览器内 token 不属于发布格式 |
| 明确格式 | 多图只提供 Copy text，不提供会丢第二图的 PNG；真实纯文本 textarea 粘贴正确 | final-copy-text-formats/value.json |
| 单图消费者 | Preview 复制→历史→恢复→从剪贴板新建，红色图片可见 | 本机 Preview，不代表 Windows DIB 消费者 |
| native-only | TextEdit 的无法无损转换 RTFD 明确 Local restore only | 不冒充跨平台完整 |
| IME 与键盘 | Ctrl+Space 后逐键输入，组合态下 Return 未误恢复；Space 得到“中文”；Down/Return 恢复，窗口关闭且 clipboard source marker 存在 | ime-before/after JSON 与 CUA 截图；已恢复原输入源 |
| 深浅色 | 两主题实际焦点/内容可见，测试后恢复 System | 未测试系统高对比/显示缩放 |
| storage 满 | 128 MiB 测试卷真实填到 available=0 / ENOSPC；新复制明确未保存，baseline 可恢复；释放填充后可正常入库，integrity_check=ok | storage-full-*；24 MiB 试验因 64 MiB 安全余量被正常拒绝，精炼记录保留 |
| storage 不可用 | storage_root 指向本次合成普通文件；历史/容量诚实离线，界面响应；恢复目录后 5 秒恢复任务 reopen，新内容持久化，integrity ok | storage-unavailable-*；chmod000 被产品主动修复，不计不可用成功证据 |
| helper 缺失 | 独立复制包缺 helper：启动告警、v12 仍可读、恢复失败且无成功提示 | helper-missing-*；封存候选未修改 |
| helper 挂起 | wrapper 直接 exec sleep 30；新 revision 触发 4 次有限尝试，各约 5 秒 kill/reap，最终约 23 秒无 child；设置/历史响应，capture_failed 持久化且有告警 | helper-hang-process-observation-triggered/terminal/feedback-timing；不是 OS delayed provider 证据 |
| 写后丢回执 | wrapper 调真实 helper 完成写入后丢 stdout；首次一条 native write_attempt，后端 Unknown、无自动重写；原界面 generic failure 导致本轮修复 | receipt-lost-* 和红绿测试；最终候选复验另记 |
| 实际旧包打开/读取/恢复 | /Applications/TailSync.app 2.3.2 GUI 打开历史恢复库；本轮实际安装的 daemon 对当前 R6 回退库经生产 API 逐字节读取文本/外置图片/文件，3 条、1 收藏，并执行 3 次真实 native restore；隔离 identity/receipts hash 未变 | installed-old-reader-current-ready.json；首次探针 hash 算法/过长 socket/仅检查 socket 存在的失败保留并说明。旧 GUI 逐条操作、真实 paired trust 未覆盖，完整 rollback UNKNOWN |

前期原生观察分属 `0d9d3810…` 、`441a268e…` 和 `f398a1b2…` 的包，launch JSON 保留身份；这些观察不能整体继承为最终源码的原生 PASS。最终包重验入口单独记载，自动矩阵则全部针对最终源码执行。
截图和 AX 状态在 CUA 对话记录中；手工写入的 UI 内容称工具输出转录，不声称自动导出截图。
本次 TextEdit 合成文稿已移到私有 fixtures；初次 Settings 冷启动曾短暂 service unavailable，Retry 成功，限制保留。

## 性能与边界

性能观察 crate 调用实际 core，使用交付相同 opt-level s / thin LTO / codegen-units 1。串行正式记录 `performance.jsonl`，并行试验另保留。

| 项目 | 测量 |
|---|---|
| 1 MiB HTML / 5383 bindings | 1000 次平均 0.381 ms，目标 ≤10 ms |
| 小包 encode / decode / native 准备 | 1000 次 p95 0.000459 / 0.000375 / 0.001958 ms |
| 精确最大 wire | 33,554,404 bytes，encode/decode/native 准备均成功；平均 14.97 / 13.99 / 6.46 ms |
| 最大 RGBA | 33,554,432 bytes；native 准备平均 106.27 ms |
| 真实 general clipboard→daemon commit | 50 次 p50 48.64 ms、p95 60.01 ms、max 70.67 ms；含 writer 启动，SQL 观察粒度 2 ms |
| 原生进程 RSS 采样 | GUI 峰值 104064 KiB，daemon 29760 KiB；50 ms 采样 |

benchmark 同时持有多个最大样本时 RSS 302,956,544 bytes，不能解释为单条生产流水线突破 256 MiB 配额。
本轮已补真实 cache 占用/复用/释放、PUBLISH 串行等待与 debug/private-board timer（50/250 ms、包含真实采集）。主代理修正了 timer body 连接错位与 atomic peak/snapshot 的混称，并独立复算。详见[性能补齐报告](COMPOSITE-CLIPBOARD-PERF-FOLLOWUP-2026-10-05.md)。这些是限定构建/适配的测量；release/general-board 的同环境 timer、读侧并发没有单独测量，仍 UNKNOWN。

## 仍需完成的条件

| 项目 | 具体阻碍 / 下一步 |
|---|---|
| VoiceOver、高对比、显示缩放 | 已发出临时开启并还原的许可请求，未收到明确答复；原生 UI 工具要求非敏感设置变更有具体授权。键盘/IME/深浅色已测，不由 AX 可读代替读屏 |
| 微信目标 | 4.0.6 停在“进入微信”；进入当前账户的请求尚无明确答复，未登录、未发送 |
| Word / Chrome | 本机未找到；Edge/TextEdit 只作为新增消费者证据。WPS 停在 EULA，未接受，不能替代 Word |
| Mac 剩余完整场景 | 真实 delayed provider 已在旧候选 capture/read 路径测过，不能推广到新候选所有 publish 路径。真实通知 queue/Swift 消费/UN add 已在重签测量副本观察；屏幕 banner 仍 UNKNOWN。旧 GUI History 入口有具体工具阻碍，真实 paired revoke 缺 peer；限定性能测量已补，release 同环境 timer 仍 UNKNOWN |
| Windows | 原生 MSVC/dev/portable/NSIS、真实读写/错误提示、布局/读屏、DIB 消费者须 Windows 主机 |
| 双设备 | 两向同步、慢接收竞争、100 轮交替、LAN/Tailscale/Iroh direct/relay、睡眠重连/ACK 丢失须第二设备与真实 peer |
| hosted CI | 未提交/推送，未知；本机绿色不代替运行结果 |

R1–R6 本地门禁可以完成；R7 有大量本机子场景可以继续，缺硬件与权限的项目应有具体原因而不是统一标“需设备”。目前不能回答“单 Mac 的所有可能任务都已完成”，也不能宣告 ACCEPTED_CANDIDATE。

## 历史 c022 包复验与会话恢复

当时矩阵源码 `c022bcb8…` 与四个默认/开启 × native/universal 包绑定。最终正常开启包的 launch 是 `retry-final-normal-launch.json`；菜单快捷键打开 History，真实采集合成中文/emoji 后恢复，文字 hash 一致。对之前真实 Edge 采集的双图条目，最终包整组恢复后的 HTML/text 与来源 hash 一致，见 `retry-final-rich-observation.json`。旧来源→消费者和 Finder 17 文件观察仍按它们各自 hash 保留，不整体转记为新包通过。

独立复制/重签包 `TailSync-retry-identity-final.app` 才注入回执丢失，未改正式包。`retry-identity-final-injection.json` 同时记录原包全部文件 hash 和注入后 hash；注入后的 GUI/daemon hash 与正式包不同，不能宣称包身份相同。

- 首次 double-click 调用真实 helper 一次后丢 stdout，UI 显示 Unknown / Copy again。采集将 ID 1 替换为 3，完整 data_hash 相同。
- 等待超过 6 秒，native write 仍为 1，按钮仍可见；一次明确点击后计数为 2，再次 Unknown，没有 generic error。
- 允许完整回执后再明确点击一次，计数为 3，文字 hash 一致，UI 显示 Restored to clipboard 并移除 Unknown / Copy again。
- 原始 wrapper 计数、SQLite root ID/hash、元数据与观察保存在 `retry-identity-final-{before,unknown-wait,explicit,published}.json` 及故障包日志；原生修复前 stale-ID 证据保留。

`final-session-restoration.json` 确认两个最终 GUI/daemon 会话全部退出、19890 空闲、两个合成磁盘卷已卸载。此处描述前一轮会话的恢复记录。后续早期子代理恢复测试前剪贴板时，没有保存测试期间后来一次用户复制；此失误已向用户说明，不能宣称全程无损恢复。之后 timer/publish 使用 private board，通知只读取 general。原 daemon 日志 138,731,421 bytes 已恢复原位置，恢复前后 SHA256 一致。主题与输入源的此前恢复保留；未修改 LaunchAtLogin 注册。

本次测试创建的 TextEdit/Preview/Finder文稿窗口已收起并保存到私有 fixtures；浏览器合成来源页的最后关闭状态没有可靠复验，不宣称所有浏览器窗口均清理。私有剪贴板备份不进入公开证据清单。

R7 新入口 `2026-10-05-single-mac-r7-retry-identity-final`：本机 fault/rollback 支持另列；十个完整门禁仍 UNKNOWN，Release NO-GO。原生目录以 `native-evidence-index.json` 保存逐文件 SHA256，源码及包身份范围由各 launch/观察 JSON 单独说明。


## 后续真实原生补齐与证据身份

[原生 follow-up](COMPOSITE-CLIPBOARD-NATIVE-FOLLOWUP-2026-10-05.md)与 supervision 的身份附录记录：

- 真实 general-board delayed provider 阻塞 45 秒，旧候选 helper 4 次约 5 秒有限尝试，最终 capture_failed，全部回收；provider 返回后无晚采集/回音写入。阻塞期间真实菜单/History 操作可响应。这个 probe 只读，不认证所有 publish late-write。
- 通知 IDs 1/2 来自真实 storage unavailable/recovered，队列、Swift 消费、UN add 成功均有直接证据；banner 没有直接观察。测量副本使用只读权限查询，并经 deep 重签：daemon/helper signed SHA 改变，独立去签名后的代码 SHA 相同。结果只绑定测量副本，不能继承为正式 GUI 权限或当前候选结果。
- 旧 2.3.2 GUI 能在隔离数据根启动，但工具不能可靠触发 History，逐载荷 GUI 恢复仍 UNKNOWN。生产 API 恢复证据与 GUI 证据分列。
- 原生 309 文件、follow-up 47 文件与旧 R6 225 记录的完整性独立重验均无变化；外部 debug 包缺失另计，不能混称 VALID。

## 用户能看到的功能与按钮

| 界面位置 | 当前行为 |
|---|---|
| App 菜单 | History / 历史记录，Cmd+Shift+H 打开历史 |
| 复合历史条目 | 同一条目保存多格式/多图，分别显示格式数、图片数；显示仅本地恢复/不完整等范围 |
| 选择复制格式菜单 | Copy text / 复制文字；实际可安全恢复的单图才有 Copy image / 复制图片；多图不提供会静默丢图的单 PNG 操作 |
| Unknown / Partial 反馈 | Copy again / 再次复制保持可见，需明确点击；不自动再写，也不假报成功 |
| 采集/同步失败提示 | 显示失败、不完整、结果未知或被新意图取代等真实结果；保留可恢复历史 |

本轮最后一项补修加强证据封存，没有再添加新的 UI 按钮。新功能的可用性与真实消费者能粘贴哪些格式分别验收。


## 当前 cd9268 候选的完整 App 启动检查

新 `TailSync-enabled-native.app` 正常 GUI/daemon/helper 包来自当前独立 R6。记录在 `artifacts/composite-repair/2026-10-05-single-mac-current-smoke/`，launch 包/可执行文件 hash 与 R6 manifest 对应。只读实际观察：

- 设置首次出现 service unavailable；明确点击 Retry 后生产 GUI 接通，Storage 路径为本次私有数据根。
- 标准 TailSync > 历史记录菜单与 Cmd+Shift+H 都实际打开 History；搜索框可达，隔离历史为空。
- 外部 Python status 请求被 parent PID 校验正确拒绝；首次 harness timeout 保留并说明。未弱化校验，连接证据来自真实 GUI，不把 Python 拒绝当产品启动失败。
- 通过实际 Quit 菜单退出，GUI/daemon 均结束、19890 空闲。原日常日志 138,731,421 B / SHA `98abedd1…` 恢复一致；剪贴板 changeCount 231→231，未写入或还原剪贴板。

这是当前候选的正常启动/设置/历史入口 smoke，不是当前候选的采集/恢复/消费者/通知完整组 PASS。CUA 观察是工具输出转录，未冒称自动导出的 AX 日志或截图。

最新 R7 入口为 `2026-10-05-single-mac-r7-self-contained-final`，绑定当前封存 R6。十个完整门禁仍 UNKNOWN，本机 fault/rollback 支持证据另列，以 run JSON 与 `native-acceptance.md` 为准。

**对“Mac 能完成的都完成了吗”的答复：R1–R6 必需本地任务已完成，已授权且当前工具可执行的 R7 子场景已补到上述范围；不能笼统宣称 Mac 全部验收完成。** 未完成项与缺硬件、缺应用、需具体授权、工具阻碍及尚未测量的范围见本报告条件表。没有改变原 R7 通过条件。
