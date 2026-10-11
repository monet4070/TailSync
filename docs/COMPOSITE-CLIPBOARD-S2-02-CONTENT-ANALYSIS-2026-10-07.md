# S2-02：共享有界内容分析与 native-only 提取 — 实施及独立复审

日期：2026-10-07。范围仅 S2-02；未进入 S2-03 或后续阶段。

## 1. 本批结论

**共享分析服务、本地自动验证、父代理独立审查及二次修复已完成。不能据此宣布整个 S2 或发布验收完成。Release：NO-GO。**

- 复用一个内置 DeepSeek/high 子代理：`cline-pass/deepseek-v4.1-flash`，不继承历史，完整明文任务、任务 ID 和唯一工作树接收确认。
- 子代理完整交接并关闭之后，父代理才修改产品。子代理交付并非最终通过证据；本批所有最终门禁重新绑定父代理最终源码。
- 父代理独立探针先后复现 16 条真实失败断言，修复后保留原期待全部通过；两个附件坐标断言属于同一类根因，不冒称两个独立漏洞。
- 最终选定的 19 项门禁均通过，但一次完整 Core 出现 Iroh 实网连接超时。原失败保留，仅原样复测一次取得通过；**网络波动没有被修复，其本轮自然诱因仍 UNKNOWN。**
- 本批没有修改分类、缩略图 cache、对外预览命令、renderer、QQ 特例、schema、migration、依赖或 lockfile。

## 2. 工作树及源码绑定

