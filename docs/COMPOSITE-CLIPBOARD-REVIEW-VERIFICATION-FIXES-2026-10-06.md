# 审查发现核实与修复复验

日期：2026-10-06，Asia/Shanghai。用户授权先核实上一轮审查，成立项由一个 DeepSeek/high 子代理修复，待其全部交接后由主代理独立复核与二次完善。

工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`；分支 `codex/composite-clipboard`；HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。输入源码指纹 `92dfd9399876deaf3fdce0148eb66bcc55e58cda80cad8c2c9d8a782b69c7a29`，783 文件，与 O03 最终封存一致。

证据目录：`artifacts/clipboard-improvement/2026-10-06-review-verification-fixes/`。保存本轮输入源码、独立探针、命令原始输出及历史封存复算；测试使用现有隔离器。以下判断由当前调用链和实际执行重新得出。

## 1. 逐项核实

| 上轮发现 | 核实结论 | 本轮处理范围 |
|---|---|---|
| P1-A：超像素预算静态 PNG 整体丢弃、不入历史 | **数据丢失结论不成立**。上轮漏读 `from_archive` 最后调用的 `preserve_native_conversion`：稳定、有已捕获正文的 `InvalidFormat/Empty/Failed` 转为 `NativeOnly`。monitor 的 native-only 分支保存 CNA1，DB 接受同一结果。 | 以 4096×4096 fixture 实际加密保存、关闭重开、原件恢复输入逐字节确认；新增正式回归保护这条生产路径。 |
| P1-B：完整采集在完成时被超越而丢弃 | **行为存在，属于已明确的采样准入规则**。ADR-005 §12 要求完成时 revision/wake 仍当前，§13 澄清保存前和保存后的边界；没有承诺逐次 OS 复制全部入历史。 | 保留当前 gate、generation 和 retry。上轮提出改变规则的建议没有证明现有规则违反用户要求，本次不按确定缺陷处理。 |
| P2-A：控制格式两名称集合重复 | **成立，维护风险**。生成 policy 已有 CONTROLS，多处纯集合判断仍重复。 | 将等价集合谓词收敛；source token 与转换 marker 各自的单名称查询仍表达具体用途。 |
| P2-B：原件域名称集合重复 | **成立，维护风险**。当前与 legacy-storable 加 controls 组合等价。 | 复用现有名称谓词，冻结旧摘要、原件域和权限差异。 |
| P2-C：图片名单和封面预算跨 Rust/Swift 重复 | **成立，维护风险**。现有 policy 生成没有覆盖封面名单与预算。 | 纳入现有生成/check；保持原额度、别名识别和 metadata/解码预算分工。 |
| P2-D：身份和采集的名称→Mime 映射重复 | **不能按等价规则合并**。身份去重只允许闭合 text/html/png；采集映射还含 RTF 和 Windows 格式。 | 保留不同语义，避免给 RTF、对象或旧 CNS1 扩大安全去重资格。 |
| P2-E：图片 MIME/预算 API 属于死代码 | **表述不准确**。MIME 函数被 `metadata` 和图片名称检测使用；预算被生产 `within_budget` 使用。`opaque` 确实仅在自身测试使用。 | 收窄无外部消费者的可见性、去除测试专用公共 API；保留生产行为。 |
| 三处 identity unwrap | **可读性问题，未发现可到达 panic**：每处都有 identity 存在的逻辑保证。 | 改为显式错误或绑定，保持 CAS 和 digest 更新顺序。 |
| HTML 布尔优先级 | **可读性问题，当前优先级正确**。 | 显式括号，等价行为。 |
| CSS 转义/实体和 @ 的 withheld | **有意保守拒绝**。尚无 CSS/entity 闭合证明，不能靠放宽字符串检查关闭资源门。 | 保留。 |
| receive 参数过多 | **风格建议**。未显示参数误接或功能错误，结构体重构会扩大本轮范围。 | 保留。 |
| helper 图片测试靠说明注释切片 | **成立，测试入口脆弱**。改变说明注释会破坏抽取边界。 | 改为明确函数边界与必需函数检查，同时加载生成策略。 |

## 2. 独立基线证据

`baseline-large-png-v2` 使用真实 `over-budget.png`（4096×4096、约 75 KiB）：`from_archive` 返回 `NativeOnly`，`add_native_archive` 成功，关闭重开数据库后 CNA1 与原件恢复输入字节完全相同。运行前后源码均为 `92dfd939…`，exit 0。

首轮 probe 错误地用“创建新隔离库”接口重开已存在库，exit 1；这是探针接口使用错误，原始输出保留。改用现有 `open_with_policy_for_probe` 后通过，未据此修改产品代码或将失败记为产品 RED。

## 3. 委派与交接

唯一子代理 `01a11083-e9c9-75b0-ad67-d8ea11f2313b`，模型 `cline-pass/deepseek-v4.1-flash`，reasoning effort `high`。先无工具逐字回显任务 ID 和唯一目录，随后递送完整明文范围。运行时兼容开关已为 true，本轮未修改代理配置。

子代理完成全部交接并停止写入后，主代理关闭该代理，再独立审查本轮增量与原始日志，没有创建第二个代理。交接源码指纹 `65a127d32ee28a31c54a71dcf5eecef46a63eb0c1ffb6b999f5466dbdbc60480`，相对输入恰为 16 个源／测试／配置文件。交接源码快照、基线到交接 patch 和子代理原始日志均保留；子代理摘要不充当最终源码验收。

### 3.1 成立项的实际修复

- `native_policy::control` 消费生成 CONTROLS，统一归档、内容身份、来源指纹、采集、DB 摘要、恢复和平台支持中的两名称集合判断。具体 source token 查询与转换 marker 查询仍是各自的单名称规则。
- 原件域资格复用 `!(legacy_storable || control)`，没有把原生 publication 准入误当作旧 CNS1 storage 准入。JPEG/GIF/WebP、未知格式仍须进入原件域；RTF 不因此获得安全历史去重键。
- 图片原生格式／MIME 和 20 帧／8 Mi 像素封面预算纳入既有 policy JSON→Rust/Swift 生成链。Windows 和 `public.jpg` 别名继续识别；metadata 200 帧／4 MiB 扫描、原件 32 MiB／归档 40 MiB 预算没有变化。
- `opaque` 收回测试模块，图片 MIME helper 改为模块私有；有生产调用的预算和图片检查接口保留。
- 三处 identity mutation 改为显式错误，保留原 CAS、摘要计算和事务次序；HTML 属性判定只添加等价括号，没有放宽 CSS／资源拒绝门。
- 图片 helper 测试以 `func rtfdToPortable(` 为函数边界，检查必需函数与生成策略，不再依赖说明注释。新增真实大 PNG 加密保存／重开／原件恢复计划回归，包括纯 PNG 和 PNG＋文字两种集合。

### 3.2 主代理二次补修

1. **发现生成器校验过宽。** 首轮允许最多 1000 帧／32 Mi 像素，虽然当前 JSON 仍为 20／8 Mi，却允许以后无意放大解码门。独立 temp fixture 调用真实生成器；`policy-boundaries-red` 实际 exit 1，证实 21 帧被接受，记录前后源码均为交接指纹。补修为不得超过 20 帧／8 Mi 像素；正式负向用例覆盖 21、8 Mi＋1、原来宽上限、非整数、零／负数、null 和字符串，禁止以 text/application MIME 混入图片表。相同独立探针随后两项均拒绝，exit 0。
2. **补上非循环的兼容断言。** 图片名称／MIME 与两个控制名称除了生成一致性外，还对照冻结的旧清单；原件域测试显式检查旧 14 个例外与 JPEG/GIF/WebP 等原件类型。避免“配置和代码一起改错仍同时通过”的自比较测试。
3. **修正本轮验证工具，而非放宽产品门。** 第一轮最终矩阵 21 PASS／2 FAIL：新增控制名单断言需要 rustfmt 换行；本轮独立 probe 错将 test-support 放在普通 dependencies，被 production feature 门真实拒绝。保留原日志、原 probe 源码和 helper。修正格式，并把 probe 改为 integration test＋dev-dependencies；未排除 artifacts、未添加 skip、未修改 production feature 门禁。修正后的 feature／fmt 定向检查均 exit 0，完整矩阵以新 ID 全部重跑。
4. **纠正子代理的环境结论。** Windows crate 可以在 macOS-host 下编译并执行跨平台测试；首轮主代理实际 127 PASS／2 既有 ignored。这仍不编译 Windows-only 原生分支，也不证明 Windows 剪贴板行为。Core examples 的默认 all-targets Clippy 失败来自缺少 test-support；支持的 workspace/all-targets/all-features 组合实际通过，保留子代理原失败，不以 lib-only 检查冒充全目标门禁。

最终候选源码 `8ea8cb276d20195d6e62e03259ea89045e5b6c7083af8f158da68120f9414ad2`，783 文件。基线到最终增量 16 文件，+327／−117，零新增／删除源文件。`increment-agent.patch` 与 `increment-final-v2.patch` 分开保存，未将 HEAD 到脏工作树的累计变更冒充本轮增量。

## 4. 验收状态

**本轮核实与修复达到 LOCAL_COMPLETE。** 最终 23 条命令 `*-final-v2` 全部 exit 0，每条执行前后源码均为 `8ea8cb27…`；最终验收不采用交接摘要或 v1 混合结果。

| 最终检查 | 实际结果 |
|---|---|
| Protocol all-features | 51 PASS，0 ignored |
| Core 标准 enabled 全库 | 668 PASS，4 既有 ignored；Iroh 用例实际执行 |
| Core integration | 14 PASS，0 ignored |
| Core DB all-features | 150 PASS，2 既有 ignored；包含新大 PNG 和冻结 Manifest digest |
| Runtime all-features | 45 PASS，0 ignored |
| macOS crate enabled | 118 PASS，3 既有 ignored |
| Windows crate on macOS-host | 127 PASS，2 既有 ignored；保留既有 Win32／InvalidData dead-code warning |
| policy generator 负向／同步 | 5 个 Node 测试 PASS；独立 2 个解码预算上界探针 PASS；Rust/Swift `--check` PASS |
| 新 helper／图片函数 | 完整 Swift 编译 PASS，self-check 为 CWH1/protocol=4，57 个图片封面断言 PASS |
| 大 PNG 原件回放 integration probe | 1 PASS：加密保存／重开→Core originals RestorePlan→supervised 新 helper 私有板发布→回读原件字节与原件域 fingerprint 相同 |
| 静态门禁 | workspace/all-targets/all-features Clippy `-D warnings`、三份 fmt、canonical、contracts、production features、capture visibility、diff check 全部 PASS |

以上测试有覆盖重叠，不相加为唯一测试数。helper 封面断言与 CLI self-check 是独立检查，不伪装成 Rust 单测数。没有增加 ignored、allow 或跳过参数；production feature 门仍扫描全部目录，并确认 13 份真实 Cargo.toml 的 test-support 均为 dev-only。

原生证明只涵盖**合成私有 pasteboard 的 Core 计划／supervisor／新 helper 往返**。没有启动完整 App 或重跑 effect journal、实际来源／目标应用接受、Windows-only 原生分支、双设备或 hosted CI；这些继续 **UNKNOWN**。P1-A 的不存在缺陷不等于所有来源采集已验收；P1-B 的合同符合也不等于逐次 OS 复制无遗漏。Release **NO-GO**，O04 未开始。

11 份历史 seal 共 10,204 文件内容摘要再次复算一致；S0 仅使用既已记录的 `core/Cargo.toml`→`core/Cargo.toml.txt` 映射，没有重写旧 seal。最终源码、两轮增量、全部命令与失败日志、子代理原日志、本报告／计划／台账副本由本轮新 `evidence-sha256.json` 单独封存；不据此让历史 CI 或旧包认证本轮源码。

## 5. 二次复查入口与复跑

- 审查判断：本报告第 1 节；P1-A 的输入源码 PASS 见 `commands/baseline-large-png-v2.json`，P1-B 的合同边界见 ADR-005 §12／§13。
- 精确补丁：证据目录 `increment-agent.patch`、`increment-final-v2.patch`；每份同时保存对应 source 文件摘要和变更文件原文。
- 真正生产接缝的保护：DB `o03_large_static_png_is_retained_and_prepared_from_originals`，Protocol 冻结名单／预算断言，Core `original_fingerprint_name_admission_keeps_the_legacy_domain`；旧 Manifest digest 回归仍执行。
- 可执行门禁定义：证据目录 `final-checks.py`，每条经 `run-check.py` 记录真实 argv、exit、原始输出、log SHA256 与执行前后源码指纹；同名记录拒绝覆盖。复跑须使用新的版本参数，不能覆盖 v1／v2。
- 原件探针：证据目录 `probe/tests/retention.rs`，test-support 只由 dev-dependency 启用。使用新编译 helper 的 private synthetic board；不读取 general 剪贴板或真实用户历史，不发设备同步。缺设备／应用条件的门禁仍保留，不通过合成样本关闭。

本次只修确认成立的维护与验证问题；没有改变完成时的采样准入、扩大便携图片限制、进入 O04 资源归档或实施 F01—F06 体验任务。未暂存、提交、创建新分支、reset/clean、替换日常 App 或重封旧证据。

若需撤回本轮，先核对相应 final 文件摘要，再按本轮增量或 baseline 副本逐文件恢复；不得 reset 到 HEAD，否则会丢失本轮之前已有的 O01—O03 等未提交改动。回退后原最终源码门禁不再认证当前代码，须另建证据目录复验。
