# S2-03：展示分类、严格首图与版本化缓存 — 实施及独立复审

日期：2026-10-07。仅完成 S2-03 的实现及本地自动验证，未进入 S2-04/05/06。

## 1. 结论与流程

**S2-03：LOCAL_COMPLETE；整个 S2 尚未完成；Release：NO-GO。**

- 使用一个内置 `cline-pass/deepseek-v4.1-flash` / high 子代理，完整明文派发、不继承历史；实际工作树、分支、范围回执核实后实施。
- 等待完整 HANDOFF 并关闭子代理后，父代理才修改产品。子代理交付明确是部分范围，不能直接作为最终验收结论。
- 父代理独立复现分类与缓存错误，补齐两端生产接线、有界索引重建、真实首图与响应身份检查，完成二次修复。
- 最终 R2 同一冻结源码的 **24 项门禁全部通过**。R1 完整矩阵的 Iroh 实网超时及 probe 的 test-support 依赖边错误仍完整保留，没有覆盖失败或修改网络测试。
- GUI/AX、真实 QQ、Windows native、两设备、hosted CI、真实性能/RSS 仍为 UNKNOWN。没有发布、打 tag、提交或替换日常 App。

## 2. 工作树与源码绑定

唯一工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`。

| 项目 | 值 |
|---|---|
| 分支 / HEAD | `codex/composite-clipboard` / `f719d581a1aa5198a1b3f2483222df5c69e3dfc0` |
| 输入 | `3edd3c43cb887d31b5f7dbe2e8d0bb77ac4c631ad2e0e26447bb46ec0cad7367`，806 文件 |
| 子代理交付 | `ff33d501cc791e8f0eec435d02edefe2e65def81ceeb711122f6ae6b461d15d3`，808 文件 |
| 父 R1 冻结 | `0b2288797f46575e2fe9c9ebecd7f18aa31afbddc4692298ba5674d00e96cfe4`，812 文件 |
| 父 R2 最终 | `62fddd236155a0ad84b0776719c65d68edf16eeca98279cffa2a98fd4b3bbb25`，812 文件 |

指纹沿用 `scripts/composite-evidence.py`，包含未跟踪产品源码，文档/证据单独封存。相对本批输入新增 6 个、修改 54 个、删除 0 个产品/测试/生成文件；不把整棵既有 dirty 工作归算为本批。

子代理增量为新增 2 个、修改 12 个文件。生成契约、两端命令及 DTO 的较多配套改动属于父代理补齐生产接线，不是子代理已完成的工作。完整三组增量分别保存为 before→agent、agent→final、before→final patch。

## 3. 独立审查与二次修复

| 已确认问题或缺口 | 最终处理 |
|---|---|
| 仅 URL 外形 + PNG 就吞掉正文，判为图片 | 无关系证明时 Unknown；真实正文和无定位 PNG 不再错误宣称纯文字 |
| 只有核心分类，文稿/未知的两端展示未接线 | 版本化 display 元数据、两端图标及中英文标签；不把 document 加成非法筛选类别 |
| 子代理认为没有 NativeAnalyzer 生产实现 | 核实已有受监督保存字节 helper 工厂；两端 history/image 接入实际 `native_analyzer()` |
| native-only 附件 portable image_count=0，可能没有图片资格 | 从共享分析事实产生类别/thumbnail_eligible；原生附件的 checked 首图路径不依赖 portable 计数 |
| 分类更新会改变用于删除配额的 primary category 桶 | 保留原 category 存储桶；仅更新 categories、confidence、classifier_version，不 trim、不迁移 schema |
| 旧记录未重建，筛选及计数可能与新展示不一致 | 每请求最多重建 4 条，锁外分析、revision/manifest CAS；pending 时不报告完整 total |
| 一条无法分析的旧记录可能阻塞整个列表 | revision 验证后保存保守 Unknown 索引，不修改坏原件；SQL/提交失败仍作为错误返回 |
| inline HTML 首图被独立 PNG 代替 | 从实际 src span 解出 inline PNG；get/prepare/async/checked 四条路径均验红图首位 |
| key 只绑定内容，缺少 DB reopen authority 和服务端复验 | 每次 DB open 独立随机 authority；key 绑定行身份、加密 manifest、分类/分析/renderer/spec 版本；请求前后验证 |
| 成功/空响应永久占据 in-flight marker，驱逐后不能重载 | in-flight 只代表 pending；finally 仅释放本次 token；空/坏响应可重新请求 |
| NaN 等尺寸可进入 cache，点尺寸掩盖实际 bitmap 成本 | 两端验证尺寸/像素长度；真实 bitmap/编码内存计账；硬预算与驱逐 |
| clear/retain 清掉 marker，却留下无限旧 Promise 队列 | 按真实未结束 job 计数，跨 generation 总计最多 64，真实 finally 才释放 |
| guard 正常完成也取消调用者 token | 使用私有 Cancellation 及 watcher；取消和 abort 锁等待不污染其他调用者 |
| Busy 在 Tauri 被转换成 internal_error，字符串 mock 的重试假通过 | 两端仅匹配固定 Busy 消息，保留 temporarily_busy 策略；React 用真实稳定错误 envelope 回归 |
| 外部测试 crate 用普通依赖开启 test-support | 改为 dev-dependencies；不放宽 production feature checker |

独立 Rust 探针先为 6 PASS / 2 FAIL；独立 React 探针先为 0 PASS / 3 FAIL。最终外部生产接缝探针 10/10，追加 React 父回归 8/8；旧失败均留存。

## 4. 分类、索引及兼容边界

展示分类版本为 `1001`；对外 `HistoryDisplayInfo.version=1`，kind 为 image/document/text/file/unknown。未知版本/kind 在 Swift/TypeScript decoder 拒绝，不以 fallback 隐藏格式漂移。

| 分析事实 | 展示 | 已注册 categories |
|---|---|---|
| 选定正文为空、有位置图或单 PNG | 图片 | image |
| 真实正文 + 有位置图 | 文稿 | text、image |
| 已支持富文本结构、没有图片 | 文稿 | text |
| 单纯文字 | 文字 | 既有文字 classifier 的合法类别 |
| 原生分析不可用、多 native item、解析边界失败、关系不明 | 未知 | 保守 text，可能有图则 text、image |
| 既有 file 条目 | 文件 | 原文件类别 |

不按格式数/资源数判图文；不根据 URL/文件名外形吞正文。单图多表示正例使用实际无正文 HTML 图节点；没有这种关系事实的 URL+PNG 反例保持 Unknown。不能把 Unknown 宣称为完整保真。

`category` 保留历史存储配额桶，`categories` 用于筛选，`display.kind` 用于展示。kind 通过版本化派生 confidence 标记区分（95=image、90=document、85=text、0=unknown），不引入新列。新插入的 HTML/RTF/native-only 条目先用 pending 派生索引，再经 owned 分析提交；归档变化使分类版本失效。

索引按陈旧分类版本每批最多 4 条，分析与 helper 在 DB 锁外；正常提交使用实际 AnalysisHandle/revision 和加密 manifest CAS，不更新原件、hash、capture/content/dedupe、size、source、favorite 或配额桶。坏记录的 Unknown 提交同样验证 revision，并保留原 bytes。

paged 响应携带 `display_index_pending`；pending 时 total=None。两端新 UI 有取消/请求 generation 检查，最多推进 256 批，不无限一次性重建整库；仍未完成则可重试继续。无法表示 pending 的旧 Vec 命令返回 Busy，而不是假称筛选已经完整。

搜索继续使用已有保存正文摘要/metadata 及合法类别；本批没有新增 native-only 全文检索索引。关键字查询的 total 仍沿用既有 None 语义，不伪造计数。Unknown 索引不是自动修复原件或自动补资源机制。

## 5. 严格首图、authority 与缓存

两端生产 image 命令 → `image_payload_checked_async` → 共享保存内容分析/实际 NativeAnalyzer → 第一 placement → owned worker 解码 → authority/revision/key 后验 → 有界缩略图响应。

- 按正文/附件顺序，不能拿资源数组第一个元素、独立 PNG、第二张可用图代替第一张。
- 第一图缺 src、外链、缺资源或不可解码时准确失败/占位；不隐式访问网络、file、来源缓存或 general clipboard。
- opaque `th1` key 包含 DB 随机 authority、id/type/timestamp/data_hash、加密 composite manifest 摘要及分类/分析/renderer/48pt spec 版本。manifest 已覆盖 capture 和 archive identity，坏 manifest 不再导致整个页的 metadata key 构造失败。
- UI 再加 history/favorites owner；服务端比较 expected key，返回后再验。Swift 安装前检查 Task cancellation 及 installed key；React 检查 generation、保留集合和 request token。
- legacy reader/旧记录兼容保留；新服务端给旧记录补版本化 metadata。旧 daemon 不提供 authority 时不能据此声称已通过旧库重开隔离，Swift 不将无 authority 图像写入共享缓存。

| 边界 | 预算/行为 |
|---|---|
| 分析 workspace | 沿用 192 MiB reservation、256 MiB 全局 ledger、单分析准入、4 秒监督边界 |
| checked image 输出生命周期 | 64 MiB lease 随返回对象；真实 decode worker 结束前不提前释放 worker 的资产 |
| 普通自定尺寸读取 | 32 MiB，不再使用 u64::MAX |
| Swift 共享缩略图 cache | ≤30 项、≤4 MiB；按 bitmap bytesPerRow×pixelsHigh 加保留编码数据计账 |
| Windows 两窗口 cache | 每 hook ≤2 MiB，两历史/收藏 WebView 合计 ≤4 MiB；entry 与 bytes 双限制 |
| Windows pending job | 串行低优先级，真实 job 总数≤64，跨 clear/generation 不归零绕过 |
| temporary Busy | 有界最多 16 次请求，仅重试固定压力错误；取消/换项检查保留 |

测试证明预算边界和最终 lease 释放，不等价于已经测量真实滚动 RSS、CPU、p95 或所有 GUI 调度情况。缓存未命中/压力拒绝允许占位，不承诺每次极端快速切换均立即显示图片。

## 6. 最终门禁与保留的失败

最终证据：`artifacts/clipboard-improvement/2026-10-07-s2-03-display-first-image-r1/`。选定 R2 矩阵为 24/24，所有命令 before/after 都是最终指纹，无修改中测试充当最终验收。

| 门禁 | 最终结果 |
|---|---|
| Core 默认 + test-support | 726 PASS、6 既有 ignored |
| Core all-features | 759 PASS、7 既有 ignored |
| Runtime all-features / 再显式启用 composite dependency | 各 85 PASS |
| Protocol / Windows helper crate | 63 / 3 PASS |
| 独立生产接缝 Rust 探针 | 10 PASS |
| macOS Rust composite lib | 119 PASS、3 既有 ignored |
| Swift | 284 PASS、4 既有 skipped |
| React | 321 PASS，54 个文件 |
| headless 保存字节 macOS helper | 5 个语义检查 PASS，真实 helper hash 绑定 |
| TypeScript / workspace及Mac Clippy -D warnings / fmt | PASS |
| 契约及命令 generator check / canonical / production features / shared resolution | PASS |
| native policy / capture visibility / Windows-host / 输入封存审计 / diff | PASS |

测试数字按完整 raw log 的 execution summary 记录，包含独立 suite/子进程，重叠 feature 组合不相加成唯一用例总数。Windows-host 仅 fmt、host check 和 test --no-run；其 target cfg 导致的 4 种已有 dead-code warning 文本保留，不宣称 Windows-native Clippy 或运行测试通过。

保留的失败/非验收尝试包括：

- 子代理早期 `| tail` 输出不是完整证据，不纳入最终通过结论；从 repo 根跑 Vitest 产生 CSS cwd 错误，最终从 windows cwd 跑整套，不修改 CSS 测试。
- 父聚焦过程的旧分类期待、测试 seam 名称错误、错误假定关键字 total=Some、inline fixture PNG 校验错误，以及新增构造器遗漏均保留日志；修正原因区分产品缺陷与探针/测试错误，不把它们合算漏洞数。
- 单字母审计曾误匹配测试描述字符串；审计改为声明位置匹配。generator 发出的单字母 decoder container 则从 generator 修正并重新生成，没有手改生成文件或关闭门禁。
- R1 Core all-features：`real_network_one_time_invite_completes_over_real_iroh_and_is_consumed` 出现 Iroh connection timed out。R2 全量原样重跑通过；本批 Iroh 源码与输入一致，**未修复网络波动，其自然诱因仍 UNKNOWN**。
- R1 production feature gate 准确发现外部 probe 的普通依赖误开 test-support；已改 dev-dependencies，checker 未改。R1 23 项矩阵因此仍为 all_pass=false，不重写它来冒称通过。

## 7. 封存、磁盘与停止条件

- 输入、子代理交接、R1/R2 冻结源码分别保留；三组精确增量 patch、命令/环境/退出码、完整 raw logs、实际统计、native helper hash、文档及总清单 SHA256 一并封存。
- O05/S2-01/S2-02 的封存 inventory/bytes 及绑定报告只读复验未变，没有把旧 source 当成当前 source、没有修改或重封旧证据。
- 复用唯一 `target/o04-shared`，Rust 串行、incremental/debug关闭、jobs=2；目前 target约5.1 GiB、本批证据约69 MiB、剩余空间约70 GiB。没有创建第二套大型 target，没有清理 App、测试包、证据或其他工作树。
- 无新 schema/migration、生产依赖或 lockfile 变化；不改 capture/hash/dedupe、网络/认证或恢复原件语义。所有既有 dirty 工作保留，未 stage/commit/reset/clean/新建分支。
- 真实列表/缩略图 GUI、QQ 同目标粘贴闭环、Windows native、两设备、hosted CI及真实性能仍 UNKNOWN；headless fixture 和组件测试不能代替现场验收。

**本批停止在 S2-03。下一批为 S2-04 的版本化文稿 DTO/资产通道，须用户继续授权；本轮没有实现完整文稿 renderer 或 QQ 特例。**
