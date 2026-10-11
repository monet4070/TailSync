# 剪贴板改进逐项执行记录

日期：2026-10-06，Asia/Shanghai。

依据：[反馈与调研汇总](COMPOSITE-CLIPBOARD-FEEDBACK-AND-RESEARCH-2026-10-06.md)、[逐项改进方案](COMPOSITE-CLIPBOARD-IMPROVEMENT-PLAN-2026-10-06.md)。

用户已授权逐项实施，并要求每完成一项任务就暂停汇报。本记录按任务追加；S0、O11-D1 和 O11-D2 分别完成后暂停。设计模型、生产接缝回归与原生/设备验收分开记录。

## 1. 当前进度

| 任务 | 状态 | 本轮结果 |
|---|---|---|
| S0 基线与问题复现 | 本机自动部分完成；已暂停 | 固定源码与封存包，完成五类缺口的可执行对照；真实用户现场单列待验证 |
| S1 首项：O11-D1 身份、去重与版本兼容设计 | 已完成；已暂停 | ADR-005 确定字段与规则；26 项设计模型检查通过；生产接线未开始 |
| O11-D2 存储、去重及重试生产接线 | 本机实现与自动验证完成；已暂停 | 三次 B02 重放均保留两份不同原件；v13、Manifest 1/2、安全重试和独立回退已接线；原生完整枚举及设备验收仍开放 |
| O01 原件与派生表示隔离 | 本机实现与自动验证完成；已暂停 | 原始 HTML/text/RTFD/TIFF 与派生格式独立；新包七组私有往返通过，见第 9 节 |
| O02 原生字节准入与敏感采集阻止 | 本机实现与自动验证完成；已暂停 | 有界不透明字节、元数据隐私门、provider 期限和精确原集合来源分类；见第 10 节 |
| O06 准备恢复计划，再集中发布 | 本机实现与合成自动验证完成；已暂停 | RestorePlan 1、资源拒绝、单次发布、FIFO/取消与菜单一致；新包原生证明见第 11 节 |
| 审查整改 | 本机实现与自动验证完成 | 无用摘要/旧接口、HTML 逐节点检查、解析复用与生产 feature 门禁，见第 12 节 |
| O10 因果语义修正 | 本机实现与自动验证完成；已暂停 | revision 关联、来源优先、准备前准入及保存/读取/同步提示分离，见第 13 节 |
| DeepSeek 全阶段复核整改 | 本机实现与自动验证完成；已暂停 | 成立项修正、采样准入澄清与接缝回归，见第 14 节 |
| O03 原始图片与动画 | 本机实现与自动验证完成；已暂停 | 原件／封面分离、容器预算与摘要兼容，见第 15 节；完整 A12 仍 UNKNOWN |
| 最新审查发现核实与修复 | LOCAL_COMPLETE；已暂停 | P1-A 误判更正、P1-B 合同符合；DeepSeek/high 修复成立项，主代理补修并通过 23 门禁，见第 16 节 |
| F01—F06 用户可见修复 | 提示归因部分随 O10 完成；体验任务待实施 | 分类、预览、首图、菜单及真实应用效果仍为后续任务 |
| S3—S5 资源、设备与发布 | 未开始 | Windows/双设备及发布保持原边界 |

**截至 O11-D1，没有新增 UI 按钮或产品功能，没有替换测试 App。** S0 建立失败证据，O11-D1 确定并验证修复规则；实际功能修复从生产接线任务开始。

**截至 O11-D2，生产存储与恢复重试已改变，现有测试 App 保留。** 下一项是 O01 原件/派生隔离，尚未启动；新源码没有本轮安装包、QQ/目标应用或双设备验收，发布仍 NO-GO。

## 2. S0：固定基线

| 项目 | 实际结果 |
|---|---|
| 工作树 | /Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35 |
| HEAD | f719d581a1aa5198a1b3f2483222df5c69e3dfc0；存在既有未提交改动 |
| 当前产品/测试/工具指纹 | cd9268e75f38d52431f985ef8dadd8c4c910eee3b09f638d289fa71a007ec1eb，756 文件 |
| 选定候选 | 封存 R6 的 TailSync-enabled-universal-apple-darwin.app，2.3.2，arm64/x86_64 |
| 包指纹 | 730b3f89e8b52b1d5653ad638d4dbb3fa1dccb0da513c9310b526c1122f4f7b7 |
| 包绑定核验 | 全部包内文件摘要与既有 manifest 相同；签名验证、helper 自检、生产 resolver 本轮执行成功 |
| 运行配置 | 包内 helper 直接调用私有 board；Core 诊断使用当前源码编译，独立数据根和显式 CompositeAllowed |
| 使用限制 | 封存候选不等于已独立确认用户当时运行的 2026-10-05-225456-composite 副本；本轮不继承全部 R6/R7 PASS |
| 系统与应用 | macOS/架构原始命令结果及 QQ、TextEdit、Preview、Edge 已安装版本记录在 baseline.json |
| 系统剪贴板 | 所有写入限定 tailsync-probe.s0.* 或既有 self-test 私有 board；未读取 general 正文、未写入 general |
| GUI/端口 | 开始时 19890 空闲；未启动完整 App 或 daemon 服务，未操作 QQ 会话 |

本轮保存完整源码指纹、候选文件摘要、原始命令输出、合成输入和观察 JSON。诊断 crate 位于 artifacts，不参与产品源码指纹；数据读写通过现有 run-isolated-tests.mjs 隔离。

## 3. 实际复现结果与对应优化

### B01：来源 HTML/文字被转换结果替换

- 输入：同一原生项目包含 RTFD（中文、图片附件）、不同于 RTFD 派生结果的原始 HTML 和文字。两组 RTFD 只改变附件文件名。
- 路径：封存包 helper 的 write-snapshot → 私有 board → read-snapshot。
- 结果：两组各重复三次，**6/6 未保留原 HTML 和文字**；6/6 原始 RTFD 字节仍一致。
- 判断：确认的是“快照混用了派生表示并替换原始表示”，不能描述为全部原始内容丢失。
- 改进：O01 将原始格式与派生 HTML/text/PNG 分开，默认回放选择明确的集合；O11 先设计兼容存储和身份。
- 对应后续验收：A09；本轮只提供失败基线，未完成整个验收用例。

### B02：不同原件因便携摘要相同被合并

- 输入：B01 两个合法 RTFD 原件的附件文件名不同；由真实 helper 生成的便携 HTML/text/图片一致。
- 路径：from_native → add_composite（含 native）→ 关闭/重新打开隔离库 → get_all/get_composite_native。
- 结果：三次独立库中，均确认原始 RTFD 不同、便携摘要相同；第二条保存后仅剩一条记录，第一条无法读取。
- 判断：**3/3 复现原件集合被便携去重合并**。这不是用户删除，也不是便携内容发生差异。
- 改进：O11 区分事件身份、原始集合摘要和派生/归档版本摘要；原生原件不同不能仅按便携摘要删除旧历史。
- 对应后续验收：A21。此项决定先做身份与兼容设计，避免后面扩大保存范围后仍被错误去重。

### B03：原生附件保存着，但展示元数据没有图片

- 输入：带一张 PNG 附件的合成 RTFD 原件；未带 rtfd-converted 标记。
- 路径：from_native 返回 NativeOnly → add_native_snapshot → 重新打开隔离库 → get_all/get_composite_native。
- 结果：RTFD 字节保存并重启读取一致；记录为 category=text、image_count=0，Swift hasThumbnail 规则据此不会请求图片。
- 判断：确认 native-only 存储/展示接缝的缺口。**未经过 QQ 复制，也未证明 QQ 的问题就是这条路径。**
- 改进：O01/O02 保留原件后派生实际内容元数据；F04/F06 从原生附件取得预览和首图，不能只看便携 image_count。
- 对应后续验收：A07/A08/A09；真实 QQ 仍待验证。

### B04：缩略图受资源存储顺序影响

- 输入：正文第一图红色、第二图蓝色的合法 HTML；只反转 bundle 的资源存储顺序，资源 ID、绑定和正文顺序不变。
- 路径：from_native → bundle.validate → add_composite → get_composite_image。
- 结果：保存内容有效，但缩略图选中蓝图，RGBA 为 [0,0,255,255]；正确首图应为红图 [255,0,0,128]。
- 判断：确认选图依赖存储顺序，未按正文位置选择。
- 改进：F06 保存/解析正文节点或附件位置，缩略图选择正文第一张可用图片；资源去重与存储顺序不能决定展示顺序。
- 对应后续验收：A08。本轮没有执行 GUI 视觉验收。

### B05：不透明原生类型在数据库环节被过滤

- 输入：普通文字＋org.tailsync.fixture.custom-format 合成字节格式。
- 路径：Core 的 NativeSnapshot → from_native → add_composite → get_composite_native。
- 结果：存回的原生集合只剩 public.utf8-plain-text，自定义类型被丢弃。
- 判断：确认数据库固定白名单的限制。输入没有经过 OS provider，不能宣称已经测试任意私有应用格式或完整采集链。
- 改进：O02 贯通 helper、DB、回放的准入规则；有界保存适合持久化的原始字节，路径和对象依赖进入相应适配。
- 对应后续验收：A10/A11。

### 两项对照与 F01 边界

1. 单 PNG → 生产 Core/DB：主 category=image，image_count=1，读取的 RGBA 与输入相同。该简单样本**没有复现主类别被归为文字**；但 categories 同时包含 text/image，摘要为 Rich clipboard content，属于后续语义展示审查点。
2. native-only 原始 RTFD 经存库/重新打开后字节一致。B03 是展示识别不足，不能据此写成附件已经丢失。

包内 helper 的现有 self-test 本轮执行成功：五组私有 board 夹具，以及过期 revision、坏输入不清空等既有断言。该控制不覆盖所有新要求。

## 4. 提示、真实来源与待验证条件

| 项目 | 当前证据 | 下一步条件 |
|---|---|---|
| F01 用户所见单图“富文本”标签 | 单 PNG 主分类正确；合成 metadata 存在混合标签及 Rich 摘要 | 绑定实际包、来源应用和对应界面字段后复现 |
| F01 用户所见系统提示 | 未取得提示原文、发出方、操作时间；不能归因 | 需对应提示及触发动作；本轮没有关闭权限或通知来绕过问题 |
| 原生成功后 capture_failed 分支 | clipboard.rs:395 的代码事实；本轮未启动通知消费链 | O10 后续修复/回归独立安排；不等于 F01 提示根因已确认 |
| F05 QQ | QQ 6.9.79 / 36352 已安装；没有读取会话或现有消息 | 合成/允许的 QQ 样本、复制动作及同目标直接粘贴对照 |
| 实际目标应用 | 本轮没有做原生粘贴 | TextEdit/Preview/浏览器等真实目标分层验收 |
| Windows / 双设备 | 没有本轮原生或设备证据 | 条件具备后执行对应矩阵；保持 UNKNOWN |

S0 退出门槛允许无法取得的现场条件明确待验证。本轮完成的是本机可自动做的基线与接缝复现，不把现场缺口判 PASS，也不阻塞之后独立的原件/身份修复。

## 5. 命令、结果与证据

证据目录：`artifacts/clipboard-improvement/2026-10-06-s0-baseline/`。

