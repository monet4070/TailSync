# R4：真实目标应用与隔离环境验收执行方案

日期：2026-10-09。状态：**PREPARED（样本工具、静态样本、候选包及父代理本地验证完成；未启动 App、未执行实机采集）；全部实机/Windows 原生/双设备门禁 UNKNOWN；Release NO-GO**。两名 DeepSeek/high 已完整交付并关闭，父代理独立审查、二修及 22 项当前源码本地门禁完成；准备完成不等于 R4 实机验收完成。

- 工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。默认 cwd 是另一 checkout，不得混用。
- 依据：`docs/RICH-TEXT-PRODUCT-SCOPE-PROPOSAL-2026-10-07.md` 第 8 节「第四轮」；`docs/RICH-TEXT-R3-ACCEPTANCE-2026-10-09.md`（R3 LOCAL_COMPLETE，源码指纹 `b1435a68d4663afe1dd2a6609d72601b3d79026916f8762430087b5457300595`）。
- R4 起点基线：`artifacts/rich-text-repair/2026-10-09-r4-acceptance/baseline.json`；封存锚点 `artifacts/rich-text-repair/2026-10-09-r4-acceptance/r3-seal-anchor.json`。
- 配套可填写清单：`docs/acceptance/rich-text-r4-manual-checklist-2026-10-09.md`。
- 两名代理未执行构建或测试；父代理已编译新的候选、作静态检查并运行隔离测试。本轮修改样本工具、回归、文档与 CI 接线，未修改 Rust/Swift/Windows 产品实现、generated、旧 artifacts、App、既有包或用户数据，未运行 App、联网或 stage/commit/reset/clean。

## 1. 结论词与证据类别（强制）

结论只能取四种：`PREPARED` / `UNKNOWN` / `FAIL` / `PASS`。

| 结论 | 含义 |
| --- | --- |
| `PREPARED` | 准备完成、证据槽位就绪，但尚未产生实机观察 |
| `UNKNOWN` | 未测或缺少可核验证据；任何未填格子的默认值 |
| `FAIL` | 观察到失败，且已保留原始证据 |
| `PASS` | 有绑定源码/包指纹、应用版本与证据文件的真实观察 |

证据类别必须**分开登记、不得互相替代**：`本地自动测试`、`package`、`CI`、`device`（真实 macOS 系统账户或 VM 上的 GUI/系统剪贴板/目标应用）、`双设备`。本地测试、fixture、CI 绿、宿主编译都**不能**写成 `device` 或 `双设备` 的 `PASS`。每条记录至少带：来源文件、样本 hash、源码与包 hash、应用版本、截图或导出证据。缺证据不得填 `PASS`。

R3 的 20 项本地门禁只绑定 R3 源码指纹，不是 R4 实机证据。R4 已对当前源码重新执行 22 项本地门禁；它们也不构成实机证据，R4 实机矩阵不继承任何更早的 `PASS`。

## 2. 隔离与授权边界（父代理隔离审查结论）

R4 的实机验证必须在**独立环境**里进行，不能靠几个环境变量就自称隔离。已在当前源码核实：

- `macos/swift-ui/Sources/TailSync/Services/SingleInstanceLock.swift:53` 的单实例锁默认固定在当前用户 `~/Library/Application Support/com.tailsync.app/instance.lock`，没有环境变量覆盖入口。
- `macos/swift-ui/Sources/TailSync/TailSyncApp.swift:850` 的 `daemonLogHandle` 固定写当前用户 `~/Library/Logs/TailSync/tailsyncd.log`。
- `shared/rust-core/src/crypto/keystore.rs:21` 的生产钥匙串服务为当前用户 `com.tailsync.app.dek-v2`（`keystore.rs:420`/`:504` 读写）。

因此：

