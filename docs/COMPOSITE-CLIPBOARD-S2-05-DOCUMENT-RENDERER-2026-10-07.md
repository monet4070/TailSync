# S2-05：两端受控文稿预览 — 实施、独立审查与二次修复

日期：2026-10-07。本轮只做 S2-05；S2-06 QQ、S2-07 打包与发布未启动。

## 1. 判定与执行流程

**判定：LOCAL_COMPLETE（本机代码与选定自动门禁）；完整 A05/A06：UNKNOWN。Release：NO-GO。**

复用一个内置 `cline-pass/deepseek-v4.1-flash` / high 子代理 Volta，ID `01a114ec-3654-7131-88cc-e4d3e5420b94`。完整明文任务、唯一绝对工作树、`fork_context: false`；核验 CPA 兼容配置和 ACK 后实施。已等待完整 HANDOFF 并关闭子代理；随后由父代理冻结交付、独立执行生产接缝负向探针、二修和最终门禁，没有并发修改产品。

完整 A05/A06 的真实 GUI/AX、网络/剪贴板 revision/同步事件观察仍为 **UNKNOWN**。代码路径与单元测试不能代替这些观察。

## 2. 工作树、源码与增量

唯一工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`。分支 `codex/composite-clipboard`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。不是用户环境中另一份同名 checkout。

| 阶段 | 产品指纹 | 文件数 |
|---|---|---:|
| 本批输入 | `2a59599df107f4128126f434f24bafdf8832b7cae7a8958258feb2f6f9b0a2b4` | 820 |
| 子代理交付 | `ed5e23a5bc079a9f5b4a68e55fb096b1256d7ad59d9a403b1bbb6f52e3d8d226` | 831 |
| 父最终 v6 冻结 | `b011a5126494e9c3ecdc8799b431a45a2c93cdabea3e355b6483980a9396a7f3` | 833 |

| 增量 | 新增 | 修改 | 删除 | 合计 |
|---|---:|---:|---:|---:|
| before → agent | 11 | 15 | 0 | 26 |
| agent → final | 2 | 21 | 0 | 23 |
| before → final | 13 | 15 | 0 | 28 |

沿用 `scripts/composite-evidence.py`，产品指纹包括未跟踪源码/测试/工具/配置，文档、诊断 artifacts 单独封存。六份最终候选快照和失败尝试均保留；只有 v6 完整矩阵可作为最终验收。独立 crate、runner、合成样本及旧 seal 审计另绑定 `review/auxiliary-final-source-v6.json`。

## 3. 生产接入与支持范围

- **macOS：**历史/收藏原有 Space/预览入口 → 独立 `HistoryPreviewWindowController` → 带真实 owner 的 `HistoryPreviewViewModel`。只有存储派生 `displayKind == document` 走文稿；能力缺失走既有 legacy，不按文件名/格式数量冒称文稿。正式 getter/身份解码 → 有界后台富文本构建 → 后端 stamp/DB/owner 安装复验 → 本地代与取消复验 → 只读 `NSTextView` 安装。
- **Windows：**原生预览 snapshot 在短 DB 锁内取得派生 document eligibility → `Preview` 只选择文稿或 legacy 一路 → 带 owner/generation/nonce 的正式 getter → typed decoder/资产摘要 → 私有 Blob URL 准备 → 再次后端安装复验 → 当前 target 门控的 React renderer。
- **受控模型：**中文/emoji、基本 run 粗体/斜体/颜色/上下标、段落/列表、两图顺序、基本表格与空单元格；缺失首图保持首位置占位，不用第二图顶替。unsupported/partial、预算和图片解码失败显示局部反馈。
- **安全边界：**React 使用字面 children、有限 RGB 样式和经检查的私有 Blob URL；Mac 直接构造 attributed string、TextKit 表格和 ImageIO PNG 附件，不引入 HTML/RTF importer、WebView、任意 URL 读取或明文临时载荷。
- **恢复：**仍由用户显式调用原有整条历史恢复命令，不把文稿视图重新编码成原件。Windows 文稿展示失败时也保留原样恢复按钮，目标来自可信预览 snapshot，不采用坏帧内的 entry ID。IME Return 不恢复；Ctrl+Return 保持窗口，Return 按 favorites/history owner 关闭原窗口，均经 mock 集成验证。
- **交互边界：**Mac selectable/read-only/findBar/AX 属性声明已测；Windows 语义 DOM、键盘/IME 事件和 owner routing 已测。双击文字选择、真实焦点、滚动/查找体验、VoiceOver 和 Windows 原生读屏仍 UNKNOWN。

RTF/RTFD 的现有 RTA1 只有全局样式标志和表格摘要，**不能据此恢复逐 run 样式或完整表格结构**。这部分继续明确 partial；本轮不扩 helper 协议、不新增 RTA2、不伪造全局加粗来替代位置证据。

## 4. 父独立审查：属实问题与二修

前三组为不削弱实现的真实生产接缝 RED；新测试加入后执行原交付实现，再修复并重跑。不是仅凭子代理陈述认定问题。

| 属实问题 | 独立证据与修复 |
|---|---|
| Windows 安装完成后关窗只 abort，不再 cancel backend request | `parent-real-red-react` 关窗探针 FAIL；hook 全生命周期显式、幂等取消，客户端取消 IPC 合并为一个 running + 每 owner 最新 pending |
| owner/条目切换会在新 committed render 暴露旧 ready；新请求失败还保留旧 URL | 同组 layout-effect 观察与失败切换探针 FAIL；同步 target 门控，切换/关闭立即释放 installed ledger |
| Windows 已取消 getter 未退出仍可启动多个 owned getter/preparer | 同组真实延迟 getter 探针 FAIL；单个真实 owned job，等待位只存 metadata、每 owner 至多一个，取消移除等待位；真实 finally 后才交接准入 |
| 64 MiB 只计 frame，漏掉 parsed model、Blob 拷贝和 bitmap placement | 同组 tiny-quota 探针 FAIL；统一 frame/model/copy reserve/每 placement RGBA 计账，先决策再创建所需 URL；超过额度显示占位，不抬预算 |
| 已取消的无资产帧仍准备成功 | 同组空资产 abort 探针 FAIL；准备始末、每个 hash await 后均检查取消/截止时间 |
| 一张图 decode failure 状态会污染下一份文稿同位置图片 | 同组 replacement 探针 FAIL；图片组件 identity 加入私有 URL，错误边界绑定 request nonce |
| Mac 串行 cancel Task 链实际上无界 | `parent-real-red-swift` 阻塞首 cancel 后共执行 36 次，FAIL；改进程级合并取消器，只有一个 running、每 owner 最新 pending，旧代不覆盖新代 |
| Mac detached materializer 没有真实 exit 准入 | 同组阻塞材料构建实际启动 8 个 worker，FAIL；进程级单 worker 准入，取消转发 detached Task，permit 在 worker defer 真退出时释放 |
| Mac 旧请求迟到 nil 能清掉新 active request 并启动旧 legacy | 同组迟到能力缺失探针两项 FAIL；先做取消/代检查再处理 nil，只清理匹配 request，旧代不落入 legacy |
| Windows 文稿渲染失败丢失显式原样恢复按钮 | `parent-real-red-original-restore`：4 PASS/1 FAIL；仅文稿失败且 snapshot/target 一致时开放显式原恢复，坏帧不参与恢复目标选择 |
| Windows 从 legacy 切文稿时，旧载荷仍进入新窗口标题/恢复 footer | `parent-real-red-cross-mode`：5 PASS/1 FAIL，layout-effect 捕获旧 `legacy.txt`；文稿模式同步屏蔽 legacy payload，不把清理寄希望于下一次 passive effect |
| Mac ordered→bullet→ordered 沿用旧编号，与 Windows 分组不一致 | `parent-real-red-mixed-list`：0 PASS/1 FAIL，第三组为 2 而非 1；bullet 分组重置 ordered index，真实 attributed 文本回归通过 |

`parent-real-red-react` 为 **0 PASS/6 FAIL**；`parent-real-red-swift` 为 **0 PASS/3 FAIL**。以上十二项失败均已聚焦重跑 GREEN，并纳入完整 v6 React/Swift 门禁。路径、摘要与计数保存在同一 run，失败日志没有覆盖。

其他由源码审查确认并补 GREEN 的问题：

- Mac 最初 retainedBytes 仅计 encoded assets，预算标志还可能被后续小图覆盖；补模型/中间 reserve/每 placement RGBA 额度，预算提示保持累计、原因按当前图片计算；tiny-quota 与大图后接小图/外源占位回归通过。
- Mac 图片从 lazy `NSImage(data:)` 改成受限 ImageIO PNG：解码前核验类型、单帧和系统读取的尺寸，解码后核验 complete/实际尺寸；强制像素解码后再装附件。真实红/蓝像素顺序、中文样式与空表格列均实测断言。
- 子代理新增了违反本轮约束的 doc/inline comments 和 `const t`。初始 `parent-agent-audit` FAIL；只删除本批新增注释，保留既有注释，修正新 renderer 的单字母绑定；完整最终规范审计不得新增 skip/allow、改 dependency/schema 或删除原源码。
- 子代理 Windows PNG 成功样本只是 32-byte header stand-in，不能证明完整 PNG。测试构造器改成真实 IHDR/IDAT/IEND、CRC32 与 zlib；父独立合成 saved frames 也保留完整可解码 PNG。jsdom 不因此变成 Windows 原生像素验收。

StrictMode 是审查假设，**未复现为原始缺陷**：名为 `parent-real-red-strictmode` 的实际结果是 7 PASS/0 FAIL，不把名字当作 RED 证据。保留该兼容探针，并把 Windows 准入完善为有界最新请求等待，避免靠无界 Promise/Task 队列换取切换成功。

## 5. 预算、取消与真实性边界

单份前端准入仍为 **64 MiB**，累计 placement 最多 **16,000,000 pixels**。Windows 计全 backing frame、8×模型字节 reserve、资产 copy/Blob reserve、4 MiB 固定中间 reserve以及每个实际 placement 的 RGBA；Mac 计资产/拷贝、8×模型 reserve、4 MiB 中间 reserve及每 placement RGBA。降级计划和 renderer 共享同一额度，重复引用不能无限复制 bitmap。

这些是**保守准入核算，不是对 JS/AppKit/浏览器全部实现开销的 RSS 数学证明**。真实 RSS、codec 内部临时缓冲、峰值与跨进程总成本仍 UNKNOWN。没有放宽原后端字节、像素或 pipeline lease 限额。

Windows 有一个 owned getter/preparer/install-check job；至多两个 metadata-only 等待位，旧位取消即移除，最新等待最多 2 秒，超时明确可重试；旧 job 仍占用准入直到实际退出。Mac materializer 忙时明确可重试失败，不提前释放正在运行的 worker。两端 cancel dispatch 都为一个 running + 两个 owner 的最新 pending，不是无限串行 Task 链。

构建/准备有 2 秒 cooperative checkpoints，检查 run/cell/block/asset await 边界；**不能强制抢占正在阻塞的系统 codec 调用**。取消不能被描述成线程已经退出，等待超时也不能被描述成内存已经回收。

后端 Tauri lease 的终点仍是 producer IPC body handoff，不是 frontend buffer disposal；前端新增准入不自动改变这个既有 partial 边界。

## 6. 最终验证与失败尝试

最终 v6 **25/25 门禁通过**，每份 command record 的 exit code 为 0，前后源码均绑定上表 v6 指纹；中断后现场复算仍一致，无残留后台测试进程。结果为 `review/final-gates-summary-v6.json`。

| 实际执行 | PASS | 既有 skipped/ignored |
|---|---:|---:|
| Core default / all-features | 741 / 775 | 6 / 7 |
| Runtime all-features / 显式 composite | 96 / 96 | 0 / 0 |
| Protocol / helper / 独立 crate | 63 / 3 / 16 | 0 / 0 / 0 |
| Mac Rust / Swift / React | 119 / 307 / 366 | 3 / 4 / 0 |
| Windows-host default + composite lib | 257（128 + 129） | 4（2 + 2） |
| 保存字节 native headless / generated semantic | 5 / 6 语义断言 | 不冒充测试框架计数 |

TypeScript、workspace/Mac Clippy、三份 fmt、contracts/canonical/features/resolution/policy/capture、旧 seal 与本批规范审计、diff 均通过。计数来自各 raw suite 汇总，存在 feature 配置重复执行，不能相加声称独立测试数量。Windows-host 是 macOS 宿主真实 lib 测试，不是 Windows-native；既有 dead-code warnings 保留且没有 suppress。

重要证据区分：

- 子代理的 validate-order RED 是临时削弱最终 guard 的 fault injection 后还原，不是原实现漏洞复现；`s205-green-win-tsc` 实际 exit 2，不能因名字 green 就算 PASS。子代理 final3 不继承为父最终验收。
- 父第一轮 React GREEN 为 26 PASS/2 FAIL，原因是新增生命周期 cancel 后页面 mock 缺少对应 API；补 mock 后 28 PASS。原日志保留。
- `parent-pre-final-tsc` 的 owner 字面量类型拓宽导致 exit 2；修成 literal 后复验。首个候选矩阵中止，core-enabled 缺完整 command record/exit，保持 UNKNOWN，不从截断日志认定通过。
- v2 runner 路径替换意外生成 `source-parent-final-v2-v2.json`，在派发任意门禁前 exit 1；元数据另存。v3 使用不重叠替换、新记录 ID、新源码/auxiliary 快照，不覆盖 v1/v2。
- v3 为 24/25 PASS；独立 crate 因诊断 Cargo.lock 被工具输出截断标记污染而 exit 101，实际 0 test。保留坏副本及日志；另建 `probe-v4`，完整读取旧 seal 中锁字节、核验 449 个 package 的 TOML/根 package identity，并以 locked/offline 真执行 16 PASS。产品源码未变；v4 仍重新完整冻结、跑全部门禁，不把旧 0-test 当通过。
- v4 的 native identifier 未更新，复用 `final` 时 exclusive JSON 写入失败，随后 runner 读取不存在的 v4 result 而停止；只完成此前六项门禁，不作完整 PASS。旧 JSON/日志未覆盖，重新编译的旧 helper 与 v3 摘要完全一致。v5 改成独占 `final-v5` 路径，helper/result 在编译前都拒绝覆盖，先完成五项 saved-bytes 语义预验，再冻结重跑全矩阵。
- v5 实际 25/25 PASS，但末轮追加的跨模式/混合列表探针又复现两项缺陷，说明既有测试通过不等于场景覆盖完整。追加两行产品修复和对应回归后重新冻结 v6 全矩阵，不把 v5 通过外推到修改后的最终源码。
- 最终检查必须全部 exit 0、源码前后不变、测试数非零、语义断言实际 PASS；0-test doc-test 子段不能冒充实际执行数。

## 7. 保全、回退与停止点

证据 run：`artifacts/clipboard-improvement/2026-10-07-s2-05-document-renderer-r1/`。输入、子代理、六个父候选完整快照，before/agent/final 精确 delta，所有失败、raw logs、source/env/argv、旧 seal 复验和 SHA inventory 均保留。最终验收通过 `seal-v6.py.txt` 独占生成并经 `verify-seal-v6.py.txt` 只读复算；此前 seal、已绑定主计划和旧报告不改写。

没有暂存/提交/新建分支、reset/clean；没有读用户 DB、QQ、来源缓存或 general clipboard；没有替换、启动或停止日常 App；没有删构建缓存、旧证据或测试包。Rust 串行复用唯一 `target/o04-shared`，debug/incremental 关闭，jobs=2。

回退只能按本批 before→final 增量定向恢复；不得按 HEAD 整体 reset，以免损失此前阶段和用户 dirty 工作。原件/hash/dedupe/schema/migration/capture/network/auth/后端整组恢复语义未改；回退 UI 会失去受控文稿展示及失败时的 UI 恢复入口完善，但不修改已保存原件。

**下一阶段是 S2-06 QQ/系统提示真实闭环；本轮不自动启动。真实 GUI/AX、QQ 直接粘贴对照、Windows-native、双设备、hosted CI、真实性能/RSS均 UNKNOWN，Release NO-GO。**