唯一工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`。

| 项目 | 值 |
|---|---|
| 分支 | `codex/composite-clipboard` |
| HEAD | `f719d581a1aa5198a1b3f2483222df5c69e3dfc0` |
| 输入源码 | `7e321b376bab7c1831266cc601790a9d66c695a6d3f3c6e7fe8e708a8537e5c1`，801 文件 |
| 子代理交付 | `03adb74f0c629f43badb051dd8041940b591f80732549c8cc181ddc946e80885`，806 文件 |
| 父代理最终源码 | `3edd3c43cb887d31b5f7dbe2e8d0bb77ac4c631ad2e0e26447bb46ec0cad7367`，806 文件 |

指纹算法为字符串相对路径排序后，对 `path + NUL + content SHA256 + LF` 求 SHA256；不以 HEAD 或普通 Git diff 代替含未跟踪源码的输入/最终比较。报告及证据单独封存。

相对输入共修改 14 个产品/测试文件，其中 5 个新增；父代理相对子代理交付二次修改 9 个文件。

## 3. 已实现的生产接缝

`runtime::content_analysis::analyze_entry(_cancellable)` → 非阻塞并发及 pipeline 准入 → DB 锁内 `prepare_content_analysis` 取得有界 owned payload sources 和身份 → 放锁读取、解密、校验 → 受监督保存字节 helper → Core 分析 → DB 身份复验 → 返回 `AnalyzedContent`。

结果保存版本、entry/capture/content/archive identity、DB authority/revision、收藏 owner、选定表示及来源、选定 native item/kind/converter、实际正文、基本样式/表格事实、文档顺序图片位置和受限 owned PNG 资产。

- 表示优先级为 HTML、RTF、Text、PNG；native-only 不把保存格式摘要冒充正文。
- native 主表示按 item 顺序，同 item 优先 RTFD；不同 item 的样式、表格及图片不合并进主正文。其余 item 的边界保留并反馈。
- HTML 图片按节点顺序和精确 binding 范围定位。重复引用保留位置；第一图缺资源/无 src，仍定位第一图，不换第二图。独立 PNG 不抢文稿图片。
- HTML 图片坐标是 UTF-8 字节范围；缺 src 的图定位整个元素。RTF/RTFD 附件坐标是保留 U+FFFC 的正文 UTF-16 code-unit 位置；纯 PNG 使用整表示坐标。
- 可用资产保留在对应 placement 中；通过编码长度、累计像素、静态 PNG及完整 PNG 解码校验后才标 Available。不是只留下可用标志后丢弃图片。
- archive overlay 先验证原 base 的 `data_hash`/content identity，再验证 overlay derivation；不能拿补图层摘要冒充原件摘要。原件、dedupe、收藏、source 和恢复语义不变。
- 原件/派生来源无法证明时使用 `Unverified`，不根据同名格式或 converter 自报猜测原件关系。

这里提供的是展示分析事实及资产，**不是完整 renderer block/run 文档模型**。三个后续消费者尚未接入；S2-03/04/05 必须消费这一版本化服务，不能宣称本批已经完成 UI 接线。

## 4. 独立复审：真实 RED → 修复 → GREEN

| 问题 | 二次修复 |
|---|---|
| HTML 数字/命名实体成为原始编码字符串 | 解析基本命名实体及合法 Unicode 数字实体；未知实体反馈不支持 |
| 行内节点间空格丢失，Hello world 变成 Helloworld | 跨文本节点维护待写空格，而非逐节点 trim |
| br 等 void 元素累计深度，100 行正文被截断 | void/self-closing 元素不入深度栈 |
| 第一 img 无 src 被遗漏，第二图冒充第一图 | 遍历每个图节点并保存 MissingSource placement |
| script 字符串生成虚假的粗体和表格事实 | raw-text 区间不参与正文/样式/表格分析 |
| native item 1 的附件/样式混入 item 0 | 明确一个主 native 表示，不跨 item 聚合 |
| 非 RTF 字节被当作完整正文 | 校验 RTF 头、根组及平衡，坏数据 fail closed |
| RTF surrogate pair emoji 变成问号 | 合并 UTF-16 代理对，维护组作用域 Unicode fallback |
| RTA1 接受越界附件位置 999 | 校验位置对应 U+FFFC、严格顺序、状态与长度 |
| abort 等 DB 锁的 future 后，真实 worker 长期占准入 | 私有 worker Cancellation 同时取消锁等待及 helper；lease 随真实 worker 退出释放 |
| 合法归档补图层被误报原件 digest mismatch | 原摘要校验移至 base，保留原内容身份和 overlay 证明 |
| 删除 U+FFFC 导致双附件/重复图 UTF-16 位置错位 | 保留完整 rich.string 和原 range.location；坐标与正文统一 |
| cellx 被当作 cell，1 行 2 cells 误报 4 cells | 完整 control-word 匹配，并只扫描实际 RTF component |
| 未知 RTF control 被冒称完整分析 | 明确支持子集，未知 control 标 partial，保留可读正文 |
| 超范围 RTF Unicode 参数被截成另一合法字符 | 准入 Unicode 参数范围，不生成伪字符 |

早期 Core/runtime 独立探针为 0/10，归档生产反例再增加 1 条失败；原生探针校正 checker 后为 6/9；追加 RTF control/Unicode 探针新增 2 条失败。最终外部生产接缝探针为 13/13、原生纯 stdin 合成探针为 9/9。

### 额外代码审计修复

- 补入全局 256 MiB pipeline ledger：workspace lease 移入真实 worker，输出缩减后的 lease 留在 `AnalyzedContent` 生命周期内。
- caller Cancellation 不再被正常完成/drop 的 guard 污染；私有取消与 deadline watcher 同时通知 helper 和 DB 锁等待。能力 self-check 同样经过 cancellable supervisor。
- 返回对象保留 opaque `AnalysisHandle`/revision，因此 live 结果的 authority Arc 不会释放后发生地址复用；提供 `is_current` 再验，不只依靠一个裸 authority 字符串。
- 核对 helper item/kind/source/converter，拒绝跨 item 自报；限制多 item 的临时复制和累计输出。
- HTML 改为有界流式事实扫描，不先建立无限 reference inventory 再截断；RTFD 在 AppKit importer 前拒绝目录/symlink 附件包装。
- 补入累计像素、完整 PNG、输出资产以及重复附件计账；无任意路径、URL、缓存或 general clipboard 读取。
- 修正新增单字母变量和新增代码注释。没有新增 allow/skip/ignore，也未通过修改既有网络测试逃避失败。

## 5. 预算与生命周期边界

| 项目 | 本批上限 |
|---|---|
| owned 输入总声明量 | 48 MiB，SQL materialization 前核对存储长度 |
| workspace reservation | 192 MiB，复用既有 256 MiB ledger，非阻塞压力拒绝 |
| 表示输入 | min(既有 MIME limit, 16 MiB)；HTML 2 MiB、RTF 4 MiB、Text 1 MiB不提高 |
| 正文 | 1 MiB UTF-8 |
| 展示输出 | 16 MiB；资产另留正文及 metadata 余量 |
| PNG 资产 | 每图 4 MiB、至多 20 个，累计 decoded pixels ≤8,388,608 |
| 节点/格式化控制、block | 8,192 |
| 表格 cells、协议 rows | 2,048 |
| HTML/RTF 深度 | 64 |
| 并发 | 1，无无界等待队列 |
| 逻辑 deadline/helper watchdog | 4 秒；取消走真实 kill/reap/join，不以丢弃 future 伪装结束 |

输入、helper、解析和复验共享 deadline；返回等待实际 worker/child 收尾，不能宣称整个 API 在任意 OS 调度/管道收尾情况下都精确于 4 秒返回。

lease 是自有有界缓冲的保守 reservation，**不是 AppKit 进程 RSS 的 OS 硬限制**；真实原生内存峰值/性能仍需现场证据。自定义 `NativeAnalyzer` 实现也必须遵守取消约定，不能从 trait 签名推导出任意第三方实现必然可杀。

`generation` 是 DB read revision，`owner` 是 history/favorites；不是 UI 请求 generation/预览 owner。后续消费者安装前仍需自己检查请求身份并调用 `is_current`，不得把本批数据库再验冒充 GUI 迟到结果闭环。

## 6. 最终验证与失败保留

所有选定门禁绑定最终源码 `3edd3c43...`，source before/after 一致。

| 验证 | 结果 |
|---|---|
| 完整 Core，含 test-support 所需 examples/integration | 原样复测退出 0；库测试 705 passed / 0 failed / 4 既有 ignored |
| Runtime 完整 suite | 69 passed；含本批 16 项生产接缝测试 |
| Protocol 完整 suite | 63 passed；含本批 12 项分析帧/RTF测试 |
| Helper suite | 3 passed |
| 外部独立 Core/runtime 探针 | 13 passed |
| Mac 原生 helper 纯保存字节探针 | 9 passed，单独解析语义断言，不当作普通 runner 的 0 tests |
| workspace all-targets/all-features Clippy、Mac feature Clippy | `-D warnings`，通过 |
| fmt、contracts、canonical、features、shared resolution、policy、capture visibility、RustSec exceptions、diff | 通过 |
| Windows host 检查 | 通过；保留 4 条既有 cfg dead-code warning，不能冒充 Windows 原生验收 |
| 输入基线及旧 O05/S2-01 r2 seal、绑定文档 | 逐文件复算通过，未修改旧证据或重封旧批次 |

Core 全量 raw log 存在子进程输出交错。通用解析器汇总 708 passed/6 ignored 包含 worker 输出，不当作去重后的 708 个唯一用例；主库 705/0/4 的完整 suite 行可直接核对。既有隔离/进程 worker 由父测试调用，未增加任何 ignored；feature-gated 后续回归矩阵未启用，不计入本批通过证据。

必须保留并区分的失败：

1. 完整 Core 首个裸命令因既有 examples 需要 `test-support` 编译失败；改用正确测试 feature，不改 examples/生产配置。
2. 正确完整 Core 首次为 704 passed/1 failed/4 ignored：`real_network_pairing_completes_over_a_real_iroh_stream`，Iroh connection timed out。相同源码、参数、隔离器仅复测一次通过。已有同类诊断记录，但本轮自然原因仍 UNKNOWN，不能宣称网络问题修复。
3. 父修复过程有 SQL FromSql 类型、测试借用、收藏保护及 Clippy 编译错误；均保存失败日志并用新 ID重跑，没有覆盖日志。
4. 原生超限输入早期 checker 只接受 exit 2，实际 helper 正确 exit 65：这是 checker 假阳性。保留 v1，修正接受明确拒绝码；并未降低正文/附件期待。
5. 首个归档 fixture 手构不合法，未计入产品缺陷；生产 `apply_resource_bindings` 构造的合法 fixture 才用于实际 RED。
6. 子代理和早期 codec 测试的正文缺 U+FFFC/RTF 图片组未闭合，修正合成输入为合法数据，未放宽业务断言。子代理 mutation RED 是事后敏感性测试，不混称开工前失败。

## 7. 证据与二次复查入口

证据目录：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/artifacts/clipboard-improvement/2026-10-07-s2-02-content-analysis-r1`。

