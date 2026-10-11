# Composite clipboard R6 verification — 2026-10-05

原始封存状态：**LOCAL_COMPLETE（macOS 本地范围，仅原 hash）**。原 42 个必需命令门禁通过，Windows 原生、完整 App、目标 App、双设备仍 UNKNOWN，发布 **NO-GO**。不继承 R0–R5 的旧指纹。

修复树为 `codex/composite-clipboard`，基线 HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。新证据目录为 `artifacts/composite-repair/2026-10-05-r6-final-evidence/`。没有提交、发布或替换用户正在运行的 App。

## 当前源码与本机验收

当前 run 为 `artifacts/composite-repair/2026-10-05-single-mac-r6-self-contained-final/`，源码 `cd9268e75f38d52431f985ef8dadd8c4c910eee3b09f638d289fa71a007ec1eb`，756 文件。完整 44 项已重新执行：**44/44 PASS、0 unexpected、1 个唯一 Windows host 编译警告**，状态 LOCAL_COMPLETE（macOS 本地范围）。封存及只读 VALID 以该 run JSON 与独立 verify 为准。

新修复：实际投递缓存预算、Mac composite 空闲轮询、独立 debug 包快照。debug daemon/helper 与四种 App 都必须封存在 run 内；验证器拒绝外部路径/软链接依赖；新包探针也执行封存 debug helper。22 项 R6 规则测试与真实 debug 输入缓存删除对照通过。

旧 `c022bcb8…` 矩阵有 44/44 PASS，但 debug 包字节依赖已清理的构建缓存，不能再宣称整体 VALID。225 个封存记录本身未改。`54582e18…` 完成 44/44 的 run 留为未封存诊断，不继承到新源码。

真实 App、消费者、故障、键盘/IME、性能及产品修复见[单 Mac 当前报告](COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md)。原生证据按实际源码/包 hash 分开。R7 完整设备组仍 UNKNOWN 10，发布 NO-GO。

## 2026-10-05 前期验收补强（历史源码）

当前证据入口为 `artifacts/composite-repair/2026-10-05-acceptance-hardened-r6-final/`。
**LOCAL_COMPLETE（macOS 本地范围）：44/44 PASS，0 unexpected，1 个唯一 Windows host 编译警告。**
当前源码聚合 `0d9d381078fbd613f9b5b616ece136d07a67b8a64beffec25730e28d7c5ad80a`，753 文件。
正式矩阵扩为 44 项，包含 R7 harness 与只读 R6 证据有效性回归。旧 `d2937a6b…` 和
`53dcfeb3…` 的 42 项记录保留为历史，不能证明补强后的源码。最终结果以新 run 的
`status.json`、`results.json` 与 `evidence-index.json` 为准。

只读验证入口为 `python3 scripts/composite-evidence.py <run-path> --verify`，同时核验当前源码
及逐文件指纹、当前 runner 的必需矩阵/命令/过滤器/测试数、原始日志、辅助证明、包内实际文件
与封存清单。封存后不可继续执行矩阵或改写封存；需要新验证时创建新 run。
测试预期 ledger 已纳入源码指纹，避免修改 green/red 预期而沿用旧证明。

R7 原“本机 PASS 2”修正为完整设备门禁 UNKNOWN 10、本机支持单列。
报告见 `COMPOSITE-CLIPBOARD-ACCEPTANCE-FOLLOWUP-2026-10-05.md`。

## 原始六项工作与门禁

| 工作 | 实际入口与证据 | 状态 |
|---|---|---|
| 三态完整矩阵 | run-composite-r6.py；runtime_disabled 固定 3 项；前端 build/lint；无 Iroh skip | PASS |
| Iroh 波动纪律 | 回环协议组 + N0/实际接口地址网络组；首次失败不可覆盖，发生后立即同环境 compare-iroh-baseline.py | PASS |
| 新包/helper | 默认/开启 × native/universal 四包，debug 单独；生产 resolver CLI、签名、minos、协议 2、全部文件 hash | PASS（Mac/debug）；Windows portable/installer UNKNOWN |
| canonical includes | 9 组共享源、双端 wrapper/模块注册；23 项正负向；已接入 CI（该源码的 hosted CI 未运行） | PASS |
| 实际日志结果 | 日志/退出码及 compiler-message JSON；唯一诊断去重；12 项解析回归；独立 scratch eq_op lint 负向 | PASS |
| 文档/版本台账 | CONTEXT、根中英 README、两平台 README、长期规格、ADR 与两份执行台账 | PASS；独立 document-fingerprint.json |