1. 只设置 `TAILSYNC_DATA_DIR` / `TAILSYNC_STORAGE_DIR` / `TAILSYNC_API_SOCKET`，或只改 bundle ID，**都不构成完整隔离**：单实例锁、守护进程日志与生产钥匙串仍落在当前用户。
2. **不得**给出在当前用户下双击启动测试包的指令；**不得**修改产品代码或绕过加密来“造”隔离。
3. 真实 GUI/系统剪贴板验收必须在**独立的 macOS 系统测试账户或 VM** 中进行，由人类提供并明确授权，并**先记录环境**（系统版本、账户类型、是否全新、网络状态）再采集。
4. 父代理本轮最多只**编译一个静态签名候选，不运行**。候选包路径与启动命令由父代理核验；未核实处一律标「待父代理补充」，不凭空写可执行命令。
5. `macos/scripts/verify_macos_bundle.sh` 会读写 general 剪贴板并占用 TCP `19890`（`verify_macos_bundle.sh:6`、`:134`、`:170`），**禁止在本机执行**。
6. 最终包只能称为「**待真实环境验收的候选**」，不得称「已验证隔离 App」。

授权要求：真实 App 启动与系统剪贴板写入都需要**人类明确授权**。禁止读取已有系统剪贴板或真实历史，禁止停止日常 App，禁止覆盖旧包/旧 artifacts/用户 DB。未经授权，相应门禁保持 `UNKNOWN`。

## 3. 前置准备：隔离候选包与样本

### 3.1 待隔离环境验收的候选包

- 实机开始前，在获授权的独立账户或 VM 中放置已构建候选，核对候选主二进制、包内 `clipboard-helper` 与构建源码指纹（SHA256）。当前包仅在本机编译与静态检查，未在任何账户启动。
- 已构建候选：`artifacts/rich-text-repair/2026-10-09-r4-acceptance/candidate/TailSync-R4-20261009.app`。arm64、debug、ad-hoc 签名，macOS 13.0 部署守卫及静态签名检查通过。不是 release 包、不是已验证隔离 App；未启动、未替换日常 App。
- 包清单：`artifacts/rich-text-repair/2026-10-09-r4-acceptance/parent-candidate-package-01.json`。`candidate_inventory_sha256` 为排序后的包内相对路径与每文件 SHA256 映射的 JSON 摘要，当前值 `b1f98889e0fbd94c0627baf1ecda7b831088930095b5b8f3e4d08cf39848de0e`；不是单个二进制的 hash。
- 分别记录 `ui_sha256`、`daemon_sha256`、`helper_sha256`、`candidate_inventory_sha256`、`production_source_sha256`、`os_version` 和账户/VM 标识。构建时核对 R3 清单全部 856 个源文件未变，这是构建时点的事实。随后 R4 新增工具并修改 CI，当前 858 文件指纹为 `b5dc8000de811b55c944da9e69bbbb2d8bcdd233bf867ba854abf88ac1cfcf50`；旧 R3 封存不能认证这些新改动。候选构建来源与当前全仓本地验证指纹分开登记。
- 当前用户启动禁止。仅在人类提供并授权的独立系统测试账户/VM 内放置候选后，可启动该环境中的候选并取证；尚未获得该环境，因此没有可在当前会话执行的启动命令。VM 须关闭宿主共享剪贴板、共享 TailSync 数据目录和个人钥匙串接入。

### 3.2 fixture 工具与已生成样本

工具路径 `scripts/rich-text-r4-fixtures.py`；回归路径 `scripts/tests/test_rich_text_r4_fixtures.py`。父代理已独立二修，29 项回归通过，并实际生成 `artifacts/rich-text-repair/2026-10-09-r4-acceptance/samples-prepared-01/`，共八个文件。

- 仅生成静态文件，确定性输出；不联网、不启动 App、不写系统剪贴板、不读用户 DB/剪贴板。
- 输出（到全新 `--output` 绝对目录）：
  - `reading.html`：正文与 PNG/JPEG/静态 WebP 三张图片的顺序对照。
  - `styles.html`：三级标题、引用、行内代码、保留空白的代码块、有界列表、简单表格、基础强调、颜色/高亮；包含语义标签与等效常见内联 CSS、安全 HTTP(S) 链接及 mailto 文字对照。
  - `partial.html`：含一张缺图/未闭合外部图片依赖的样本，用于缺图可用内容场景。
  - `editor.rtf`：自造 RTF 输入，覆盖基础强调；不是实际编辑器导出的剪贴板证据。须在选定编辑器打开后用真实复制动作采集，再与直接粘贴比较。
  - `images/transparent.png`、`images/color.jpg`、`images/static.webp`：原始 PNG/JPEG/静态 WebP 字节，与入库来源一致。`images/absent-color.jpg` 故意不存在，不能补出第四张图片。
  - `manifest.json`：其余七个文件的相对路径、size、SHA256 与样本角色。manifest 自身摘要由父代理另存，不自引用。