- `agent/ACK.md`、`agent/HANDOFF.md`、`delegation.json`：派发、完整交接和关闭边界。
- `review/input-to-final.patch`：完整输入到最终增量，含新增文件内容；`review/agent-to-parent-final.patch`：父二次修改。
- `source-before.json`、`review/source-agent-frozen.json`、`source-parent-final-r2.json` 及各源码备份：可逐文件复算。
- `review/final-acceptance.json`：选定 19 项最终检查；失败的 `final-gates-summary-r2.json` 原样保留，不篡改为全绿。
- `commands/`、`logs/`、`sources/`：真实命令、退出码、raw logs及对应指纹；所有 RED 和重跑保留。
- `evidence-sha256.json`：本批独立 seal；`review/verify-seal.py.txt`：只读库存、逐文件 hash、aggregate、源码、文档、日志及门禁绑定复验。

优先复查：取消后真实 worker admission、输出 lease/authority 生命周期、补图原身份保持、U+FFFC/UTF-16坐标、首图缺失不换图以及 provenance 保守性。

## 8. 未完成与停止边界

**UNKNOWN**：运行 App/GUI/AX、真实 QQ/目标应用、Windows 原生 RTF/helper 与双设备、hosted CI、性能/RSS现场、当前 Iroh 自然超时诱因。Windows RTFD在非 AppKit 纯路径明确 unsupported，而非“已支持”。

**尚未实施**：S2-03 展示分类/首图/cache；S2-04 预览后端及二进制契约；S2-05 renderer；S2-06 QQ 场景。整个 S2 未完成。

旧 S2 方案和 S2-01 报告属于已绑定历史证据，本批不改写其状态；以本报告新增实施证据。未 stage、commit、reset、clean 或创建分支，保留既有 dirty 工作、日常 App、用户数据库及测试包，未清理构建缓存。到此停止，等待下一阶段授权。