## 独立版本

| 契约 | 现行版本 | 决策 |
|---|---|---|
| identity/base | 4 | 身份兼容不变 |
| data frame | 5 | 认证握手允许旧端协商回退 4 |
| main history | 条件 11/12 | feature/runtime/helper 控制 v11→v12；已有 v12 可读，承接 v12 数据不降 schema |
| effect journal | 1 | 独立 SQLite，原生副作用权威不由可删除历史提供 |
| local contract | 1 | 生成物 drift 门禁；restore 稳定错误码保持既有决定 |
| helper protocol | 2 | 现行版本，本轮没有 wire 变更 |
| origin fingerprint | 2 | 系统发布投影的本机来源摘要，独立于 WireDigest |

R6 增加只读 CLI 诊断、测试过滤器与证据工具；没有改变实际线协议。未来改变任一实际协议须另有版本决策。

## 台账映射

| 行 | 独立门槛 | 新证据 | 状态 |
|---|---|---|---|
| H-17 | 生成物/本地契约无 drift，版本决策明确 | contracts command；版本表；ADR-004 | PASS（本地范围；R7 保留 UNKNOWN） |
| I-1 | 双端 canonical 接线及删除一端必失败 | canonical-includes / canonical-negative，23 项 | PASS（本地范围；R7 保留 UNKNOWN） |
| I-2 | 条件 schema 文档与政策一致 | runtime-disabled 3 项、Core policy/迁移/承接回归 | PASS（本地范围；R7 保留 UNKNOWN） |
| I-3 | 长期规格、ADR、K1–K10 可追踪 | features/composite-clipboard.md、ADR-004、R1–R5 followup | PASS（本地范围；R7 保留 UNKNOWN） |
| I-4 | source/package/log/document SHA256；可分享脱敏材料 | fingerprint、package-manifest、evidence-index、shareable-logs | PASS（本地范围；R7 保留 UNKNOWN） |
| I-5 | 实际 diagnostics 去重，Cargo 汇总行不重复 | results.json、evidence-parser 12 项 | PASS（本地范围；R7 保留 UNKNOWN） |
| I-6 | 每门槛 PASS/FAIL/UNKNOWN 独立 | results、fault-matrix、package-manifest、status；R7 UNKNOWN | PASS（本地范围；R7 保留 UNKNOWN） |
| I-8 | cache 不被误记为 lint 未跑 | clippy 五组、lint-negative 的实际 eq_op 诊断 | PASS（本地范围；R7 保留 UNKNOWN） |

## 证据边界

`source-fingerprint.json` 覆盖产品、测试、工具、构建配置、锁文件和未跟踪实现文件，带 HEAD、dirty diff hash、Git 状态及逐文件 hash。Markdown/docs、生成 schema、artifacts 不参与测试源码聚合；文档另用 document-fingerprint.json 封存，包另按完整文件清单聚合。每条命令在 records 中绑定测试源码 hash，原始日志永不覆盖。

`fault-matrix.json` 读取生产接收器计数 adapter 的 native attempts/history/terminal/预算及 pending 释放；实际私有 board supervisor 记录 child kill/reap 和 lease 释放。probe seam 不适用的字段为 null，并说明范围。没有把 simulator 当作 Windows OS 的验证。

`rollback-proof.md/json` 使用新 writer 和旧 f719d58 core 的新构建 reader，读出文本/外置图片/文件/收藏，含 database-only 负向对照、文档恢复脚本及升级后 v12 composite 归档。authority 范围使用合成 stand-ins 验证不回滚；真实 receipt 重启语义由接收器故障回归另证。

私有 board 测试只触碰 `tailsync-probe.*`，使用合成内容；不触碰用户 general clipboard。Mac debug/App 内 helper 是不同包形态，各自 self-check、生产 resolver 与 RTFD 发布/回读。fault helper 在独立目录编译，未覆盖任何产品包。

