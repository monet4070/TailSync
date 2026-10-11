# S2-04：文稿预览后端与安全契约 — 实施及独立复审

日期：2026-10-07。本批仅完成 S2-04 的后端、二进制契约和客户端接缝；未推进 S2-05 renderer 或 S2-06 QQ。

## 1. 结论与执行流程

**S2-04：LOCAL_COMPLETE（有明确 partial 边界）；整个 S2 尚未完成；Release：NO-GO。**

- 复用一个内置 `cline-pass/deepseek-v4.1-flash` / high 子代理，完整明文任务、唯一绝对工作树、不继承历史；收到 ACK 并核对范围后才允许实施。
- 子代理 ID：`01a1149c-2fc6-7e43-8c11-70e800502158`。已等待完整 HANDOFF 并关闭，父代理随后独立审查、负向探针与二次修复，没有与子代理并发修改产品。
- 最终选定矩阵 **25/25 PASS**，全部绑定同一冻结产品源码。初始完整矩阵与强化后的独立 framing 重跑均保留，未覆盖任何失败。
- 文稿 DTO/二进制通道、两端生成契约、后端取消/身份复验与客户端解码已接通；这不是“完整文稿预览 UI 已完成”。
- native RTF/RTFD 逐 run 样式和表格结构仍为明确 partial；GUI/AX、QQ、Windows-native、两设备、hosted CI、真实性能/RSS 仍为 UNKNOWN。

## 2. 工作树、输入与增量

唯一工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，不是用户环境中另一份同名 checkout。

| 项目 | 绑定值 |
|---|---|
| 分支 / HEAD | `codex/composite-clipboard` / `f719d581a1aa5198a1b3f2483222df5c69e3dfc0` |
| 本批输入 | `62fddd236155a0ad84b0776719c65d68edf16eeca98279cffa2a98fd4b3bbb25`，812 文件 |
| 子代理交付 | `52b7592e8de58870e5deece7516f5fbb5cb1729f064718c1434ff76ce84d4d5f`，818 文件 |
| 父最终冻结 | `2a59599df107f4128126f434f24bafdf8832b7cae7a8958258feb2f6f9b0a2b4`，820 文件 |

产品指纹沿用 `scripts/composite-evidence.py`：计入未跟踪产品/测试/工具/配置，文档与诊断 artifacts 单独封存。独立探针及 semantic/headless runner 另有 `review/auxiliary-final-source.json` 字节清单，不能把它们冒充产品指纹内的文件。

| 增量 | 新增 | 修改 | 删除 | 总计 |
|---|---:|---:|---:|---:|
| before → agent | 6 | 29 | 0 | 35 |
| agent → final | 2 | 32 | 0 | 34 |
| before → final | 8 | 34 | 0 | 42 |

三组源码快照、精确增量 patch 和路径清单均保留。整棵既有 dirty 不是本批改动；没有 stage、commit、reset、clean、建分支或修改旧封存报告。

## 3. 父独立审查及二次修复