- fixture 通过只属 `本地自动测试`，**不能**替代真实目标应用或真实公开下载证据。

以下命令**只生成文件**。示例输出目录必须尚不存在，不能用它覆盖已封存的 `samples-prepared-01`；需要重生成时换一个全新绝对路径。

```sh
cd /Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35
node scripts/run-isolated-tests.mjs -- env PYTHONDONTWRITEBYTECODE=1 python3 scripts/rich-text-r4-fixtures.py \
  --output /private/tmp/tailsync-r4-manual-samples-20261009-01
```

默认地址仅为 `https://example.com/tailsync-rich-text-r4/` 文档占位，未部署、不可验收。指定真实 `--public-base` 也只做语法检查，不验证 DNS/HTTP、不发布：`acceptance_eligible=false`、`network_verified=false`、`publication_status=UNKNOWN`。公共页面必须由操作者实际托管并另行取证，不能把工具接受一个地址当作资源已公开可用。

### 3.3 公共样本

- 三编码有序图必须托管在**真实已发布**的公开页面；`example.com`、`loopback`/本机地址**不能**充当公开下载成功。
- 样本地址：**待父代理补充**（尚未发布，故自动下载门禁当前 `UNKNOWN`）。

## 4. 固定目标应用与版本

按范围方案第 8 节：先选**一款常用浏览器 + 两款实际编辑应用**，目标应用与版本在整轮对照中保持一致。

| 角色 | 应用 | 版本 | 状态 |
| --- | --- | --- | --- |
| 来源浏览器 | 待指定 | 待指定 | UNKNOWN（未指定） |
| 编辑应用 A | 待指定 | 待指定 | UNKNOWN（未指定） |
| 编辑应用 B | 待指定 | 待指定 | UNKNOWN（未指定） |

同一轮里所有直接/历史对照、所有条件都必须使用同一 `target_app`/`target_version`；不一致判为无效观察。目标应用未指定前，相关门禁全部 `UNKNOWN`。

## 5. 成对对照方法（同目标、同版本，先直接后历史）

对每个样本、每个目标应用：

1. 记录来源动作（复制动作、时间），保存来源直接粘贴到目标的基线（文字、格式、图片数量与顺序）。
2. 通过 TailSync 历史恢复**同一**条目到**同一**目标，比较文字、格式、图片数量与顺序。
3. 先直接、后历史，成对记录；两侧 `target_app`/`target_version` 必须一致。
4. 记录该样本对应的 history entry / capture / revision 与原件摘要。

编辑器场景不能只复制网页：分别在编辑应用 A/B 打开自造 RTF，选取内容并执行真实复制，再在固定目标应用进行直接/历史对照。记录来源编辑器及版本；fixture 的 RTF bytes 不是编辑器复制时实际提供格式的替代证据。

判定规则（范围方案第 8 节）：

- **直接粘贴支持、历史恢复丢失** → 列为**修复缺陷**。
- **直接粘贴本身就不支持** → 记为**来源/目标组合边界**，不伪造 TailSync 完整效果。
- 若来源只有文字，如实记录该来源/动作限制，不用 TailSync 冒充完整图文。

## 6. 场景矩阵（最少分）

每格结论只能 `PREPARED`/`UNKNOWN`/`FAIL`/`PASS`，并附证据编号。

### S1 默认自动下载关闭的零自动图片请求