Core 正常套件保留既有 4 项 ignored：历史性能、发送性能、9×60s worker benchmark、由父测试显式运行的 subprocess worker；Iroh 无 ignored/skip。Swift 中依赖活跃 GUI/WKWebView 的既有 skip 单独保留 UNKNOWN，不归入本地 native GUI 成功。

Windows 原生 MSVC 链接、dev、portable/installer、完整 App、目标 App、双设备、VoiceOver/IME 与旧 macOS 实机仍 UNKNOWN。host check、包静态与私有 board 不替代这些门槛；发布 NO-GO。native-acceptance.md 属 R7，尚未执行不生成虚假记录。

## 本轮实际结果

源码聚合 SHA256：`d2937a6bc04f2b04e85fa512151daf2ab9a41f3b30488c11988a46d76a16835c`，749 个产品/测试/工具/配置文件。42 个必需命令退出与断言全部 PASS；结果由 scripts/composite-evidence.py 从实际日志与 JSON diagnostics 生成。

| 门槛 | 实际结果 |
|---|---|
| Core 默认 / 扩展 / repair | 607 / 610 / 620 passed；每组 14 个 Iroh 用例全部执行，无 skip |
| 运行时关闭 | 3 passed，healthy helper + v11 禁止升级 + v12 复合表示/收藏保留 |
| 真实 Iroh 网络组 | 2 passed，N0 与本机接口地址；本轮没有自然失败，条件基线对照未触发 |
| protocol/runtime、runtime repair、receiver faults | 60 / 10 / 10 passed |
| macOS host 默认 / 开启 | 115 / 116 passed，3 项既有 ignored 各自保留边界 |
| Windows host 默认 / 开启 | 125 / 126 passed，2 项既有 ignored；非 Windows native 证据 |
| Swift / React | 254 passed + 4 GUI/WKWebView skipped；React 268 passed |
| 前端 build/lint、check 两组、Clippy 五组 | PASS；Clippy 均使用 -D warnings |
| canonical includes / parser / negative lint | 9 组双端接线；23 / 12 tests；scratch eq_op 被实际拒绝（driver PASS） |
| repair gate | 30 checks，0 unexpected |
| 回退 | 当前新 writer、旧基线新 reader、外置 payload、收藏、数据库-only 负向、恢复脚本、升级后归档 PASS |
| 新包 | 默认/开启 × native/universal 四 App + debug，签名、最低系统、架构、生产 resolver、helper protocol 2 PASS |
| 私有 board 包探针 | 5 种 Mac/debug helper 的 RTFD 发布/回读；实际故障、启动前拒绝、输出超限 PASS |

故障矩阵共 17 条实际观察，按不同 seam 分别计 native/history/budget/child；null 均有范围解释。包 SHA256 与每个文件 SHA256 在 package-manifest.json。

唯一 Cargo warning 为 Windows 源码在 Mac host 编译时的 `ClipboardFileReadError::{Win32,InvalidData}` dead-code 诊断。默认/开启两次出现，程序合并为 **1**；Cargo generated-warning 汇总行不计数。属于 host 条件编译边界，没有修改 native Windows 错误语义或隐藏诊断。

本轮开启态平台测试首先失败于旧 fixture 未设置 helper 健康，日志保存在 `artifacts/composite-repair/2026-10-05-r6-final/logs/macos-enabled.1.log`（Windows 同根因日志也保留）。随后 fixture 明确安装健康 recording provider 并用 Drop 恢复原状态，未放宽生产 handshake；目标用例与最终两平台开启态全量转绿。关闭态 fixture 也明确设 healthy helper，避免用 helper 未知状态证明运行时禁用的空洞通过。

前期不完整 run 只用于诊断，标 INCOMPLETE，不能继承通过证明。最终新 run 内源码聚合 hash 在全部命令前后相同。未启动完整 App、未修改用户 general clipboard、未提交/推送/发布。

CI 配置已加入这些入口；本轮未提交/推送，hosted CI 状态为 UNKNOWN，不能用本地 PASS 代替 CI 通过。

当前 cd9268 矩阵真实测试数：Core 默认/开启/repair 610/613/623；Mac 116/117；Windows host 125/126；Swift 261；React 273；R6 规则 22；R7 harness 28。正式 debug 探针运行 run 内的 clipboard-helper。