| 已确认问题 / 缺口 | 二修与证据 |
|---|---|
| 空表格 cell 被过滤，后续列左移 | 保留空 cell。独立 RED：期望 3 列，实际 2 列；最终原断言通过 |
| native UTF-16 逐 code unit 转字符损坏 emoji | 按 UTF-16 字符边界处理附件切分。RED：`中文��末尾��`，期望 `中文😀末尾🧪`；最终通过 |
| 色值 `#a💥b` 按 byte slice 导致 panic | ASCII 十六进制白名单与边界安全解析；无效样式显式 partial，中文/emoji 正文保留 |
| encoder 不校验身份/kind，Rust decoder 未完整校验资产 | encoder/decoder 均验证身份、版本、union、资产引用/长度/顺序、SHA-256 和 PNG 尺寸；截断、尾部字节及 orphan fail closed |
| generated decoder 只做表面字段校验 | generator 增加跨字段文稿语义、UTF-8 byte、累计 run/cell/pixel、首图与 orphan 校验；原 6 个语义探针有 4 FAIL，最终 6/6 PASS |
| 子代理把正文放在 metadata，metadata cap 为 4 MiB | 改成独立 control/model 分区；control ≤1 MiB，正文 JSON ≤4 MiB，不放宽旧 JSON line cap |
| owner/generation/cancel 未形成生产会话闭环 | 增加有界 `DocumentSession`、每 owner 高水位、close tombstone、validate/cancel 命令及真实客户端接缝；拒绝旧请求回放和迟到结果 |
| 文稿模型新增内存未计入 analysis retained bytes | 按 Vec/字符串保留容量计账，shrink 包含文稿模型，不只计算 PNG 长度 |
| Tauri 返回 Vec 时提前释放 producer lease | 自定义 `DocumentIpcResponse` 持 guard 至 `IpcResponse::body()`，再移交 Raw；没有谎称租约覆盖框架缓冲及前端 RSS |
| deadline 未覆盖初始 DB 等待，取消/编码所有权不完整 | 整请求 4 秒私有 cancellation scope、owned 编码 worker、64 MiB 输出 lease；初始锁等待、真实 worker 延迟退出、50 次释放循环有独立回归 |
| Windows 在 await 预安装复验期间 abort，仍返回文稿 | 独立 RED：5 个 client 测试中 1 FAIL，返回值实际成功；await 后再次检查 signal，最终 5/5 PASS |
| macOS 生产 reexport 漏接，父新增状态漏改测试构造 | 实际平台编译暴露并修正，最终 Mac Rust、Swift 和 Clippy 通过；不是只凭 registry 静态检查宣称接通 |

响应编码完成后也短锁复验 `document_key` 与 favorites 资格。收藏资格撤销的独立探针在补此显式后验前已经 PASS，说明既有分析 revision 已能拦截该用例；**它是额外防御，不算已复现的新漏洞**。history owner 可预览收藏条目，favorites owner 则必须核验收藏资格。

独立 framing 探针初版在 encoder 已拒绝坏资产时可能不执行 decoder 断言。父代理进一步用成功 encode/decode 的真实 PNG 帧改坏资产，再手工构造无声明尾部 payload，直接调用 decoder 验证拒绝，避免测试“空通过”。强化发生在初始独立门禁之后，故另用 `parent-final-independent-confirmed` 重跑；初始矩阵没有被重写。

## 4. 安全文稿模型与真实支持范围

`DisplayDocument` v1 仅定义受控 paragraph/run/basic style、基本 table rows/cells、image placement、partial 和内存资产描述；没有 raw HTML/RTF、URL/path/file/blob 渲染字段。不调用 HTML importer、危险 HTML renderer 或外部内容加载。

| 输入 / 能力 | 当前支持与降级 |
|---|---|
| HTML | 位置化正文、基本粗体/斜体/下划线/删除线/上下标、受控颜色、列表、基本表格、按正文顺序图片 |
| 纯文本 | 受控纯文字段落，不伪造样式 |
| native RTF/RTFD | 真实正文、UTF-16 附件位置、已保存的图片资产；中文/emoji 附件切分保留 |
| native 样式 / 表格 | **逐 run 样式、表格结构未实现**；`styles_partial=true`，明确 `NativeStylesUnavailable` / `NativeTableStructureUnavailable`，不将全局 style_bits 套整篇 |
| table 内图片 | 显式 partial flatten，保留正文顺序；不伪造 cell attachment 布局或跨列布局 |
| 外链 / 缺资源 / 首图不可用 | 准确 availability/placeholder；第一图缺失不让第二可用图冒充首图 |
| 脚本 / 活跃标签 / 不支持样式 | 不进入受控正文执行路径；局部 partial 或安全拒绝，无隐式网络/file fallback |
| 超限 | 截断、partial 或 PixelBudget 占位；不修改原件、不放宽既有准入预算 |

native 逐 run 样式/表格若要求完整保真，需要新的 helper facts 模式及协议扩展，当前 RTA1 全局 presence bits 不能提供该证据。本批没有静默增加 helper 协议，不能将此缺口写成“已支持”。

## 5. 二进制契约与兼容

帧：`TSDC | version=1:u8 | control_len:u32LE | control_json | model_json | png_assets`。