- 默认状态（`auto_save_network_images` 关闭）下复制含网络图的新条目：TailSync 不应发起自动图片下载请求；不能要求整个浏览器或 TailSync 的发现/同步通道零网络流量。
- 记录：开关初始值、可归因于 TailSync 自动图片任务的请求数量、是否有误导性的“下载失败”式提示。浏览器加载来源页面的请求与其他发现/同步流量分别记账。
- 状态：`UNKNOWN`（需独立环境 + 授权观察）。

### S2 打开后新本机复制自动下载三编码有序图

- 打开“自动保存网络图片”后，对**新**本机复制（公开页三编码有序图）自动下载并保存。
- 记录：三张图（PNG/JPEG/静态 WebP）是否保存、顺序是否正确、是否为已发布公开页。
- 记录确实发生了符合条件的新本机复制；旧条目不因开关开启而自动补抓。若去重或来源动作没有形成新记录，先核对事件/条目身份，不把无下载直接判为缺陷。
- 状态：`UNKNOWN`（公共样本地址未发布）。

### S3 关闭取消且不影响手动

- 关闭开关：取消未完成的自动下载，已保存资源保留，**不**影响显式手动保存。
- 记录：取消行为、已保存资源是否保留、手动保存是否仍可用。
- 取消测试必须先用受控公共服务器延迟响应或限速，并确认有未完成自动任务。三张小图已全部保存后再关闭不能证明取消；没有可观察的在途任务时本格保持 UNKNOWN。
- 状态：`UNKNOWN`。

### S4 缺图可用内容与提示

- 缺一张图时：其余正文/格式/已保存图片仍可阅读与恢复；缺图位置有说明，顶部最多一条汇总（如「1 张图片未保存」）。
- 记录：可用内容是否可复制、提示文案、原历史是否被改写。
- 状态：`UNKNOWN`。

### S5 来源关闭 / 测试 App 重启 / 离线预览恢复

- 已保存的同一条目在**来源关闭**、**测试 App 重启**、**断网**三种条件下分别仍能预览与恢复。
- 三种条件**分别记录**，不得互相推断；来源关闭/重启/断网动作须在测试会话内安全进行。
- 状态：`UNKNOWN`。

### S6 链接仅点击打开

- 预览不自动加载网页；只有用户点击才交给系统打开；不安全链接（含凭据、控制字符、无唯一安全 base 的相对链接）不被继承或自动打开。
- 状态：`UNKNOWN`。

### S7 安全与深浅主题 / 键盘辅助功能

- 区分两个策略：预览链接拒绝凭据、控制字符、危险协议等不安全写法；网络图片归档另行拒绝私网/非公开资源。不能把下载的 SSRF 限制误当成所有用户点击 HTTP(S) 链接的限制。
- 主题：深/浅主题下正文、链接、引用、代码与高亮的对比度可读。
- 键盘辅助功能：键盘可达、焦点顺序合理、不出现未命名控件。
- 状态：`UNKNOWN`。VoiceOver 可另作可选记录，不替代键盘可达与可访问名称核查。

## 7. 独立门禁（Windows 原生、真两设备）

- **Windows 原生**：真实 Windows 上的采集/恢复；macOS 宿主编译**不**等于 Windows 原生证据。状态 `UNKNOWN`。
- **真两设备**：两台真实设备之间传输、断线/离线/重启恢复；本机 loopback/模拟**不**等于双设备证据。状态 `UNKNOWN`。
- 二者需独立设备与授权，本机不可达，保持 `UNKNOWN`，不得用本机结果推广。

## 8. 记录字段与证据登记

每条观察记录至少包含：

| 字段 | 说明 |
| --- | --- |
| `scenario` | 场景编号（S1–S7 或独立门禁） |
| `evidence_class` | 本地自动测试 / package / CI / device / 双设备 |
| `source_file` | 来源文件或样本相对路径 |
| `sample_sha256` | 样本 hash |
| `source_sha256` / `package_sha256` | 源码与包 hash |
| `app_version` | 应用版本 |
| `target_app` / `target_version` | 目标应用与版本 |
| `condition` | immediate / source_closed / offline / restarted（如适用） |
| `evidence_ref` | 截图或导出证据路径（+ size + SHA256） |
| `result` | PREPARED / UNKNOWN / FAIL / PASS |