| 文件 | 内容 |
|---|---|
| baseline.json / source-fingerprint.json | 候选、应用版本、源码及全部文件摘要 |
| prepare.py / fixture-generator.swift | 元数据核验与合成私有 board 复现，保留完整代码 |
| helper-results.json / logs/prepare-command.json | 原 HTML/text 保留断言六次 FAIL；命令 exit 1 |
| core/src/main.rs / core/Cargo.toml.txt / core/Cargo.lock | 引用实际 Core 的诊断；Cargo manifest 原文已归档，见下方维护说明 |
| core-results.json / logs/core-command.json | 六项断言：2 PASS / 4 FAIL；命令 exit 1 |
| logs/*.stdout / *.stderr | 自检、resolver、签名、helper 输出及编译运行原始结果 |
| fixtures/* | 仅合成 RTFD/快照/PNG/HTML，不含用户正文 |
| acceptance-matrix.json | 全部 A01—A21 条件；验收状态仍 NOT_RUN，诊断结果不能顶替通过 |
| verification.json / evidence-sha256.json | 最终核对与证据文件完整性索引 |

退出码 1 保留为真实失败基线，不修改断言或掩盖成产品 PASS。Core 编译成功，四项失败来自上述目标行为断言，而非编译失败。

复验时使用新的证据目录复制这组诊断，把 Cargo manifest 从文本归档物化到工作树外的临时 crate，并绑定当前 Core 绝对路径；重新绑定实际候选和源码，不能覆盖本目录的已记录结果。产品修改后，需要将选定样本提升为正式生产接缝回归，并执行该项所需检查。

## 6. 本项结束与下一项

S0 本机自动部分已完成；此处保留 S0 完成时的检查点。下一项 O11 的身份、摘要、存储版本和兼容设计现已完成，见下节；O01 原件/派生隔离及 O02 保留范围仍未实施。

本轮效果是确定五类可重复缺口及其修复接缝；现有 UI、正常复制/粘贴和历史恢复行为没有因本轮改变。发布仍为 NO-GO。

## 7. O11-D1：内容身份、去重与版本兼容设计

### 7.1 已完成的优化设计

详细设计：[ADR-005](adr/ADR-005-composite-content-identity.md)。

| 决策 | 解决的问题 |
|---|---|
| 分开 capture、原件、依赖去重键和归档版本 | 相同便携内容不再被当成原件相同；新复制和旧事件重传不混淆 |
| 原件摘要保留平台、项目/类型顺序、名称、读取状态和字节 | RTFD 文件名、真实格式、空白或项目边界不同，都保留差异 |
| 完整原件＋采集时依赖闭合才提供去重键 | 同 URL/路径、缺失格式或未知类型不能凭字符串删除旧记录 |
| 旧 data_hash 与网络/journal 摘要保留 | 历史校验、旧事件回执不因替换摘要算法而失效 |
| 重试跨行匹配只用安全、同域同版本的去重键 | 两份原件仅便携内容相同，不能“再次复制”到另一份原件 |
| 补图绑定 capture/原件/归档版本并事务核对 | 不更新别的记录，不覆盖新版本，不复活已删除历史 |
| 独立新版本准入与回退 | 旧数据可读；新原件采集能力与健康状态明确；升级前载荷完整备份 |

确定的后续版本目标：历史 schema 13、Manifest 2、helper health 3、新 CNA1 archive read、local contract 2、独立 pre-v13 回退包。现有 CNS1 发布接口、网络消息/CBP1、journal 和来源指纹继续沿用。本轮没有修改任何生产版本常量。

### 7.2 实际验证及效果

证据目录：`artifacts/clipboard-improvement/2026-10-06-o11-design/`。

- 规则模型调用当前 Core 的 NativeSnapshot/from_native，输入来自已封存 S0 合成样本。
- 最终 `cargo test --offline --locked` 实际运行 **26 tests：26 PASS / 0 FAIL / 0 ignored / 0 filtered**；零个 doc-test 不计入执行数量。
- S0 两份 RTFD 的便携摘要仍相同，但新模型中的原件 ID 和去重键不同，模型保留两份身份。
- 缺失读取、未知平台、旧投影、未闭合依赖均没有安全去重键；同引用不同字节的键不同。
- 迟到版本、已删除行、错误 capture/原件无法提交；补资源后原件/capture/旧 hash 保持不变。
- 当前 CBP1 WireDigest 与实际编码摘要一致，模型没有改写它。
- 最终模型源文件和 manifest 摘要绑定日志；初版模型运行日志另保留，不混为最终证据。

这是设计规则验证，不是生产 DB 修复。S0 的真实数据库 B02 仍是失败基线；源码指纹仍为 cd9268e75f38d52431f985ef8dadd8c4c910eee3b09f638d289fa71a007ec1eb（756 文件）。A09/A14/A21 的完整验收未标 PASS。

### 7.3 发现并修正的诊断收尾问题

S0 留下的 standalone core/Cargo.toml 含正常依赖 test-support，被生产 feature 扫描识别。这是上一项遗漏的诊断清理，本轮已将它以完全相同字节归档为 Cargo.toml.txt；原始 seal 不重写，新增 manifest-archive-location.json 记录路径映射，原 90 个文件的内容摘要仍可逐一验证。

新模型在树外临时 crate 运行，不启用 test-support。临时模型 crate 的生产 feature 检查通过，退出后临时目录删除。

全仓 `check-production-features.mjs .` 仍 **FAIL**：原有 shared/rust-core 与 shared/tailsync-runtime 的 `repair-regressions = ["test-support"]` 声明被规则拒绝。它们在 S0 前就存在；本轮已移除诊断 manifest 引入的第三项违规，没有放宽检查或顺带改产品。两项原有声明须在后续门禁整理任务处理，不能宣布整个仓库门禁通过；本轮也不据此断言实际生产二进制启用了 test-support。

### 7.4 暂停点与下一项

本节保留 O11-D1 完成时的暂停点：当时 **O11 整项尚未完成**，下一项是 O11-D2 的生产接线与迁移/回退验证。现已完成的 O11-D2 见第 8 节；完整原件采集与资源/设备门槛仍在后续阶段。

本项效果是把“该比较哪些内容、旧数据怎么保全、迟到任务如何拒绝”确定为可执行规则。用户可见复制/粘贴和 UI 没有改变，发布仍 NO-GO。

## 8. O11-D2：存储、去重与恢复重试生产接线

### 8.1 实施与实际效果

本项继续使用 `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，分支 `codex/composite-clipboard`、HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。保留既有未提交改动；未暂存、提交、替换测试 App 或修改日常数据。测试经隔离数据根运行；本项没有写入 general 剪贴板。

| 已接入生产的变化 | 效果与范围 |
|---|---|
| Core HistoryIdentity 与条件 schema v13 | capture、带域内容身份、安全去重键、归档版本/摘要分别保存；保留旧 data_hash 与网络/journal/来源摘要算法 |
| Manifest 1/2 版本分派和身份关系核验 | 旧 Manifest 1 与载荷原字节保留；新 Manifest 2 和关系表在同一事务提交，读取时核对摘要及身份 |
| composite 专用安全去重查询 | 不按相同便携 hash 删除不同原件；收藏/protected 保留；新 External 事件及远程事件幂等仍走既有因果链 |
| degraded 搬运按 capture 身份核对 | 不合并便携相同的不同原件；同 capture 归档冲突拒绝，保留两侧库；目标版本达到来源版本 |
| 存储搬迁保留 schema | 修复本轮实测的维护锁内再次升级导致的死锁；搬迁不在持有该锁时触发升级 |
| 归档摘要 CAS | 核对 capture、内容、原版本/摘要后更新；拒绝迟到、删后、错 capture/原件结果。资源载荷替换与 lease 尚属后续任务 |
| Swift/React 和 daemon 的恢复重试 | 跨行只允许同域同版本安全键匹配，移除 data_hash 回退；daemon 在准备 payload 的 DB 锁内复核持有身份；格式选择及显式新 intent 保留 |
| local contract 2 统一生成 | Rust→schema/Swift/TS/decoder/fixtures 配套；stable error contract 保持 1；保留 additive-field 兼容，身份版本比较仍拒绝不兼容版本 |
| 独立 pre-v13 回退包与恢复脚本 | v12 一致快照、live composite 外置载荷及清单齐备；验证路径、摘要算法、长度、权限和完整文件树；只恢复到新目录，升级后数据另行保全 |

S0 B02 的原始 helper 合成结果在实际 Core/DB 重放三次：原件仍不同、便携摘要仍相同，每次关闭/重开后都保留 **2 条**，第一份原件仍可读。S0 基线为 **3/3 仅剩 1 条**；正式生产回归先 RED（期望 2、实际 1），接线后 GREEN。

### 8.2 安全去重的分阶段准入

当前 CNS1/helper protocol=2 仍为 **LegacyProjection**，保存的原生集合只有 hcl1 身份，没有安全原生去重键。相同原生内容的新复制可能保留多行；这是未完成原始完整枚举时的保全规则。不能因 schema=13 或模型已有 Exact 规则就将旧采集标为 Exact；O01 的 CNA1/protocol=3、原件/派生角色与完整枚举尚未实施。

便携 hpd1 安全键保守限定于完整 text/PNG 与仅含已知、无属性格式标签的 HTML。RTF、图片 HTML、任意属性、provider、对象或未知依赖均没有安全键，等待 O04 的资源闭合证明。后续补图不回溯删除采集历史。

S0 的 native-only 附件展示、正文首图选择、不透明格式保留仍是失败样本；本项未把这些后续目标改为 PASS。B01 的原 HTML/text 替换也仍待 O01。

### 8.3 检查与证据

证据目录：`artifacts/clipboard-improvement/2026-10-06-o11-storage/`。`validation-results.json` 登记实际命令、退出码、原始日志、计数、失败历史与绑定范围；不将重试结果覆盖首轮失败。

| 检查 | 实际结果 | 主要日志 |
|---|---|---|
| 最终 DB 接缝 | 135 PASS，2 既有 ignored | db-verified.log |
| 标准 enabled Core 功能集合，串行全库测试 | 623 PASS，4 既有 ignored；含两项 real_network 测试 | core-enabled-verified.log |
| Runtime 库与 repair 集成 | 25＋11 PASS | runtime-frozen.log |
| macOS Rust | 117 PASS，3 ignored | macos-tests.log |
| Windows Rust，macOS 宿主 | 126 PASS，2 ignored；有既有 dead_code 警告；不代表 Windows 原生 | windows-host-tests.log |
| Swift | 269 项中 265 PASS、4 skipped、0 FAIL | swift-frozen.log |
| Windows UI | 51 文件、275 PASS | windows-verified.log |
| Windows UI 构建 | PASS | windows-build-frozen.log |
| Workspace all-targets/all-features Clippy，-D warnings | PASS | clippy-verified.log |
| 生成契约与 production decoder 检查 | PASS，schema 2 / wire 5；77 Windows、46 macOS 命令 | contracts-additive.log / contracts-additive-check.log |
| 独立 v12 生产 reader→v13 writer→新目录回退 | PASS；3 条旧历史、原件/载荷摘要及收藏一致；新增 v13 数据另行保全 | rollback-verified/proof.json |
| rustfmt、回退脚本语法、diff 检查 | PASS | fmt-verified.log / rollback-*-syntax.log / diff-check.log |

新增 10 项 O11 DB 回归和 1 项 daemon 身份锁回归，覆盖不同原件、收藏/protected、未闭合依赖、CAS、删除、关系损坏、degraded 冲突、旧字节、升级失败重试及回退包损坏。SQLITE_FULL 测试用 SQLite max_page_count 实际触发页耗尽，并验证 v13 DDL/版本回滚；另以 SQLite trigger 拒绝 v13 schema marker，验证事务失败后旧内容可读及升级可重试。没有单独注入 COMMIT/文件系统 fsync 失败，也没有执行物理满盘或断电。

独立回退使用未改动的 R0 封存 **schema12 生产源码**创建旧库与旧 reader，当前源码 writer 升级到 v13，再恢复 pre-v13 包到新目录交给旧 reader。3 个外置 payload 被核验；“只复制 DB、漏载荷”和“旧 reader 打开 v13”两项负向路径均退出 1，符合预期。proof 保存编译前后当前 shared Rust/TOML 的 152 个摘要，全部与最终源码一致；临时 harness 在树外构建，manifest/lock 以文本保留。

最终产品/测试/工具源码指纹：`bd25ffe0d1ce2d67c54262ad3b82af426e8af3194603954cea65bdc325265166`，**759 文件**。完整库存见 `source-fingerprint.json`。Markdown 文档与其他证据按该指纹工具的既有规则另封存。早期平台/Swift/Runtime 检查的时间和绑定范围单列，不能把收尾库存追溯当成每条旧命令执行前的完整指纹；最终 Core 验证在指纹固定后执行。

### 8.4 失败记录、限制与暂停点

- 初次并行 all-features Core 为 626 PASS / 3 FAIL / 4 ignored：两项 runtime-disabled 环境开关断言竞态、一项 Iroh 超时；支持的 runner 使用串行。随后串行 all-features 为 631 PASS / 2 个 Iroh timed out / 4 ignored，退出 101。标准 enabled 集合的串行全库检查通过，包含这两项网络测试；**此前超时仍保留，根因未证明，也不宣称 all-features 全库通过**。旧 f719 对照未包含这两项新 real_network 测试，不能拿它豁免失败。
- 增加身份夹具后，Windows/契约曾因 HistoryIdentity 的 deny_unknown_fields 拒绝 additive-field 而失败；已修正并重新生成，最终 Windows 275 项和 decoder 检查通过。此前失败日志保留。
- `check-production-features.mjs` 仍退出 1，仅报告既有 Core/Runtime 的两项 `repair-regressions = ["test-support"]` 声明。未放宽门禁；不据此推断生产二进制实际启用了 test-support。
- 真实 QQ/目标应用、完整 native GUI、Windows 原生、Mac↔Windows/双设备、真实断电/物理磁盘耗尽、新安装包与资源任务生命周期仍未验收。当前源码已变化，旧 R6/R7 包和摘要不能认证新源码；本项没有替换封存 App，也没有发布。

**O11-D2 本机实现和本项自动验证完成，按用户要求暂停。** 完整 O11/O12 验收仍开放，Release **NO-GO**。下一项是 **O01 原始/派生隔离**：先将原 HTML/text/TIFF/RTFD 与转换结果分开，接入版本化的 archive read/角色和完整枚举，再恢复安全原生去重资格。本轮不进入 O01。


## 9. O01：原件与派生表示隔离

### 9.1 实施与可观察效果

2026-10-06，在同一 `codex/composite-clipboard` 工作树、HEAD f719d581a1aa5198a1b3f2483222df5c69e3dfc0 执行。承接 O11-D2，保留全部既有未提交改动；未暂存/提交，未替换既有 App，未启动完整 App，未读写 general 剪贴板。本项所有 native 样本使用 `tailsync-probe.*` 私有 board，均为合成正文/图片/附件。

| 实际生产改动 | 本项效果 |
|---|---|
| macOS helper protocol 3、CNA1/version 1 | 原 advertised 格式、顺序、平台、revision/枚举/读取事实与转换输出分开；RTFD→HTML/text、TIFF→PNG 有来源/转换器版本 |
| NativeArchive 与 Core 兼容投影 | 转换只读取已取得原字节；原 HTML、text、RTFD、TIFF 不被派生覆盖；不合法/不可生成便携投影的合法原件仍可入库 |
| Manifest 2 可选 archive_version=1、独立加密 archive part | 一个 CNA1 原件/派生归档与便携 parts 分别保存；读取核验角色/版本、内容 ID、归档摘要和安全键；配额/外置载荷/删除沿用生产机制 |
| 默认原件恢复与显式兼容格式分开 | 默认发布原集合；显式 text/html/rtf/png 选择派生/便携集合，下一次默认仍是原件；新 archive 平台不支持时不会默默改用派生默认恢复 |
| 不完整原件的发布前拒绝 | Failed/TooLarge/Absent 描述不被删掉，未完整枚举也不能把已读子集报 Published；common runtime 返回 originals_incomplete，有效显式格式仍可选择；新稳定错误码配套中英文及生成契约 |
| hcn1 原件身份与 hnd1 安全键 | 派生变化/revision/token 不改变原件 ID；完整已知闭合 text/PNG/无属性 HTML 才可去重；RTFD/RTF/TIFF/未知依赖无安全键 |
| 独立 native-original 来源域 | 旧便携 OriginFingerprintV2 不变，原始 RTFD/TIFF/已知多项目恢复获得匹配指纹；读取含派生输出时按原件分类，私有往返为 SelfWrite |
| 采集 memory reservation 204→240 MiB | 更完整记账，全局 256 MiB 上限不变；CNA1 原件＋派生 ≤64 formats /16 items /40 MiB，历史 72 MiB 上限不变 |

本轮没有扩大任意类型读取/发布白名单：advertised 但未准入类型保留 Absent 描述，由 O02 后续处理。CNA1 的 Exact 角色不等于完整读取，更不等于外链、provider、RTFD 资源依赖全部闭合。网络 CBP1、data frame、journal 与旧 data_hash 算法没有变化。旧 Manifest 1/2 不批量改写，旧字段缺省时仍按旧序列化/摘要规则读取；独立回退证明使用真实旧源码 reader。

### 9.2 失败基线与回归闭环

- 使用修改前 helper 再跑 S0 六组 RTFD 样本，6/6 原 RTFD 保存，但原 HTML/text 被转换结果替换，`red-results.json` 与原始输出保留。
- 原始 RTFD 发布指纹回归先失败 InvalidComposite，增加独立 native-original 域后通过；旧便携指纹的现有回归仍通过。
- 收尾新增“不完整读取描述不得被过滤”和“默认恢复拒绝未完整原件”两项回归，先实际 RED，退出 101；修复后 GREEN。拒绝发生于共用准备路径，在 OS publication 之前；有效显式文字恢复仍通过。旧便携记录默认路径保留。
- 新增正式生产回归 **13 项**：codec 4、Core 7（其中 DB 6）、Runtime 2；覆盖截断/尾随数据、链接/版本/预算、原件/派生重启留存、身份稳定、旧控制标志不能证明转换、完整/缺失枚举去重、multi-item/failed descriptor、外置归档 GC、原始来源指纹、显式转换与默认恢复隔离。
- 不删除/放宽已有断言，没有新增 ignore/skip 或提高超时。初期测试夹具重开方式、私有 DB API 引用和非 Debug enum 的编译问题已修正，失败日志仍在。曾有 tool session exit 元数据与日志不一致，最终检查统一以 Python subprocess 实际 returncode 记账，不追溯把旧 run 改成 PASS。

### 9.3 最终源码检查

冻结源码指纹：`16096428b7e20d23efea6c4f5457223ca7d3aea9d7de74dd9b25f9d40c5e6e5e`，**761 文件**。基线为 O11-D2 的 bd25ffe0…（759 文件）；本项修改 24 个既有源码/测试/工具文件，新增 NativeArchive codec 与 original_archive_probe 两个文件。每条最终命令结束后核验完整 files map 没有漂移。Markdown/证据/包按既有指纹规则另封存；不混用旧 package 的证据。

| 最终检查 | 实际结果 |
|---|---|
| Protocol 全测试 | 40 PASS |
| 标准 enabled Core 全库，串行 | 630 PASS /4 既有 ignored，含两项 real_network |
| DB all-features，串行 | 141 PASS /2 既有 ignored |
| Runtime all-features＋Core composite | 27 library＋11 integration＝38 PASS |
| macOS Rust enabled | 117 PASS /3 既有 ignored |
| Windows Rust，macOS 宿主 | 126 PASS /2 既有 ignored；不代表 Windows native |
| Workspace all-targets/all-features Clippy、macOS enabled Clippy，-D warnings | PASS |
| Windows helper x86_64-pc-windows-msvc portable-check，macOS 宿主 | PASS；不代表 Windows link/package/run |
| 契约/decoder、跨平台检查、canonical include 与 23 项负向 | PASS；local contract 2 /stable error 1；新增 code 由 Rust 统一生成 |
| Swift | 265 PASS /4 既有 skipped（269 total） |
| Windows UI /UI build | 275 PASS /构建 PASS |
| Workspace/macOS/Windows/shared-platform 格式与 diff 检查 | PASS |
| 生产 feature 声明检查 | **FAIL，exit 1**：仍为既有 Core/Runtime 两项 repair-regressions=[test-support]，未增加/放宽违规 |

Windows 宿主编译记录两项 dead_code：已有 Win32/InvalidData 变体，新增 macOS 专属 read_archive 在 Windows crate 的 macOS 宿主编译中未使用。Windows 原生 target 的 cfg 不编译这个 macOS-only 入口；本项没有以 allow/suppress 隐藏它。Workspace/macOS Clippy 范围内没有警告。此前 O11-D2 all-features 全 Core 的两项 Iroh 超时仍保留；本项仅重跑支持的标准 enabled 全 Core 和 all-features DB，不把它们写成 all-features 全库 PASS。

### 9.4 新包、私有原件往返与旧 reader 回退

最终新包：`artifacts/clipboard-improvement/2026-10-06-o01-originals/TailSync-O01-final-native.app`。初始 `TailSync-O01-native.app` 保留，但不作为最终源码候选。

- final arm64 App 全新构建，ad-hoc 签名与 deployment 检查 PASS；三个 Mach-O 均 arm64 /最低 macOS 13.0。
- 包内 helper `CWH1 protocol=3`；生产 tailsyncd resolver 报 composite_compiled=true、debug_resolver=false、healthy=true，路径准确指向同包 helper。
- 新 daemon＋修改前真实 helper2 放在隔离副本，resolver 退出 1、healthy=false、expected=3、found=2；不替换任何原 App。
- 最终包内 helper 的既有五组私有 board self-test PASS（text/HTML/PNG、RTF、双图、多项目及过期 revision/坏输入不清空等）。
- **7/7 新包实测通过**：六组 S0 RTFD（两个附件名×三次，原 HTML/text 故意不同）＋一组原 TIFF→派生 PNG；包内 CNA1 捕获→生产 DB 保存/关闭/重开→默认原 CNS1→生产 supervisor 私有 board 发布→CNA1 回读，原件字节一致、派生另存、Published receipt 与 SelfWrite 匹配。
- 独立当前源码 v13 writer＋封存 schema12 生产 reader 再验回退 PASS：旧 3 条历史/原件/收藏一致、3 个外置 payload 验证；新增 v13 历史另行保全。旧 reader 打开 v13、只复制 DB 漏载荷两条负向均拒绝。旧源码与当前 shared 源码在运行期间保持不变。

包聚合 SHA256：`50faeec067d1a19b761b0df23bbf9ae7c6ee5da5c11de3057cc39647dfc26914`；helper SHA256：`0eb45d793fd5480d798ce1cce57955b2eb49bcf8e050a97182a0e6761eba1d62`。这些只认证本项 arm64 包；未运行 universal 构建、关闭态新包或完整 GUI，旧 R6 universal 包不继承 O01 认证。

包检查记录脚本首轮在读取已成功的拒绝结果时误用 reason 键（真实字段为 detail），产生 KeyError；产品 build/manifest/旧 helper 拒绝均已通过，尚未发布私有内容。保留脚本首版和 exit 1 记录，以正确字段核验既有原始输出，随后完成 self-test 与七组往返；不重写初次失败。

### 9.5 证据、限制与暂停点

证据目录：`artifacts/clipboard-improvement/2026-10-06-o01-originals/`。

| 路径 | 内容 |
|---|---|
| source-before.json /source-before/* | O11-D2 源码库存与修改前全部文本封存，保留先前 dirty state |
| source-fingerprint.json /source-after.json /source-delta.json | 最终执行绑定、收尾库存与 O01 源码变化 |
| red-results.json /logs/*-red.* | 原始 HTML/text 替换、原始指纹和不完整恢复的真实失败 |
| final-verification/commands/* /logs/* /results.json | 22 条最终命令的实际 exit、日志/计数、源码绑定；21 PASS＋1 既有 FAIL |
| package-manifest.json /final-package-proof.json | 最终包签名/架构/最低系统/resolver/文件摘要与旧 helper 拒绝 |
| final-private-proof/* | 7 组 source CNS1 /captured CNA1 /published CNS1 /restored CNA1、Core/DB 原始输出与结果 |
| rollback-final/proof.json | 当前 writer 与旧真实 reader、新目录恢复及负向证明 |
| validation-results.json /evidence-sha256.json | 本项结果索引、失败历史、分层限制与完整证据封存 |

O01 的本机实现及本项自动验证完成。A09 的自动部分已取得上述证据，TextEdit/QQ/其他真实目标应用部分仍 UNKNOWN；QQ、完整 native GUI、Windows 原生、Mac↔Windows/双设备、provider 与真实断电/物理满盘、资源补齐生命周期未验收。没有因此宣称任意格式保留、所有应用图文粘贴或发布就绪。

**本项完成后暂停；下一项 O02 尚未执行。Release 仍 NO-GO。** 原件恢复完整性、派生角色/存储和本机七组往返已实证；不把待设备/真实应用条件交叉记成 PASS。


## 10. O02：原生字节准入与敏感采集阻止

### 10.1 原件准入、隐私与边界

本项在 `codex/composite-clipboard` 工作树实现，HEAD 仍为 `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`；未修改用户提供的另一个 checkout，未暂存或提交。修改前核验 O01 源码库存与封存证据，旧包、已有 dirty/untracked 工作保持。没有读取或写入 general clipboard、启动完整 App 或替换已有 App；native 试验只用 `tailsync-probe.*` 私有 board 与合成数据。

- policy 1 的 JSON 同时生成 Rust/Swift。未知名称非空、UTF-8 ≤256 bytes、无 ASCII 控制字节时按 opaque Data 捕获；可选单格式 ≤4 MiB。text 1 MiB、HTML 2 MiB、RTF 4 MiB、RTFD/PNG/TIFF 32 MiB 额度保持。共同的 16 项目、64 原件＋派生格式、40 MiB CNA1、5 秒 supervisor、240 MiB reservation/256 MiB 管线总预算不提高。
- 全部 items 的元数据先检查四种精确抑制标记：TransientType、ConcealedType、AutoGeneratedType 和 `com.agilebits.onepassword`；先于项目数量限制及任何正文请求。helper 的 CNA1/CNS1、旧 text/image/file 入口均阻止正文，返回空 Suppressed 或无正文失败。监听仅完成 revision 确认/重试清理/旧文件 intent 的 supersede，不写历史、不广播、不降级读正文；DB archive/legacy 入口先于摘要/转换拒绝，内层再防御。
- 常用表示优先读；描述仍按 OS 顺序保存。超限、失败、未读取及枚举不完整事实不伪装成完整原件。纯文件选择走既有文件管线；混合文件 URL、旧 filenames、file promises 等 policy deferred 项只保存 Absent 描述，不读依赖、不发布路径，不绕过文件授权/物化器。
- 新 opaque bytes 经 archive role 保存和重开，并经允许的 macOS Data 接口发布；未知内容不被任意反序列化/执行或抓取外部资源。未知格式不能因此获得依赖闭合证明、目标应用兼容证明或安全历史去重键。集合包含未被旧投影覆盖的格式时，整个原件集合参与独立 origin 域比较，即使同时有 text，也不能仅比较 text；便携 origin v2 算法不变。
- macOS helper protocol **3→4**，Windows 仍为 **2**。CNA1 version 1 增加 metadata-only Suppressed=6（空项目/格式，revision 稳定）；helper 4 检查拒绝旧 codec。history 13、Manifest 2、local contract 2、原件 origin 独立域 1 保持。旧库、schema、迁移/rollback 格式没有增加新版本。

### 10.2 真正的 RED 与修复

复制实际 O01 包内 helper 3，使用真正的 `NSPasteboardItemDataProvider`：未知原生格式没有被请求或捕获；另一 item 存在 ConcealedType 时仍请求 `public.utf8-plain-text` 正文并返回 Stable。两个 subprocess 都退出 0，但原始请求日志和 CNA1 已证明缺陷，不以 exit 0 当成测试通过。

Core 两项回归（opaque origin、敏感元数据 DB 拒绝）和 macOS 发布准入回归在修改前实际退出 101；原始日志保留。修复后定向 GREEN，随后再运行最终完整矩阵。旧 snapshot 存储入口的抑制检查也提前到摘要之前。

最终验证第一次在 macOS Clippy 的 `needless_return` 退出 101；第二次 Clippy 通过，但 macOS 格式检查退出 1，要求新 Suppressed match arm 排版。两次原始日志和命令记录按原样保存在 `first-verification/`、`second-verification/`，位置映射另外记录，不重写原内容。去掉多余 return、按 rustfmt 调整格式后重新冻结源码并完整复跑。本项没有新增 ignore/skip、删除原测试、加长期限或加入 warning 豁免。

### 10.3 最终源码与本地矩阵

最终源码 SHA256：`a1d5121f6aa66a56fb3fd430b117cfeb9336146e4c42329748f8f7f3c20de21f`，**767 文件**。对 O01 库存新增 6、修改 11、删除 0 个指纹文件；每条最终命令结束后核验 files map 不漂移。Markdown/其他 artifacts/包按既有规则单独封存。

| 最终检查 | 实际结果 |
|---|---|
| Protocol 全测试 | 42 PASS |
| 标准 enabled Core 全库，串行 | 632 PASS /4 既有 ignored；含 real_network |
| DB all-features，串行 | 142 PASS /2 既有 ignored |
| Runtime all-features＋Core composite | 38 PASS |
| macOS Rust enabled | 118 PASS /3 既有 ignored |
| Windows Rust，macOS 宿主 | 127 PASS /2 既有 ignored；不代表 Windows native |
| Workspace all-targets/all-features Clippy、macOS enabled Clippy，-D warnings | PASS |
| MSVC helper portable-check，macOS 宿主 | PASS；不代表 Windows link/package/run |
| 契约/decoder、跨平台、canonical include＋23 项负向 | PASS |
| Native policy 同步检查＋3 项负向 | PASS；检查双边漂移、重叠策略和非法限制/名称 |
| Opaque production Core/DB/supervisor probe build | PASS |
| Workspace/macOS/Windows/shared-platform 格式与 diff | PASS |
| 生产 feature 声明门禁 | **FAIL，exit 1**；既有 Core/Runtime 两项 repair-regressions=[test-support] 声明仍被拒绝 |

**22 条最终命令：21 PASS＋1 既有 FAIL。** O01 的 unused macOS read_archive 宿主警告随实际读路径接线消失；Windows 宿主还保留一项既有 Win32/InvalidData dead_code，未隐藏。Workspace/macOS 的上述 Clippy 检查没有警告。此前 O11-D2 all-features 全 Core 的 Iroh 超时仍保留；此处标准 enabled 全 Core 与 all-features DB 不代替 all-features 全库。O02 没有修改 UI 来源或 UI contract，故本轮不重复 O01 的 Swift/React 检查充数，也不把旧 UI 测试认证为新包 GUI 验收。CI 配置接入 policy 门禁，但本轮 hosted CI 未执行。

### 10.4 最终新包与受控原生证据

全新独立构建 `artifacts/clipboard-improvement/2026-10-06-o02-admission/TailSync-O02-final-native.app`，没有替换 O01 的两个 App 或启动完整 App。包中三个 Mach-O 均 arm64、最低 macOS 13.0；ad-hoc 签名、deployment 检查 PASS。helper 自检为 `CWH1 protocol=4`，release resolver 报 compiled=true、debug_resolver=false、healthy=true，路径恰为同包 helper。复制新 daemon＋原封存 helper 3 到隔离目录，真实 CLI 退出 1、healthy=false、expected=4、detail 明确 found=3。

包聚合 SHA256：`6e9c3e5d1dc274b6b2c00b01a201c89f8cf91b69cd74a9d9014b70cc8808d86f`；helper SHA256：`90c3cb1b5b072ed92afbe74e973ea86b7d75d9a90c1893e73c601a7018458b66`。受控 provider 从保存的 fixture.swift 新编译为 fixture-final，源码/二进制摘要及实际编译退出码保存。所有最终原生试验均使用这个新包内 helper，绑定最终源码 `a1d5121f6aa66a56fb3fd430b117cfeb9336146e4c42329748f8f7f3c20de21f`。

| 合成原生检查 | 实际结果 |
|---|---|
| 自定义多 item／单 item、含空 Data、类型名称和次序 | 精确原件留存；两组 DB 重开→原集合回放→SelfWrite 匹配 PASS |
| 四个敏感标记＋第 17 项敏感标记 | CNA1 metadata-only Suppressed；CNS1/旧 text/image/file 无正文；所有 provider 请求为 0 |
| 混合 HTML/text＋file URL/promise | 只请求 text/HTML；依赖描述 Absent，未物化/未发布；DB 重开完全保留 |
| 纯文件选择 | metadata-only FileSelection，provider 请求为 0，Core 拒绝当复合历史保存 |
| 单格式 4 MiB＋1、81 类型、44 MiB 可选输入 | TooLarge／枚举不完整准确保留；text 优先；64 格式／40 MiB 不突破 |
| 8 秒慢 provider | 生产 5 秒 supervisor 超时；实测 5.478 秒（含隔离 wrapper 开销）；helper kill/reap 后不存在，无半帧提交 |
| 新复制打断旧 provider | 旧帧 Unstable、无历史；下一次读取新 text，未混合版本 |
| sensitive／file URL／opaque 超限／非法名称发布 | 四项退出 65；私有 board 字节和 revision 未改变，拒绝发生在 clear 前 |

**15 项 native 检查、13 项 Core/DB 存储检查均 PASS。** 不稳定、Suppressed、FileSelection 输入在写历史前拒绝；稳定但不完整的归档重开后完全一致，未调用 native 发布。未知类型的历史 dedupe key 均为空。

既有 helper 五组私有 board self-test PASS；O01 的封存七组脚本在新目录使用 helper 4 重跑，RTFD 原 HTML/text 故意不同的六组和 TIFF 原件的一组全部 PASS。RTFD/TIFF→派生转换没有污染原件恢复。

新 O02 Core writer＋封存 schema 12 生产 reader 的独立回退证明 PASS：升级后的行另行保全、旧三条历史与三个外置 payload/收藏保持；旧 reader 打开 v13、只复制数据库漏 payload 两项负向均拒绝。回退在最终 macOS-only 格式调整前运行，其所有 157 个 shared Rust/TOML 输入与最终源码逐字节一致；保留当时完整 root hash，不改写为新 hash，另存严格输入比对 `rollback-current-binding.json`。

### 10.5 证据、限制与暂停点

证据目录：`artifacts/clipboard-improvement/2026-10-06-o02-admission/`。

| 路径 | 内容 |
|---|---|
| source-before.json／source-before/* | O01 源码库存与修改前全部文本；保留既有工作 |
| red-results.json／baseline-*.cna／logs/*-red.* | 实际 helper 3 缺陷与三项生产接缝 RED |
| first-verification／second-verification／位置映射与 first/second fingerprints | 原始 Clippy/格式失败、对应源码与命令记录，不覆盖失败 |
| source-fingerprint.json／source-after.json／source-delta.json | 最终执行绑定与 O02 源码差异 |
| final-verification/commands/*／logs/*／results.json | 22 条最终命令的实际 exit、日志摘要、计数与源码绑定 |
| package-manifest.json／final-package-proof.json | 新包架构/签名/最低系统/resolver／真实旧 helper 拒绝 |
| final-private-proof/* | provider 请求名、原始 CNA1／CNS1／失败输出、DB 重开与回放逐字节证明 |
| final-original-proof/* | O01 七组原件/派生回归的 helper 4 实测 |
| rollback-final／rollback-current-binding.json | 新 writer、旧真实 reader、完整载荷回退与最终 shared 输入一致性 |
| regression-preservation.json／documents/*／validation-results.json／evidence-sha256.json | 未削弱既有测试、当前文档快照、分层结论与完整封存 |

O02 的本机实现与合成原生自动部分完成，A10/A11/A16 在上述边界内取得证据；当前工作树还存在明确记录的生产 feature 门禁 FAIL。QQ/真实来源与目标应用、完整 native GUI、Windows 原生、Mac↔Windows/双设备、universal／关闭态新包、hosted CI 及真实断电/物理满盘仍 UNKNOWN。本项不宣称任意 opaque 类型依赖闭合或通用互操作，不把 O01 旧包/GUI 自动测试当成新包真实验收。

**完成 O02 后暂停。按方案下一项为 O06；本轮未启动 O06/O10/O03。Release NO-GO。**


## 11. O06：准备恢复计划，再集中发布

### 11.1 输入、选择与一次发布

新增不可变、owned 的 RestorePlan version 1。输入绑定行的 HistoryIdentity、CNA1 来源版本/平台、独立域的原始载荷摘要和请求格式；输出逐项记录 item、稳定类型名、字节长度与 BLAKE3 摘要。原件、资源补齐的兼容输出及显式单格式互相独立，不回写历史原件，也不把派生表示混入之后的原件恢复。

同平台完整且无已知 HTML 外部依赖的集合选择原件；原 HTML 使用失效外链、便携数据已有精确绑定及真实资源时，另行准备内嵌双图的 HTML；异平台只用完整、目标支持的便携表示。缺资源返回 resources_incomplete；CNA1 原件未取得完整仍返回 originals_incomplete。不透明格式保留 O02 的字节回放边界，本项不承诺任意未知类型的语义依赖闭合。

准备阶段验证 HTML 节点跨度、资源 ID、真实 PNG、累计像素与“绑定资源＋源码内嵌图”20 项额度。相邻 srcset、CSS URL、poster 等不借用另一个绑定的证明；不访问外部 URL、file 或 blob。Windows 提前准备 UTF-16、CF_HTML UTF-8 偏移、必要终止字节及既有 PNG/CF_DIBV5 组合，Mac 保持原 item 边界与原生准入额度。所有输出字节由计划持有，不依赖源历史文件继续存在。

历史恢复在短 engine lock 下登记 generation/revision，blocking worker 准备后进入 FIFO，再核对 generation，把同一个计划和 admission 移交 ClipboardPort。生产 Mac/Windows adapter 直接复用原 supervisor/helper、journal、expected revision 和取消通道。取消 async waiter 不提前释放阻塞 worker 持有的输出、memory/FIFO；Partial/Unknown/NoEffect 保留类型语义，不转换为成功或自动重写。

自动接收也采用这一计划：合法但准备不能完成的包仍保存历史并返回 StoredIncomplete，发送方的 Complete 不绕过依赖核查。原授权、generation、operation permit、claim、tombstone 与 ACK 状态机保留。列表对有界、校验过摘要的 HTML 做 owned worker inventory；缺失/损坏载荷拒绝 HTML 动作。native-only 默认菜单也使用同一恢复判定。

history 13、Manifest 1/2、CNA1 1、local contract 2、error schema 1、Mac helper 4、Windows helper 2 与原件 origin 独立域 1 保持。RestorePlan 1 只在操作内存中存在，没有新增协议帧或 schema；新稳定原因由 Rust 导出并生成 Swift/TS 契约，两端中英文分派已验证。

### 11.2 实际 RED、修复与未削弱检查

- O02 生产接缝上的初始两项 RED 均 exit 101：缺资源仍 Published/发生 native write；旧排队恢复在新 intent 后仍 Published。最终 Runtime 回归均通过。
- 首次完整 Runtime 发现 RR30 的菜单/恢复不一致，exit 101；保存原日志后补 DB 同步查询、关键词扫描及 owned read 的 HTML 判定，未修改存储 manifest 或弱化断言。
- 首次冻结矩阵在 Clippy 实际 exit 101：large_enum_variant 与 nonminimal_bool。以 Box 持有计划及等价布尔式修正，无 warning allow。
- 复核追加 native-only 默认菜单、原件构造器直接拒绝外链和“20 bound＋1 inline”资源额度三项真实 RED；各 exit 101，最终均 GREEN。
- 第二次冻结矩阵的 Swift 实际 exit 1：新增稳定原因遗漏 ApiError 的 restoreCodes 分派，两个测试共四条断言失败。补上真实 API/本地化分派后，六项专项与全套均通过。
- 初次 Core 试跑的 Arc 类型推导编译失败也保留；没有把任何失败写成 PASS。

first-verification / second-verification、各自 source fingerprint 和全量 source-first/source-second 文本保留。O02→当前新增 **17** 个 Rust 回归，无删除既有测试/源码、无新增 ignore；既有 ignore 总数仍 **13**。未提高期限、像素/字节预算，不新增 --skip/--ignored，不绕开生产 feature gate。现有工作树改动与 App 全部保留，没有 staging/commit。

### 11.3 冻结源码与最终矩阵

工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`；HEAD 仍为 `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。

O02 输入指纹：`a1d5121f6aa66a56fb3fd430b117cfeb9336146e4c42329748f8f7f3c20de21f`，767 文件。最终 O06 指纹：`303fc44c9bb0fd6dc18865a59a778055b81e609124798ae14587c922667e4338`，**770 文件**。每条最终命令及包/proof 都绑定同一文件库存；文档以独立快照封存。

| 检查 | 实际结果 |
|---|---|
| Protocol 全套 | 42 PASS |
| Core 全套：composite＋file sliding window＋image compressed chunks，serial | 642 PASS，4 既有 ignored |
| DB all-features，serial | 143 PASS，2 既有 ignored |
| Runtime all-features＋Core composite，serial | 45 PASS；含 O06 7 项与既有 11 项 integration |
| macOS Rust enabled | 118 PASS，3 既有 ignored |
| Windows Rust on macOS host | 127 PASS，2 既有 ignored；不认证 Windows 原生 |
| workspace all-targets/all-features Clippy；macOS enabled Clippy | -D warnings，均 PASS |
| Windows MSVC helper portable-check | PASS；host check，不是 linker/原生运行 |
| 生成契约、cross-platform、canonical、native policy | PASS；canonical 23 项与 policy 3 项负向 PASS |
| Swift 全套 | 265 PASS，4 既有 skipped；headless 单元测试，不是 GUI 验收 |
| Windows UI／前端 build | 275 PASS／PASS |
| 新 RestorePlan probe 与 opaque probe build；各 fmt/diff | PASS |
| production-features | **实际 FAIL，exit 1**；恰为既有两处 repair-regressions=[test-support] 声明，未修复/未豁免 |

最终 **26 命令：25 PASS、1 实际 FAIL**。使用上述已声明支持矩阵；O11 历史 full Core all-features Iroh 超时失败继续保留，没有写成已通过或用 skip 掩盖。

### 11.4 全新包与真实私有 board

全新独立包：`artifacts/clipboard-improvement/2026-10-06-o06-restore-plan/TailSync-O06-final-native.app`，2.3.2，composite enabled，arm64。三个可执行文件架构、签名、最低系统 13.0、release resolver 均 PASS；没有启动完整 App，也未替换之前的包。

包聚合 SHA256：`8cce91f2080fa321dbb5da1baee2454e554e39d813ecde47427aa5d303c37dbc`；helper SHA256：`90c3cb1b5b072ed92afbe74e973ea86b7d75d9a90c1893e73c601a7018458b66`。Mac helper 协议仍 4：O02 原 helper 4 与新 daemon 健康兼容；实际旧 helper 3 被新 daemon 拒绝（expected 4/found 3），负向二进制来源和摘要已保存。

最终全部原生试验使用这个新包内 helper：

| 证明 | 实际结果与边界 |
|---|---|
| helper 私有 board 自检 | 5 夹具 PASS；原子多格式、item、expected revision、malformed-before-clear |
| O02 provider／DB／回放 | 15 组 native checks、13 组 storage observations PASS；敏感标记无正文请求、provider 期限/回收、revision race 等保持 |
| O01 原件回归 | 7 组 PASS：6 RTFD 图文＋1 TIFF；来源 raw HTML/text、原件、派生证据、DB 重开和回放字节核对 |
| O06 已绑定双图 | 真实 DB 保存/重开→Runtime RestorePlan→正常删除源历史/关闭 DB→production supervisor→新包 helper，一次整组发布；红/蓝图顺序、输出字节、receipt revision 与 SelfWrite 相符 |
| O06 原件独立 | RTFD、TIFF、两 item opaque 三组原件重开后相同，显式格式请求不改变之后默认计划；源历史正常删除后仍回放，并得到 SelfWrite |
| O06 拒绝边界 | 缺绑定准备拒绝后，已种私有 board 字节/revision 相同；旧 expected revision 返回 Superseded，拒绝在 clear 前 |

O06 probe 验证真实 DB/runtime selector/supervisor 与 packaged helper；异步 ClipboardPort 的取消/FIFO/单调用与 typed Partial/Unknown/no retry 在生产 port/Runtime 计数平台回归中验证。它**不替代 live platform adapter journal、完整 App 或真实目标粘贴**。已有公开 URL 归档和 Windows 带图 RTF/目标特定输出未实现；本项双图是已有合成绑定资源，不能写为 A13 的公开 URL/断网/实际目标验收。

### 11.5 回退、封存与下一项

`rollback-frozen/proof.json` 为最终源码新运行的 schema 13 writer＋封存 schema 12 真实生产 reader：三个旧条目、收藏和三个外置 payload 摘要一致，新增 v13 条目保存在原目录及另存副本；旧 reader 直接开 v13、仅复制数据库漏 payload 两项负向均拒绝。所有 shared Rust/TOML 输入与最终源码逐一核对；早期两轮 PASS proof 也保留各自原指纹，没有改写。

O01 的 1,083 个、O02 的 1,397 个封存文件逐一校验无变化。source-before/first/second/final、所有真实失败、最终命令/包/proof、回归保全、文档快照与 validation-results.json 纳入 O06 evidence-sha256.json。源库存与包文件在封存前再次逐一比对。

本项 **LOCAL_COMPLETE** 限定本机实现与合成自动部分。QQ/真实来源和目标、完整 native App/GUI/live adapter journal、Windows 原生/portable/installer、双设备、hosted CI、物理断电/满盘，以及 O04 的公开引用与未来文件 lease 仍 UNKNOWN/后续任务。所有当前输出都是 inline owned bytes，没有当前 clipboard 仍依赖的临时文件；这不证明未来 materializer lease/GC 已完成。

**完成 O06 后暂停。下一项为 O10 因果语义修正；本轮不启动 O10/O03。既有 production-features FAIL 保留，Release NO-GO。**


## 12. 审查整改

日期：2026-10-06。用户授权处理[实现审查](COMPOSITE-CLIPBOARD-IMPROVEMENT-CODE-REVIEW-2026-10-06.md)与后续核查发现的问题。本次仅整改已完成阶段；不启动 O10、F04/F06、公开资源归档或发布。

### 12.1 基线与实际改动

- 原工作树与 HEAD 保持：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，`f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。
- 整改前：`303fc44c9bb0fd6dc18865a59a778055b81e609124798ae14587c922667e4338`，770 文件，与 O06 封存相同。
- 整改后：`145c47d9c695544323b763f4498d5db9c0d918b52d94e3a8ae964245396102da`，770 文件。
- 相对本轮开始的未提交源码：23 文件，+471 / −482；不是相对 HEAD 的累计统计。保留全部既有改动，无暂存或提交。
- 证据目录：`artifacts/clipboard-improvement/2026-10-06-review-cleanup/`。整改前源码完整副本、最终变更文件副本、增量 patch、真实 RED、命令与输出分别保存。O01/O02/O06 三份历史封存复算：1083/1397/2874 文件，全部无变化；已有 App 均保留。

| 审查项 | 实际处理与效果 |
|---|---|
| 生产 feature 门禁 | 两 crate 的 repair-regressions 改为空选择标记；测试 runner、R6 matrix 与 CI 显式选 test-support。门禁原代码与拒绝规则不改，当前 12 manifest 检查通过，9 项负向/正向测试通过；默认 Runtime 依赖树无 test-support |
| RestoreInput 惰性元数据 | 删除无消费者的 identity 副本、source_archive_version、payload_digest 及两条输入摘要计算路径；没有增加重复哈希。DB 的身份、归档与部件完整性验证保持，来源平台和请求格式继续参与准备 |
| 输出清单成本 | outputs() 从存储切片改为按需生成诊断 Vec。正常准备/发布不分配类型名称、不计算逐输出诊断摘要；计划仍独占不可变实际字节，既有清单/字节断言继续执行 |
| ClipboardPort 方法梯 | 删除 publish_native_admitted 与 composite 的 cancellable/admitted/shared_admitted 包装层；保留所需 base 方法和生产 plan 方法 |
| 确证死代码 | 删除 HelperPath、NativePublish::is_proven_no_effect、unresolved_intent_age（及仅供它读取的内存时间字段）、set_history_ref 和空 decode 绑定。history_ref 已由 ClaimPrepared/claim_prepared 在 claim 插入时写入，无需后置回填 |
| HTML 规则 | 采集、列表资格及发布准备共用有界词法 inventory 与属性谓词；注释不贡献标签，真实危险标签和 CSS URL 仍拒绝，每个 img 独立检查 src。修复合法图片掩盖缺来源节点的漏判，也修复注释内标签、普通正文 URL 字样导致的误拒绝 |
| 解析与事实源 | 完整能力查询只 decode 一次并复用；异步恢复的预算扫描与准备复用同一便携 bundle。Manifest 与 payload 准备共用 RestoreFormatInventory 的拒绝原因，修正“runtime API 驱动生产菜单”的错误注释 |
| 列表成本与诊断 | 同步与 owned 列表复用已解密 manifest，去掉一次重复解密；HTML 仍在有界 worker 上校验摘要/引用。直接解构 Candidate 消费 menu，去掉 mem::replace；失败与隐藏 HTML 留下无正文诊断 |
| 默认 adapter 回执 | Superseded 使用本次实测 revision；未能观测 revision 时返回 NoEffect，发布调用数为零。真实 Mac/Windows adapter 的 override 保持 |
| 测试维护 | 删除已失效接口对应的旧取消测试，其 FIFO、内存、取消断言由生产计划测试覆盖；该测试新增 worker 进入及退出前的实际输出字节断言。新增 HTML 表驱动回归和 revision guard 回归，扩展既有 DB/runtime 用例，无新增 ignore。不同双图夹具的中文、透明度、表示顺序与绑定索引差异保留 |

RestorePlan 仍为内存计划 1；存储、网络、helper、local contract 版本均不变。ADR-005 第 11 节同步解释完整性边界和按需诊断；不把未使用的摘要说成发布前验证。

### 12.2 真实失败与回归保全

新增 `dependency_inventory_ignores_comments_and_checks_each_image` 在旧实现下真实失败（exit 101）：`<img src='有效内嵌 PNG'><img>` 被标为 Complete，而预期 MissingFormats。原始日志见 `commands/html-red.log`，没有改写为成功。

修复后同一表驱动用例覆盖：缺来源节点位于合法图前/后、注释内 img/script、纯文本 URL 与安全 style、实际 CSS URL 和 iframe。既有 DB 用例增加同步查询、关键词查询、关闭 DB 后 owned 查询的缺来源/注释对照；既有 runtime 用例增加一张正确绑定 PNG 加缺来源节点，验证默认/显式 HTML 均不发布，text 仍可独立发布。

新增 2 个 Rust 测试、合并移除 1 个旧接口测试；现有生产计划取消测试保留旧不变量并增加输出字节校验。源码显式 ignore 注解 11→11；本次所有运行的既有 ignored 均保留。删去的计划元数据字段断言随字段移除，DB 身份/版本/部件完整性与输出语义断言没有放宽。

### 12.3 最终验证

最终选择的 **18 条命令全部 exit 0**；初步 focused/runtime 日志与预期 RED 分别保留，不混入最终矩阵。完整 Core 启用 repair-regressions 的 10 项额外回归，因此测试数不能直接与 O06 未启用该标记的矩阵作新增测试数对比。

| 最终检查 | 结果 |
|---|---|
| Core enabled＋repair＋两项传输扩展，serial | 653 PASS，4 既有 ignored |
| Core 接收端与 repair integration | 11＋3 PASS |
| DB all-features | 143 PASS，2 既有 ignored；属 Core 子集，不重复加总 |
| Runtime all-features＋Core composite | 27＋7＋11＝45 PASS |
| macOS Rust enabled | 118 PASS，3 既有 ignored；未启动完整 App |
| Windows Rust on macOS host | 127 PASS，2 既有 ignored；不认证 Windows 原生 |
| workspace all-targets/all-features Clippy | PASS，-D warnings，0 warning；含 example 探针编译 |
| MSVC helper portable-check | PASS，host check；不认证 Windows linker/原生运行 |
| production-features／默认依赖树 | PASS，12 manifest／无 test-support |
| production feature 规则回归 | 9 PASS，拒绝规则未豁免 |
| Python 证据与 runner harness | 69 PASS，含零测试/遗漏回归拒绝验证 |
| local contracts／canonical／native policy | PASS；canonical 23 项负向 PASS |
| fmt／diff-check | PASS |

详细 argv、退出码、耗时和原始输出见 commands/；后段验证同时记录运行前后源码指纹，两者均为最终值。`validation-results.json` 列出最终与初步检查，`source-before.json` / `source-final.json` 记录逐文件摘要，`incremental.patch` 只覆盖本次源码增量。

列表仍需读取该页 composite 的 HTML，以保持菜单与拒绝边界准确；本次没有引入缓存、延后显示或取消校验。已经消除的重复解密/解析可从调用链确认，尚无本次用户可见时延的量化结果。

### 12.4 验收边界与下一项

本次达到本机源码整改与自动验证完成。当前源码不与 O06 旧包混称同一候选；本次没有构建或安装新 App，也没有完整 GUI、真实目标粘贴、Windows 原生、Mac↔Windows 双设备或 hosted CI 证据。相关条件继续 UNKNOWN；F04/F06、公开引用与未来文件 lease 仍后续任务，Release NO-GO。

**审查整改完成后停止；下一项仍是 O10。本次没有启动 O10。**

## 13. O10：因果、防迟到与提示语义

日期：2026-10-06。用户在审查整改后授权继续原计划下一项；本轮实施 O10，完成后按逐项约定暂停，未启动 O03/F04/F06。

### 13.1 基线与范围

- 工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`；HEAD 保持 `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。
- 开始源码：`145c47d9c695544323b763f4498d5db9c0d918b52d94e3a8ae964245396102da`，与第 12 节整改封存一致。
- 最终源码：`e9d431aff31a9ae2181cc2e8313ebf05d385c89516c79b2e2de5856daaaeb739`；770 文件，本轮源码/测试/工具增量 15 文件，+508 / −87。文档独立保存，不计入源码指纹。
- 证据：`artifacts/clipboard-improvement/2026-10-06-o10/`。包含完整基线源码副本、最终变更副本、增量 patch、每次实际尝试的 argv/退出码/原始日志、最终结果及文件封存摘要。
- 保留所有既有未提交改动；没有暂存、提交、重置、安装或启动完整 App，没有访问通用剪贴板，没有更换已有测试包。

### 13.2 实际修正与用户效果

| 接缝 | 原行为 | 当前行为 |
|---|---|---|
| 双平台原生专用保存 | UnsupportedItems/NativeOnly 成功入库后仍发 capture_failed | 成功提交后调用共用 record_saved_capture；转换不支持不再被当作读取失败 |
| 部分读取 | 不同转换分支可能把真正 provider 失败当作空内容 | 有可恢复源字节时保留原件，并依据 Failed/TooLarge/MissingFormats 发读取不完整提示；全部失败、没有源字节时进入有界读取失败路径 |
| Windows 便携投影失败 | 稳定快照无便携表示或转换失败可能直接跳过原生保存 | from_native_capture 区分有效原始字节与便携投影；进入原生历史保存，不把转换能力等同于读取能力 |
| 旧 CNS1 原生专用存储 | 格式过滤后可能只剩自动生成的摘要，也返回保存成功；混合格式可能丢掉其中的原始字节 | 没有原生部件，或任一已捕获非控制格式被存储策略过滤时，在提交前拒绝；既有支持类型的原始字节仍可存储。不扩大 Windows 格式白名单 |
| MissingFormats 重试 | 在来源分类及 generation supersede 前等待 provider | 先识别 SelfWrite；外部/不明来源先 supersede，再按该 revision 的预算重试。SelfWrite 不因缺格式误入外部广播 |
| 延迟读/转换结果 | helper 可能读到与调度观察不同的稳定 revision；失败重试可重建旧 revision 的预算 | before/after 必须属于调度 observed_revision；完成后再次核对当前 OS revision 和 wake recovery generation。旧结果丢弃，不能 ACK 新复制；过期 retry 不重建或消耗新复制预算 |
| 保存等待与发送 | 保存/故障记账等待期间可能有新复制或显式恢复 | 存储失败仅在原 revision 仍当前时重试；成功历史保留，自动发送前核对 OS revision 与 capture generation，并在实际 enqueue 前再次核对 |
| 自动接收准备 | 先占 decode slot/准备，再检查授权及 generation | 原生准备前检查授权、generation 和 OS revision；撤销拒绝，过期完整事件仅入历史并回 Superseded。原有发布前复查、claim-before-I/O 与五类结果不变 |
| 空/禁止采集决策 | 部分路径只有 ACK，或把 Suppressed 当读取失败 | 空剪贴板使旧 generation 失效；Windows 对合法 Suppressed 帧保持无正文、无历史、无广播、无失败提示的终结决策。不宣称 Windows 敏感格式原生实现已验收 |

保存结果的诊断分开记录 history_saved、native_formats_admitted、portable_convertible、archived_resource_count、read_incomplete、origin。`native_formats_admitted` 是平台格式准入事实，**不冒充整组可恢复**；本机默认/显式恢复资格仍由 DB CompositeInfo 与 RestorePlan 的完整性、资源和目标检查决定。已归档资源数也不证明所有外部依赖已闭合。

存储拒绝文案也区分容量/可用性与内容不受支持，保留从来源应用重新复制或尝试其他复制格式的可执行建议；不把格式策略拒绝说成必然满盘。

两个新增提示种类已接入 Swift/React 和中英文：capture_origin_unknown 表示“已保存，但来源不明，未自动同步”；capture_not_shared 表示“已保存，但同步内容准备失败”。真正的读取失败仍为 capture_failed，存储失败仍为 capture_not_saved，未知写入仍由既有 typed publication outcome 表达。没有增加常驻状态标签。

重新复制提示明确指向来源应用；旧 clipboard 被替换后，不提供“重读旧剪贴板”恢复动作。Unknown/Partial 手动再次恢复仍由已有入口创建新 intent；原 capture/content identity、安全闭合重绑定和服务端存在性验证继续执行，不以可见标题或便携 hash 猜另一条记录。

### 13.3 身份与迟到边界

本次读/转换 worker 使用调度时的原生 revision 关联一次观察，并只处理不可变 NativeSnapshot/CNA1；中间结果不合并前一次格式。成功历史由现有 O11 逻辑生成并保存 capture/content identity 与归档版本。没有引入新的并行预览、下载或资源物化任务，也没有仅为日志添加另一套未消费的 ID。

已实现的迟到历史摘要更新仍使用完整 HistoryIdentity 的事务 CAS。已有用例验证旧版本、错误 capture/content、删除记录均拒绝更新，合法更新不改变原 capture/content/hash。**这不是 O04 下载或载荷补齐已经实现的证据**；将来的资源 worker 必须持有同一记录及归档版本，不能重写系统剪贴板、绑定新记录或复活删除内容。

origin-before-dedupe 及不同外部 revision 的相同内容仍允许形成新事件；完整历史内容去重不承担来源分类。缺回执继续是 Unknown，只有明确 NoEffect 才允许自动重试原生操作；现有 journal、helper 期限/kill/reap、publication permit、取消及授权 watcher 未被替换。

### 13.4 失败证据与测试维护

最初先把两平台 NativeOnly 的既有通知逻辑提取到共用生产函数，保持原错误行为，再运行真实 DB/native-only 回归。`capture-red` 实际执行 1 项，exit 101，失败为“saved native bytes are not a failed read”；记录没有被覆盖。

后续检查还保留了旧 CNS1 只保存摘要、原件读取为 None 的失败，以及本轮处理中“存在已捕获原件＋失败 PNG”被错误拒绝的失败；后者修正为保留已有原件并报告部分读取失败。Windows UI 的一次失败来自测试 mock 为 expired_event 添加 `{peer}` 后缀，修正了期望文本；产品提示映射未为测试放宽。文案收尾时 Swift 原有恢复动作断言再次发现缺少“重新复制”，最终文字恢复了明确动作而未放宽该断言；React Space 用例也有一次时序失败，核对发现它没有等待点击后的 load effects，改为 async act 等待同一点击后的状态更新，保留单次 Space、防默认及 preview 调用断言，未改产品键盘行为。全部尝试都列在 validation-results.json，失败与最终矩阵分开。

新增 4 个 Rust 回归：

1. 新 revision/OS sequence 变小、不可用 revision、迟到 retry、混合 before/after，不重绑定或消耗新复制预算。
2. 有效原始字节但便携 UTF-16/未知类型转换失败，与 provider 全部失败、非法快照分开。
3. 真实 CNA1/旧快照存储往返、原生保存无误报、部分读取、来源不明、同步准备失败，以及真实 SQLite `query_only` 写拒绝；保留原件，peek 不消费，ACK 仅移除对应 warning。
4. 占满该 peer 的 decode slot 后，已撤销事件仍先拒绝，过期事件仍可只存历史并回 Superseded；两种情形 native write 都为零。

复用并扩展既有 UI 测试：React 可见通知用例改为参数表，新增两种通知的实际呈现及按 ID ACK；Swift 既有双语本地化表增加两条新键及原有未保存键，没有另建重复测试。既有隐藏窗口不 ACK、Unknown 手动重试、身份重绑定、晚到 CAS、写后丢回执、NoEffect/Partial/Unknown 和 revoke 取消回归继续通过。没有新增 ignore、skip 或放宽测试过滤；Rust 显式 ignore 注解 11→11。

结构可见性 guard 更新为当前 macOS 的两个 add_native_archive 保存分支及 Windows 两个实际接口，分别检查成功事实与存储失败通知。它只证明接线路由；SQLite 拒绝是独立行为回归，物理满盘不是本次证据。

### 13.5 最终验证

最终矩阵 **18 条命令全部 exit 0**，每条运行前后源码指纹均为本节最终值。中间失败不能计入这些最终结果；重复的 focused/full 运行不加总为新增测试。

| 检查 | 实际结果与边界 |
|---|---|
| Core enabled＋repair＋两项传输扩展，serial | 657 PASS，4 既有 ignored；含新增 4 项及现有因果/journal/晚到 CAS 回归 |
| Core integration | 11＋3 PASS；真实 claim/history 顺序、revoke、取消与恢复路径 |
| Runtime all-features＋Core composite | 27＋7＋11＝45 PASS；输出、revision、较新恢复、Partial/Unknown/NoEffect 与手动恢复边界 |
| macOS Rust enabled | 118 PASS，3 既有 ignored；编译生产 monitor/adapter，未启动完整 App |
| Windows Rust on macOS host | 127 PASS，2 既有 ignored；不认证 Windows 原生 |
| React History | 49 PASS；含可见/隐藏通知 ACK 和 Unknown 显式重试 |
| Swift localization＋retry identity | 13 PASS；编译 UI，验证双语及身份解析，不等于 GUI/系统通知验收 |
| workspace all-targets/all-features Clippy | PASS，-D warnings；workspace 0 warning |
| production-features／canonical／local contracts／native policy | PASS；12 manifest、9 canonical 源、现有契约与策略保持同步 |
| capture visibility／TypeScript | PASS；结构路由及 tsc build |
| workspace＋双平台 fmt／diff-check | PASS |

Windows host 编译保留一条既有 warning：clipboard_file.rs 的 Win32/InvalidData 分支在 macOS 不构造；同一告警已存在于第 12 节原始日志，文件本轮未修改。未通过删除 Windows 分支或隐藏 warning 达成表面清洁。

schema、CNA1/CNS1、helper、网络/local contract 版本不变。两种 warning kind 使用已有开放字符串字段，未手改 generated 文件。18 条命令的准确 argv、耗时、实际测试数与日志摘要可从证据独立复算，最终选择为 *-complete-v3。

O01/O02/O06/审查整改四份历史封存复算为 1083/1397/2874/851 文件，均无不匹配；已有包没有变化。

### 13.6 验收判定与下一项

| 验收项 | 本次判定 |
|---|---|
| A02 | 生产函数＋真实 DB 保存/拒绝＋两端提示自动部分 PASS；正常本机可恢复原件不因转换不支持误报警，真失败可见。真实应用提示发出方、完整 GUI/系统通知 UNKNOWN |
| A16 | revision/generation/重试及现有归档摘要 CAS 自动部分 PASS；本轮真实慢 provider/快速原生复制、完整 live monitor 竞态 UNKNOWN。O02 历史原生证据保持封存，但不当作本轮新包验收 |
| A17 | journal、claim、写后丢回执、Partial/Unknown/NoEffect、revoke 及显式重试自动部分 PASS；完整 GUI、真实故障包/live adapter 和双设备 UNKNOWN |

O10 达到 **LOCAL_COMPLETE（本机实现与上述自动部分）**。没有新包、真实 QQ/目标应用、Windows 原生、Mac↔Windows、双设备或 hosted CI 证明；物理断电/满盘、O04 实际资源下载与 lease/GC 仍 UNKNOWN/后续任务，Release **NO-GO**。

**本项完成后暂停；下一项按原计划为 O03：保留原始图片编码及动画。本轮不启动 O03。**


## 14. DeepSeek max 全阶段复核与整改

用户要求复核今天所有工作并按审查修复。原始报告见 [DeepSeek 审查](COMPOSITE-CLIPBOARD-DEEPSEEK-REVIEW-2026-10-06.md)，逐项核实与验收见 [整改验收](COMPOSITE-CLIPBOARD-DEEPSEEK-REMEDIATION-2026-10-06.md)。managed 子代理未识别任务，实际审查由显式 DeepSeek/max 的独立只读执行进程完成，原始记录保留。没有把其发现自动当作事实。

- 接收侧未保存提示、三个无调用发布接口和策略重复已修正；完成时的过期采集 gate 符合 ADR-005 §12，保留并明确准入边界。
- 主代理另修复：240 MiB 采集 scratch 导致 >16 MiB 合法载荷自拒绝；检查与实际入队之间的发现/锁/背压等待竞态；CSS import/编码与 SVG 引用漏判。共用预算交接、最终队列 gate 和 HTML inventory 接入两平台。
- 4 条语义 RED、5 条中间编译失败与全部后续尝试均保留。新增 3 个行为回归，接收/双端 UI 复用已有测试扩展；ignore 11→11。
- 最终源码 `364fbe7819331b2e28f3eb2477e219f1a32a691dba3e4469fec58493bb984aec`，771 文件；本轮源码/测试增量 22 文件，+376 / −144。
- 19 条最终命令全部 PASS：Core 660＋4 ignored；integration 11＋3；Runtime 45；Mac Rust 118＋3 ignored；Windows Rust on Mac 127＋2 ignored；Protocol 42；React 50；Swift 13；Clippy（-D warnings）、features/canonical/contracts/policy/visibility/tsc/fmt/diff-check 全通过。每条运行前后源码一致。
- 证据为 `artifacts/clipboard-improvement/2026-10-06-deepseek-review-remediation/`。历史 S0/O11-D1/D2/O01/O02/O06/cleanup/O10 八份封存内容摘要复算一致；S0 manifest 使用已记录的 .txt 路径映射。没有修改旧 seal、替换测试包、提交/暂存/重置或访问通用剪贴板。

本轮整改达到 LOCAL_COMPLETE（本机自动部分），真实 GUI/QQ/目标应用、Windows 原生、Mac↔Windows/双设备、hosted CI、物理断电/满盘及 O04 实际资源归档继续 UNKNOWN/后续，Release NO-GO。**本轮完成后暂停；下一项仍为 O03，不在本轮启动。**

## 15. O03：图片原件与动画，首轮实施及独立复核

用户指定 DeepSeek/high 实施；内置 Multi-Agent 使用完整明文 message、fork_turns=none，任务回执成功。141 次子代理命令均使用唯一工作树；首轮交接后冻结并完整快照，主代理独立复核并二次修改。

源码：`364fbe78…`（771 文件）→ 首轮 `b9642eb1…`（772）→ 最终 `92dfd9399876deaf3fdce0148eb66bcc55e58cda80cad8c2c9d8a782b69c7a29`（783）。相对基线 22 个源／测试／配置文件变化，零删除。

- 原件与静态 PNG 封面继续采用 CNA1 分角色归档。JPEG/GIF/WebP 通告类型提升至既有图片 32 MiB 字节准入；仍遵守整份 40 MiB 归档限额。原件回放不使用封面解码预算。
- 独立复制源码复现四项失败：截断 acTL 越界、单帧 APNG 误判、TIFF 循环、后续大页面预算漏算。二次修复容器范围／终点／逐页预算与溢出，并明确元数据最多 200 帧／4 MiB 扫描，封面仍最多 20 帧／8 Mi 像素。现有静态 PNG 对 APNG 的拒绝不变。
- 限定封面来源与静态 PNG 输出，保护已有静态 PNG 原件；纯图片原件元数据与封面角色接入现有加密摘要及正式历史列表。原件指纹与分类两侧一致，超便携预算 PNG 使用原件域。
- 删除仅自身测试使用的辅助 API，统一 TIFF／静态封面解码，使用 10 个真实固定图片替代不完整头部成功样本；无新增 schema／manifest 字段或 wire 编码，冻结旧摘要回归通过。
- 最终 18 项门禁全部 exit 0 且源码前后不变：Protocol 48；Core 666＋4 ignored；integration 14；Runtime 45；macOS 118＋3 ignored；Windows macOS-host 127＋2 ignored；Clippy、三份 fmt、实际 helper 图像函数 57 个断言、完整 helper typecheck、features／canonical／contracts／policy／visibility／diff 均通过。保留 Windows 宿主既有 dead-code warning；不增加 ignored。
- 原件字节／帧数／身份经加密历史重开保持，正式列表显示原图信息与封面。GIF/APNG/JPEG/TIFF/WebP 的 ImageIO 首帧封面、透明度／颜色采样／尺寸与预算通过。本次没有 GUI、Windows 原生、双设备或 hosted CI 证明。
- 初版 parser 四项 RED、主代理错误摘要测试 RED、子代理完整命令／输出及其首轮私有板探针保留。最终判定仅采用主代理不可覆盖的 `review/commands/*-final-v1`；子代理同名重跑记录不充当最终源码验收。最终 native 发布未重跑，仍 UNKNOWN。
- 十份旧 seal 共 8384 文件复算一致，S0 使用既有 .txt 映射；日常 App、general 剪贴板、用户库及旧证据不变，未暂存／提交。

证据：`artifacts/clipboard-improvement/2026-10-06-o03-image-implementation/`；完整支持范围、生产链、测试与回退：[O03 验收文档](COMPOSITE-CLIPBOARD-O03-ACCEPTANCE-2026-10-06.md)。**判定：O03 LOCAL_COMPLETE，完整 A12 UNKNOWN，Release NO-GO。本轮停止；下一项 O04 未启动。**

## 16. 最新审查发现核实、DeepSeek/high 修复及主代理二次完善

用户要求先核实，再由一个 DeepSeek/high 子代理修复成立项，等待完全交接后由主代理二次修改。唯一子代理 `01a11083-e9c9-75b0-ad67-d8ea11f2313b` 使用 `cline-pass/deepseek-v4.1-flash`／high，精确回执唯一目录后接收完整明文任务。完成全部写入后关闭，主代理再独立审查增量与执行复验。

- **P1-A 数据丢失结论不成立。** 输入源码上的真实 4096×4096 PNG 已能 NativeOnly 加密入库、关闭重开并逐字节作为原件恢复输入。上轮漏读 `preserve_native_conversion`。本次新增正式 DB 回归保护纯 PNG／PNG＋文字，不为误判改变产品策略。
- **P1-B 现象是已明确合同，而非本轮确定缺陷。** ADR-005 §12／§13 要求完成时 revision／wake 仍当前，准入前的临时观察被替换可丢弃。generation／retry／origin-before-dedupe 门继续保留，没有承诺逐次 OS 复制全入历史。
- 成立项修复：控制集合和原件域集合收敛、图片名单／封面预算进入 Rust/Swift 生成策略、测试专用 opaque API 收回、MIME helper 收窄、identity 显式错误／HTML 括号、helper 测试函数边界与真实大 PNG 回归。采集／去重的 Mime 资格不同，不错误合并；CSS withheld、资源门和 receive 接口保持。
- 主代理发现首轮生成器允许 1000 帧／32 Mi 像素，独立探针真实 RED；补锁 20 帧／8 Mi 像素上限，扩展正式负向和冻结旧名单／原件域资格回归。第一轮最终矩阵还检出 rustfmt 与本轮 probe 普通依赖启用 test-support 两项 FAIL，原始输出／probe 保留；修正格式及 integration test＋dev-dependencies，未弱化扫描门。
- 源码 `92dfd939…`→交接 `65a127d3…`→最终 `8ea8cb276d20195d6e62e03259ea89045e5b6c7083af8f158da68120f9414ad2`，均 783 文件。基线到最终 16 文件变化，+327／−117，零新增／删除产品源文件；分阶段快照与本轮 patch 独立保存。
- 最终新 ID `*-final-v2` 的 23 条命令全部 exit 0 且源码不变：Protocol 51；Core enabled 668＋4 既有 ignored；integration 14；DB all-features 150＋2 既有 ignored；Runtime 45；Mac Rust 118＋3 既有 ignored；Windows Rust on Mac 127＋2 既有 ignored；generator 5、解码上界 2、图片封面 57、完整 helper 编译／self-check、Clippy／三份 fmt／policy／canonical／contracts／features／visibility／diff 全通过。各套测试有重叠，不合计唯一数。
- 新 helper 私有合成板原件往返 probe 实际 1 PASS：Core originals RestorePlan／supervisor／helper 发布后回读字节与原件域 fingerprint 一致，进一步证伪大 PNG 原件丢失；不宣称完整 App／journal／目标应用已验收。纠正子代理“Windows crate 不能在 Mac 编译”表述；Windows-only 原生分支仍 UNKNOWN。
- 11 份历史 seal 共 10,204 文件复算一致；S0 只沿用既有 .txt 映射。全部尝试和失败日志保留，以本轮新 seal 保存最终证据；没有暂存／提交／reset/clean、替换日常 App、访问 general 剪贴板或用户库。

证据：`artifacts/clipboard-improvement/2026-10-06-review-verification-fixes/`；[逐项核实与复验报告](COMPOSITE-CLIPBOARD-REVIEW-VERIFICATION-FIXES-2026-10-06.md)。**判定：本轮 LOCAL_COMPLETE，真实 GUI／QQ／目标应用、Windows 原生、双设备、hosted CI 仍 UNKNOWN，Release NO-GO。本轮停止，O04 未启动。**

## 17. O04：DeepSeek/high 资源归档与主代理二次修复

用户继续授权下一阶段，唯一子代理 `01a11129-e27c-7830-a1fd-2df12642df33`／`cline-pass/deepseek-v4.1-flash`／high 在指定 composite-clipboard 工作树实施 O04。先核验实际兼容开关和精确明文回执；等待完整交接并关闭后，主代理冻结源码再独立审查和修改，未进入 O05 或 UI 反馈 F01–F06。

- 输入 `8ea8cb27…`（783 文件）→交接 `e891c037…`（784 文件）→最终 `d1ffeaf807db1df5159bd3b16a2a46cf94f768c3523eb50a2de56a7a3b0c2f86`（786 文件）。输入到最终 45 文件改变，交接到最终 27 文件改变，各阶段 inventory／增量 patch 保留。
- 独立八项 RED（0 PASS／8 FAIL）证实 mapped／reserved IPv6、歧义 HTML、MIME 漏检、pending deadline、MissingFormats 升级、重复下载及 DB-lock 取消问题；最终原探针 **8 PASS**。另补 Manifest 3／v2 digest 兼容、严格派生证明、完整 SQL identity＋加密 manifest CAS、quota／payload 回滚、Runtime reopen-during-fetch、partial retry、CNA1 v3 reader、owned 列表／预览、生成 report、Swift timeout／request_id 和两端诚实反馈／迟到 session 隔离。
- 23 条 `*-final-v1` 命令全部 exit 0，前后同最终源码；canonical runner／命令／最低测试命中／日志 hash／既有 skips 与 warnings 复算无误。Protocol 51、Core enabled 692＋4 ignored、integration 14、DB all-features 158＋2 ignored、Runtime 51、独立 8、public/native 2、Mac Rust 118＋3 ignored、Windows on Mac 127＋2 ignored、React／decoder 58、完整 Swift 267＋4 skipped、policy 负向 5，以及 Clippy／三份 fmt／生成／契约／features／visibility／tsc／diff 全 PASS。套件重叠，不合计唯一测试数。
- 实际文件 DB 保存、原件／身份／收藏保持、关闭重开、离线 RestorePlan 与新编译 helper 私有 board 回读验证双图位置；另通过生产 Reqwest public HTTPS 获取两个 URL 同字节并去重。general clipboard／日常 App／用户库不动；不是完整 App 或目标消费验收。
- agent 早期改写方式、raw Cargo＋tail 丢失 exit、并行全局 budget 失败，以及主代理 fixture／request_id／CNA1 reader／Clippy／probe API 的全部失败和重跑保留，不降低门禁、不加 allow／ignore／skip、不放宽预算。Windows-host 的一条既有 dead-code warning 单独披露，Windows 原生仍 UNKNOWN。
- 补充默认 feature Core 全 lib 679 PASS＋4 既有 ignored（0 filtered），同源码 exit 0；文档 diff-check PASS。12 份历史 seal、11,193 文件 **0 mismatch**；S0 沿用既有 .txt 映射。单一共享 target 约 3.7 GiB＋关闭 debug／incremental，避免多份巨型缓存；未 stage／commit／reset／clean 或替换旧包。

证据：`artifacts/clipboard-improvement/2026-10-06-o04-resource-archive/`；[完整 O04 报告](COMPOSITE-CLIPBOARD-O04-RESOURCE-ARCHIVE-2026-10-06.md)。**判定：O04 本机实现与所列自动／接缝部分 LOCAL_COMPLETE；只支持公开 HTTP(S) 静态 PNG。A13/A14 真实 GUI／来源／Word／QQ 等目标、Windows 原生、双设备与 hosted CI 仍 UNKNOWN，Release NO-GO。本轮停止，O05 未启动。**

## 18. O05：来源授权文件桥、独立质疑与二次补修

- 用户授权沿用 DeepSeek/high 实施、主代理等待完整交接后独立复核并二次修复。唯一 agent `01a11192-14ae-77d2-b429-f6d7e10a06ce`，`cline-pass/deepseek-v4.1-flash`／high／不 fork context；实际明文递送与 task-id／唯一工作树回执已核实，完成后关闭，主代理才改产品。
- 输入 `d1ffeaf8…`（786 文件）→交接 `262324d7…`（794）→最终 `a996992fc3922c4fe37164f7241fee40780d751c3dde04042f396fb396d91096`（796）。输入到最终 51 文件改变，交接到最终 25，inventory／精确增量 patch 保存。唯一工作树和 HEAD 保持，旧 App／DB／packages 不动。
- 独立 Rust 0 PASS／2 FAIL、JS 0 PASS／4 FAIL，证实 origin proof 漏核、partial 旧 proof 丢失、srcset 换图、navigation 绑定漏核、合计读取后置、fetch 截止缺失。全部二次修复，最终原探针 2 PASS／4 PASS；另加来源→公开补缺、regular-file／FIFO／symlink、capacity／重放、撤销／过期／取消／重开等待、target id、late picker／grant、实际 socket 取消、popup／body／DOM／PNG chunk 回归。
- 交付严格限定手动 file-based bridge：explicit ticket→可选 MV3→packet→authenticated id＋path 导入。8 live grant／300 秒／一次消费，64 KiB ticket、24 MiB packet、8 节点／8 MiB 单图／16 MiB 合计；10 秒 request／30 秒 collection／45 秒 import。保留父 PID／私有 token／1 MiB line caps；不加 CORS、localhost HTTP、Cookie/token 共享或 native messaging。
- Manifest **4** 与最小 SourceProof 明确版本化。read／commit 验证原 origin＋src hash＋index／span，partial 累计 proof 及 source→public 层保持；原 HTML／CNA1／hash／capture/content/dedupe／source／favorite 不改写，v2/v3 冻结合同及 schema／helper／网络版本不变。JSON/base64 与派生准备移到有界 owned worker，admission／共享 256 MiB lease 随 worker 存活；commit 仍有严格验证锁开销。
- **29 条最终同源码命令 PASS，验证器 0 errors。** Protocol 51，Core default 688＋4 ignored／enabled 701＋4，DB 161＋2／integration 14，Runtime 62，macOS Rust 118＋3／Windows-host 127＋2，React 63／Swift 269＋4；O04 原独立探针 8、O05 原 Rust 2／JS 4、extension 10、auth guard 3、capture policy 5。Clippy `-D warnings`、fmt／contracts／features／visibility／TypeScript／diff 同组通过。Windows-host 保留一条既有 dead-code warning，不当作 Windows 原生验证。
- 实际文件 DB／加密 CNA1／收藏，source partial→source→public 三图不同颜色，重开保全与 offline RestorePlan 后，用本轮新编译 helper 在 `tailsync-probe.o05.*` 私有 board 发布／读回 **1 PASS**。helper source `a7d82341…`／binary `2ac5d725…`；没有日常 clipboard 或真实网页／目标 App 副作用。source_peer 保全是代码确认，不虚列探针独立断言。
- v1 auth 静态 guard 将 Windows `st` 误写成 `state`，原 FAIL 保留；纠正为逐平台完整拒绝 block／dispatch 顺序对比，v2 29 条重跑全绿。早期混源诊断不作最终验收。agent log-21／log-53 部分失败被重用文件覆盖的缺口照实披露，无法恢复的原 log 不补造，不使用作验收证据。
- 13 份旧 seal／12,226 文件复算 **0 mismatch**。单一 `target/o04-shared` 约 4.2 GiB，关闭 debug／incremental，Rust 串行，Rust／Swift／Vitest 走隔离 runner。文档与各版失败／探针／命令／源码／helper／CNA1 evidence 另行封存；未 stage／commit／reset／clean 或继续其他阶段。

证据：`artifacts/clipboard-improvement/2026-10-06-o05-source-context/`；[完整 O05 报告](COMPOSITE-CLIPBOARD-O05-SOURCE-CONTEXT-2026-10-06.md)。**判定：可选手动同源静态 PNG 文件桥本机 LOCAL_COMPLETE；真实 A15 UNKNOWN，Release NO-GO。本阶段停止，等待二次复查。** 文件 scope 证明不是 browser signature／attestation；响应丢失的未交付 grant 只能受 TTL／capacity 限制，Windows session close 只验证 late UI 隔离。真实扩展加载／登录或 blob 页面、GUI／目标 App、Windows 原生／双设备／hosted CI 未验收；无 QQ 专用适配、O07／O09／F01–F06 或自动发布。
