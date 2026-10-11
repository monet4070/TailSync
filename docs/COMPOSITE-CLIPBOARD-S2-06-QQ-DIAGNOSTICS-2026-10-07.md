# S2-06：QQ / 系统提示诊断准备、独立审查与二次修复

日期：2026-10-07。唯一工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`。

## 1. 判定与停止点

**本轮离线工具与人工验收准备 LOCAL_TOOLING_COMPLETE；S2-06 的真实闭环、A07、A02 现场仍 UNKNOWN，缺真实来源/提示材料。Release NO-GO。** 不宣称修好了 QQ，也不自动进入 S2-07。

首先补完 S2-05 收尾：其最终 v6 的 25/25 自动门禁自然完成，现场复算源码一致，报告已定稿；7318 个证据文件封存，aggregate `73ff7ab0e047f145ed598dc6f585f7b5a9caa465d4df07c0e735f6cfb12cff64`，只读复算通过。S2-05 的完整 A05/A06 仍 UNKNOWN。

S0 只有合成 RTFD / 私有 pasteboard 探针和历史已安装 QQ 元数据；S2-00 也没有真实 QQ 复制—同目标粘贴证据。既有反馈的提示原文/时间/发出方未提供。本轮已经询问用户可公开测试消息、版本/复制动作与提示材料；没有授权偷偷读取聊天、用户 DB、来源缓存或 general clipboard。缺真实材料不妨碍有界工具准备，但阻止根因归因与完整现场验收。

## 2. 复用流程与源码绑定

一个内置 DeepSeek/high：`cline-pass/deepseek-v4.1-flash`，子代理 Boole `01a11558-3f07-73a3-a6eb-fe5ffe6e1f41`，`fork_context: false`。完整明文任务、唯一工作树和边界先送达，核验 CPA 开关、实际模型/effort、任务全文和 ACK，随后 CONTINUE。子代理初始 shell cwd 是另一 checkout，因此全部命令明确要求本工作树；交付环境和源码差异均独立核对。完整 HANDOFF 后关闭，父再冻结、复现、二修，没有并发修改。

分支 `codex/composite-clipboard`；HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。

| 冻结点 | 完整源码指纹 | 文件数 |
|---|---|---:|
| 本轮输入 / S2-05 final | `b011a5126494e9c3ecdc8799b431a45a2c93cdabea3e355b6483980a9396a7f3` | 833 |
| 子代理交付 | `77b13265ea991f3a1e45e3d1eadc66a66c0784c7cf4a1a9e5690f35f7f0f32cf` | 835 |
| 父最终 | `b110e01de38da668da433328ce3c4b1972136f958db48f145d1ead8a8b8f93ce` | 835 |

before→agent：仅新增两个 Python 文件；agent→final：只修改这两个文件；before→final：两个新增文件，零旧文件修改/删除。输入的 833 个既有产品/测试/工具/配置字节全部保留。文档/artifacts 另封存，不改变 fingerprint。

## 3. 本轮交付

- `scripts/validate-s2-06-evidence.py`：stdlib、显式清单与 evidence root、只读离线验证；生成的空模板不是采样证据。
- `scripts/tests/test_s2_06_evidence.py`：最终 50 项，包括原 23 项和父新增 27 项持久回归。
- [人工验收指南](COMPOSITE-CLIPBOARD-S2-06-MANUAL-ACCEPTANCE-2026-10-07.md)：逐层定位、候选/source/helper 绑定、五类 QQ 内容/动作、同来源同目标直接/历史对照、四种持久条件与提示归因。

清单校验只输出 INVALID / INCOMPLETE / READY_FOR_MANUAL_REVIEW。READY 只证明申报结构和引用字节长度/hash 一致，不认证来源真实性、回执语义、实际启动副本、修复成功或 Release GO。自造材料即使声明 `real_qq` 也仍输出 A07/A02 UNKNOWN，必须由人工核对；测试中的这种伪标签是反证边界，不是真实 QQ ledger。

准入：清单 64 KiB、单文件 16 MiB、累计引用 64 MiB、128 引用、单格 32 引用、样本/提示各 64。授权未确认不打开 receipt。POSIX root fd 逐层 O_NOFOLLOW、普通文件 fstat、O_NONBLOCK、有界读取及前后元数据检查；不能用 symlink、FIFO、增长文件绕过。系统 root 父目录按显式路径规范化，root 内所有链接拒绝；不支持安全原语的宿主 fail-closed。Windows 原生运行该 Python 校验器未验证，不能据此认证 Windows-native。

这些是材料处理限制，不是 App RSS / codec / GUI 性能验证。没有自动抓日志、DB、clipboard、QQ 缓存或联网行为；没有改 Rust/Swift/React、hash/dedupe/schema/network/auth/RestorePlan 等产品语义。

## 4. 父独立复现与二修

交付源码上 `parent-real-red-input` 为 1 PASS / 12 FAIL；`parent-real-red-io` 为 0 PASS / 2 FAIL。末轮候选 `parent-real-red-final-edge` 为 1 PASS / 2 FAIL。共 16 项属实不足，不是凭 HANDOFF 接受结论：

| 问题 | 修复 |
|---|---|
| 恶意 provenance 原文进入错误摘要 | 输出只允许固定枚举，否则 null |
| 未知 CLI 参数由 argparse 回显敏感值 | 固定 E_ARGUMENTS JSON，不反射参数 |
| 额外字段 / 伪 PASS claim 被静默忽略 | 顶层、样本、各格子与 receipts 逐层字段白名单 |
| 重复 JSON key 覆盖 synthetic 等早期声明 | unique object parser；非标准数值拒绝 |
| direct/history 的目标名/版本缺失仍 READY | 两侧非空身份，缺项 INCOMPLETE，错目标 INVALID |
| scope none 配合两个 true 标记仍视为授权 | 显式 scope 与 consent；未授权不读取 receipt |
| 未观察 stage 仍 READY | observation 与 result 分开；not_observed / unknown 保持未完成 |
| 没有候选构建源码指纹仍 READY | 增加 source_sha256，并明确人工核对实际 build/start 证据 |
| 错会话可以拼入样本 | run/session 绑定、唯一 source_action_id；各 stage/pair/condition 与提示校验关联 |
| root 内父目录 symlink 绕过末级 symlink 检查 | 每级 dir-fd no-follow，不依靠 resolve 后再 path.open |
| tomorrow 等字符串可以充当提示时间 | 严格 ISO 8601 带时区 |
| 真实 capture_failed 小写稳定码被拒绝 | 保留实际大小写；不诱导编造新码 |
| stat 后替换文件为外部 symlink，旧代码仍打开 | 实际 fd-root / no-follow；新回归在最终 stat 后模拟换链仍拒绝 |
| manifest 只 stat 限额，read_text 可在增长后无界读取 | fd 实际长度检查、有界读取、前后元数据；不再 unbounded read_text |
| 零字节文件可以当已观察 receipt | E_RECEIPT_EMPTY；未观察格子可以合法留空，不伪造证据 |
| 原提示换行/制表符被拒绝，迫使改写 | 有界保留原文，摘要永不回显正文 |

IO RED 使用 tempfile 中自造公开攻击样本和真实文件替换/增长时序，不读取真实用户内容，也不临时削弱产品 guard。最终另有真正 fd-stat→open 换链、读取中增长、FIFO、累计预算前置、未授权不读取等持久回归。

原始 deep JSON 和后期 NUL-root 假设各自实际 PASS，没有认定为原始缺陷；继续保留兼容探针。真实失败用 observed + failed，仍可进入人工审查；明确观察时段未出现提示可附 receipt 并留空发出方/正文/日志码，避免伪造提示，但 A02 依然 UNKNOWN。

## 5. 最终验证

选定 **8/8 同源码门禁通过**，每份最终 command record 的 before/after 均为上表父最终指纹，exit 0：

| 门禁 | 实际结果 |
|---|---|
| 工具聚焦 unittest | 50 PASS / 0 FAIL / 0 skipped |
| scripts/tests 全套 Python | 119 PASS / 0 FAIL / 0 skipped |
| 父独立 input / IO / edge 重跑 | 13 / 2 / 3 PASS |
| 独立 CLI 子进程语义 | 9 个实际断言通过；不是 0-test 冒充 |
| 源码/规范/旧 seal 审计 | 833 既有文件不变；六个旧 seal 20499 个 inventory hash 与绑定文档复算通过 |
| 最终 diff | git diff --check 通过 |

聚焦、全套和独立探针有重叠，不相加声称独立测试数量。由于本轮只增加离线 Python 工具/测试和文档，不运行 Cargo、Swift、React 或构建候选；S2-05 历史 25/25 保留原始源码绑定，不改称 S2-06 新指纹全产品矩阵通过。

失败与限制原样保留：

- 明文 receipt 首次父解析只识别 agent_message，但实际输入为 user/input_text，断言 exit 1；第二次解析核实完整任务与 ACK，不是传输故障。
- parent-green-v1 的新增语句插入到 target_version 分支中间，导致 IndentationError；两项导入实际 0 test、unittest 为一个 _FailedTest，均不是 GREEN。修正位置后重跑；日志不覆盖。
- 两次 apply_patch 因上下文顺序不匹配失败，未落盘；修正后应用，不当作产品测试结果。
- 首个最终 audit 假设所有历史 seal 都有 aggregate_sha256，O05 的已知旧 schema 实际仅 file inventory，KeyError/exit 1。新 audit-v2 仅允许该确切历史 schema，复核每个文件与 seal 自身 hash，其他五个 seal 仍强制 aggregate；不改写/补造旧证据。本轮产品源码未变，最终选择该新审计记录，保留旧失败。
- 子代理声称 own evidence 已 sealed，但交付目录没有独立 SHA inventory；父只把它作为未封存交接输入，冻结后在本轮最终 seal 纳入，不继承口头 sealed 声明。

## 6. 保全与后续输入

证据：`artifacts/clipboard-improvement/2026-10-07-s2-06-qq-diagnostics-r1/`。before/agent/final 全量快照、精确三份增量、所有命令/失败/raw logs/source inventory、明文任务/ACK/模型配置、指南交付版与父定稿、最终 SHA seal 和只读复算保留。没有 stage/commit/branch/reset/clean，未清缓存或旧包，未操作日常 App。

下一步不是 S2-07：先取得用户在场授权的公开 QQ 测试消息、版本/动作、隔离候选及来源/目标材料；如出现系统提示，给原文/时间/发出应用与可追溯稳定码。不提供这些材料时保留现场 UNKNOWN，不猜测 QQ 特例；材料齐备后才按共享分析→保存格式→显式临时资源能力的实测失效层修复，再做同目标与关闭/断网/重启对照。