任何声称已观察的格子都必须有证据；没有证据的已观察判为无效。

## 9. 禁止事项

- 在当前用户环境不运行 App、不开浏览器、不联网；不安装依赖。父代理仅允许静态构建/检查和隔离工具测试，真实 GUI/剪贴板观察仍须满足独立环境与授权前置条件。
- 不 stage/commit/reset/clean；不修改产品源码、generated、旧 artifacts/App/包/用户 DB/general 剪贴板。
- 不使用第三代理或后台 CLI；不停止日常 App；不覆盖旧包。
- 不凭空写候选包路径/启动命令；未核实处标「待父代理补充」。
- 不用 `example.com`/`loopback` 冒充公开下载成功；不用 fixture 冒充真实测试。

## 10. 未核实项（待父代理补充）汇总

| 项 | 状态 |
| --- | --- |
| 候选包 | 已构建并静态检查；见 3.1，尚未运行 |
| 静态样本与生成工具 | 已生成八文件、29 项本地回归通过；未发布、不构成实机证据 |
| 可启动的独立验收环境 | 未提供；当前用户禁止启动 |
| 公共样本（已发布公开页）地址 | 待父代理补充（未发布） |
| 浏览器与两款编辑应用及版本 | 待指定 |
| 独立 macOS 系统测试账户 / VM | 待人类提供与授权 |
| Windows 原生设备 | 未提供 |
| 两台真实设备 | 未提供 |

## 11. 与 R3 / Release 的关系

R4 是范围方案第 8 节「第四轮」：由真实目标应用决定后续投入。R3 已 `LOCAL_COMPLETE`，但真实 GUI/目标应用/设备仍未验收。**整个富文本方案保持 active，Release 保持 NO-GO**；只有拿到绑定当前源码/包指纹的真实设备证据，才可把对应门禁改为 `PASS`。

## 12. 父代理独立审查与准备证据

- 首交付 25 项测试有真实失败：多播 IPv4 被 `is_global` 误放行。父代理增加回归后，29 项出现 16 failures 与 1 error；修复多播/保留/映射 IP、畸形 URL、外侧空白、C1 控制字符、反斜线、编码点路径、歧义 IPv4、空 query/fragment 与 IPv6 方括号保留问题。
- 不把部署地址参数等同于已部署；补齐安全链接、mailto 非 HTTP 对照、简单表格、颜色/低对比高亮样本，并修复 RTF 非 BMP 字符的 signed UTF-16 surrogate 转义。
- 当前源码完整本地矩阵 22 项通过：Core 914 PASS / 4 ignored、integration 14、Runtime 121、Protocol 66、Swift 344 PASS / 4 skipped、Windows UI 410、canonical 27、本地契约 3、fixture 29、CI scope 18；另含编译、构建、Clippy、生成检查、lint、格式与 diff。不把零测试的 check 命令当作执行了测试。
- Windows 全量 lint 的 22 条既有警告、Windows 宿主编译的 4 条既有诊断与 R3 逐项核对；现存 ignored/skipped 不作为实机通过。CI 接线与 scope 本地测试通过，不等于托管 CI 已运行。
- 两名代理只交付初版；父代理执行最终门禁。临时文件以 shell `cat` 编写的编辑方式偏差、文档代理被拒绝的 `rm -f` 及一次只读无匹配退出保留在审计，不把被拒绝命令或缺失数值退出码记为成功。

证据根 `artifacts/rich-text-repair/2026-10-09-r4-acceptance/`；当前源码清单 `parent-source-final-01.json`、22 项门禁索引 `parent-validation-final-01.json`、样本清单 `parent-samples-final-01.json`、代理审计 `parent-agent-audit-final-01.json`、逐项要求 `parent-requirements-final-01.json`、保护核验 `parent-protection-final-01.json` 与封存 `parent-evidence-seal-final-01.json` 分开保存。`final-delta-final-01/` 只针对本轮基线，不能 reset HEAD 或覆盖前序未提交工作。该封存只认证 **R4 准备**，不认证 R4 实机完成。