- control 仅身份、版本/长度等元数据；`model_bytes` 分隔模型与资产，`size_bytes` 为整个 body 长度。Rust 内部 `document` 字段不参与 control 序列化/schema。
- 资产仅引用本次已保存、已准入的字节，连续 offset、确定 index、明确尺寸与 SHA-256；三端摘要统一为 SHA-256。
- `get_document_preview` 走独立 binary 命令和显式 capability；Mac 私有 socket、两端 Tauri 注册以及生成客户端都有生产接线。
- Windows JSON socket 不声明文稿能力；Windows 文稿路径是 Tauri Raw IPC，不是假装 JSON socket 支持 binary。
- 原 TSPV 与 `text|image|file` 预览保留，旧严格 decoder 不被静默投喂 document；未知文稿/帧版本、kind、损坏资产 fail closed。
- 原 socket/PID/token 验证及 1 MiB JSON line cap 保留，文稿解析在已认证的既有路由后进行。没有明文临时文稿或全局 payload/asset 注册表。
- schema、Swift/TS/local decoder、两端 registry 与 handler 均从 generator 产生，未手改 generated 文件；最终契约检查为 schema v2 / wire v5、85 Windows / 49 Mac 已分类命令。

| 预算 | 最终边界 |
|---|---|
| 整帧 / control / model JSON | 16 MiB / 1 MiB / 4 MiB |
| 正文 / block / 累计 run | 1 MiB UTF-8 bytes / 8192 / 8192 |
| 累计 cell / asset / pixel | 2048 / 64 / **16,000,000** pixels，不是 16 Mi pixels |
| 既有 analysis input / workspace / output / pipeline | 48 / 192 / 64 / 256 MiB，均未放宽 |
| 准入 / deadline | 单文稿分析、整请求 4 秒（含初始 DB wait） |

## 6. 身份、取消、所有权与安装接缝

请求：`entry_id + owner(history|favorites) + generation + request_id`，request_id 为 1–128 UTF-8 bytes，generation 不超过 JS safe integer。每 owner generation 单调推进，cancel/close 保留 tombstone，bounded 会话只保留一份 active 小元数据，不留资产。

`dc1` stamp 绑定每次 DB open 随机 authority、行身份、加密 manifest 摘要、分类/分析/文稿/spec 版本；不使用 Arc 地址，不重新定义 data_hash。归档更新、删除、DB 实例替换均 fail closed。

生产顺序：短锁身份/owner 准入 → 锁外保存字节分析/helper/解码 → owned 编码 worker → 短锁 stamp/owner 后验 → session 当前请求复验 → transport handoff。预安装命令再次核对 session、stamp、DB authority/archive 与 owner 资格。

- drop future / timeout 取消私有 worker scope，不污染可复用 caller token。
- worker 忽略取消并延迟退出时仍保留其内存租约，真实退出才归还；独立探针实际观察 abort 后 ≥192 MiB reservation 保持，退出后回到基线。
- 输出对象持 64 MiB lease；Mac socket 保持 guard 至 binary write 结束。Tauri producer guard 至 IPC body handoff，随后 Raw Vec 交给框架。
- **Tauri 框架移交后的缓冲与前端持有内存不在该 Rust lease 的证明范围内**，需 S2-05 的 renderer 释放及原生 RSS 验收；不把局部 ownership 证明夸大为全链内存证明。
- Windows getter 解码、hash/尺寸验证、backend validate 后才返回，并在所有异步边界之后检查 abort；失败/abort 发 cancel。
- Swift getter返回身份核对后的 candidate，显式 validate/cancel helper 已提供并测试。当前没有 renderer 消费，S2-05 必须在实际安装前调用 validate，并核对本地 owner/generation/request nonce；不能提前宣称真实 renderer 安装复验完成。

## 7. 最终门禁、诊断与证据等级

证据目录：`artifacts/clipboard-improvement/2026-10-07-s2-04-document-contract-r1/`。

最终选定清单：`review/final-gates-confirmed-summary.json`，25/25；初始完整清单：`review/final-gates-summary.json`，25/25，原样保留。最终独立门禁引用强化重跑记录，其余 24 项均引用初始完整矩阵。全部产品 source before/after 均为父最终指纹，无修改中的诊断测试冒充最终验收。

| 门禁 | 最终结果 |
|---|---|
| Core default + test-support | 739 PASS，6 既有 ignored |
| Core all-features | 774 PASS，7 既有 ignored |
| Runtime all-features / 再显式 composite dependency | 各 96 PASS |
| Protocol / Windows helper crate | 63 / 3 PASS |
| 独立 Rust 文稿/framing/lifecycle 探针 | 16 PASS，包含真实资产破坏及无声明 payload 拒绝 |
| macOS Rust composite lib | 119 PASS，3 既有 ignored |
| Swift / React | 289 PASS、4 既有 skipped / 333 PASS、56 文件 |
| headless 保存字节 native helper | 5 个语义检查 PASS，绑定 helper/fixture SHA-256；不是 GUI/QQ 验收 |
| generated 文稿语义 | 6/6 PASS，非只有 exit 0 |
| TypeScript / workspace及Mac Clippy -D warnings / fmt | PASS |
| 契约 / canonical / production features / shared resolution | PASS |
| native policy / capture visibility / Windows-host / 输入封存审计 / diff | PASS |

数字来自完整 raw log 的真实 execution summary；组合与 suite 可重叠，不相加冒称唯一用例数。所有命令、环境、退出码、日志 hash、前后源码指纹和 test counts 可追溯。

Windows-host 仅 host fmt/check all-targets/test --no-run，4 种既有 cfg dead-code warning 文本保留，不宣称 Windows-native 编译运行/Clippy 通过。Swift headless 测试的系统服务连接诊断也完整保留，未通过删日志制造“零诊断”；本批不修无关系统服务问题。

保留的失败与证据不足单独分类：

- `parent-red-independent-r2`：4 PASS / 3 产品 FAIL（空 cell、emoji、色值 panic）；`parent-red-framing`：2 FAIL；`parent-red-generated`：6 语义中 4 FAIL；`parent-red-validation-abort`：4 PASS / 1 FAIL，均有 raw log。
- 首次 independent probe 的不存在 enum variant 是探针编译错误，不算产品漏洞；Mac 初次编译的 reexport 是产品漏接，settings test 初始化是父新增状态的配套遗漏，均区分记录。
- `parent-red-response-owner` 虽命名为 RED，但实际 1 PASS，不能声称该次已复现 owner 漏洞。
- 子代理 HANDOFF 的早期 RED/并行预算失败有部分仅回述，没有完整独占 raw log，因此不作为可追溯 RED 或最终矩阵证据；子代理聚焦 GREEN 和父原始 RED/最终门禁分开。
- 输入已存在的 `check-local-contracts.test.mjs` schema v1 期待与当前 schema v2 不符，本批未修改该测试，未将它纳入 PASS 矩阵或伪称仓库所有可能命令均通过。
- 生成器/fmt 的修改中命令有 source_stable=false 属预期操作记录，不归入最终冻结矩阵。

## 8. 保存、磁盘与停止条件

- 新 run 保存输入 dirty 状态、三个源码快照、子代理 assignment/HANDOFF、父二修 patch、全部命令/raw logs、强化探针清单、最终 acceptance/status、文档绑定与总 SHA-256 inventory；封存后只读复验，不重封旧 run。
- O05/S2-01/S2-02/S2-03 的旧 seal inventory/bytes 与绑定报告只读复验一致，分别 1228/1787/3681/3730 文件，未改旧报告或旧证据。
- 复用唯一 `target/o04-shared`；incremental/debug 关闭、jobs=2、Rust 串行。最终资源记录：target约5.2 GiB、新证据封存前约52 MiB、剩余约70 GiB；未创建另一套大型 target，未清理 App/测试包/旧证据/工作树。
- 相对本批输入不改生产依赖/lockfile、schema/migration、capture、原件/hash/dedupe、网络/认证或恢复原件语义。未读取用户剪贴板数据库/来源缓存、未操作 general clipboard/QQ，未替换或停止日常 App；真实无副作用仍需后续现场证据，不能仅凭单元结论证明。

**本批停止在 S2-04。下一阶段是 S2-05 两端受控文稿 renderer、真实安装复验与完整交互；必须另行授权。native 样式/表格 partial、Tauri 前端内存边界和真实设备/GUI 验收不得被后续自动门禁掩盖。**
