# R3：常用阅读语义本地验收

日期：2026-10-09。状态：**R3 LOCAL_COMPLETE；真实 GUI/目标应用/设备 UNKNOWN；Release NO-GO**。两名 DeepSeek/high 已完整交付并关闭，父代理独立审查、二次修复及最终本地门禁完成。实际工作树 `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。这是当前工作树源码的本地验收，不是已安装 App 或真实设备验收。

## 用户能得到什么

| 内容 | 本轮支持与边界 |
| --- | --- |
| 标题 | 统一为三级；HTML h4–h6 映射到第三级，不保留任意字号排版 |
| 链接 | 仅明确、无凭据的 HTTP(S)；用户点击才交给系统打开；预览不自动加载网页 |
| 引用与代码 | 有界引用层级、行内等宽文字、保留换行与空白的代码块 |
| 列表 | 有序/无序和有界嵌套；返回父层继续编号，退出列表后重置 |
| 颜色与高亮 | 保留可读的显式颜色；深浅主题或高亮导致对比度不足时只调整预览颜色，目标对比度 4.5 |
| 基础格式 | 保留粗体、斜体、下划线、删除线、上下标，以及已有图片位置与简单表格 |
| RTF 预览 | 只在正文逐字对应、位置可证明时应用六种基础强调；不把全篇 style_bits 当作每个字的样式 |

原件保存/恢复与简洁预览仍是不同职责。预览不是网页/CSS/Word 引擎，不执行脚本、外部样式表、自动链接探测或外部请求，不从简化预览反推原件。没有增加模式或格式按钮；未知装饰仍由折叠诊断表达。R2 的网络图片自动保存仍默认关闭，本轮没有扩大其联网范围。

## 契约、预算与原件保护

- DisplayDocument 保持 v1，paragraph/table/image 类型不变；新增阅读字段可缺省，旧文稿按默认值解码。生产能力 schemaVersion 为 2、wireVersion 为 5；不把能力 schemaVersion 与文稿版本混为一谈。
- Swift/TypeScript/schema/fixtures 均由生产 Rust DTO 导出及共享生成器生成，未直接手改生成文件。协议 RTA1 编码、原件、manifest、数据库与恢复身份不因阅读模型新增字段而重定义。
- 单链接最多 2048 UTF-8 字节；文字和链接共同计入既有预算，收集阶段即限制累计 run 数量与字节，而不是分配完再裁剪。标题最多三级，引用/列表深度最多八级，未放宽既有资源预算。
- 在 URL 归一化前拒绝控制字符、空白、反斜线及 authority 中的 userinfo，包括空凭据。相对链接必须有唯一且安全的 HTML base，不猜来源；非法嵌套链接不能继承父链接的目标。
- RTF scanner 输出有界逐位置样式证明，涵盖分组作用域、Unicode 与 surrogate 边界。正文或位置不能证明对应时不推测样式；Runtime 回归确认预览之后原始 bundle 不变。

## 父代理独立审查与二次修复

1. 修复局部 paragraph 变量遮蔽测试 helper 的编译错误；调整安全链接分段测试，普通文字合并不能被误判为链接丢失。
2. 新增真实 RED 回归，修复 PendingRuns 收集阶段的累计链接字节/run 数量限制、非法嵌套 anchor、HTML base 归一化前检查，以及表格单元格首段样式作用域。
3. 重新生成生产契约并修正 Swift 新可缺省字段、schemaVersion 测试与共享链接预算断言。Runtime 重开用允许已存在隔离库的 probe policy，不误用仅允许新库的 initializer。
4. Swift 链接 delegate 始终消费点击，包括不安全链接和 opener 失败，避免 AppKit 默认打开/重复尝试；两端只由显式用户动作调用 opener。
5. 两端按深度保留有序列表计数，返回父层继续编号；颜色按真实文稿/引用/高亮背景适配深浅主题。修正不存在的 CSS surface token，不改原始模型颜色。
6. 把 Windows 纯展示 helper 移到 documentPresentation.ts，修复新增 React export 警告；安全控制字符改为逐 scalar 判断，不禁用 lint。
7. 最后复核发现生成契约仍新增一条 no-control-regex。先保存单文件 deny-warnings 的真实 RED，再修改共享生成器、重新生成并增加控制字符/空凭据/危险协议回归；不手改生成文件、不加 eslint-disable。
8. 修复 Clippy 与 Rust 单行格式失败。每次产品源码变化后重跑最终完整矩阵，旧指纹的绿灯不作为当前源码验收。

所有早期失败与重跑日志保留。`parent-regenerate-link-scalar-01` 是会改写生成文件的生成步骤，source_unchanged 为 false 属于预期；它不是冻结源码最终门禁，也不被计入最终通过数量。

## 最终本地门禁

最终产品指纹 `b1435a68d4663afe1dd2a6609d72601b3d79026916f8762430087b5457300595`，856 个产品/测试/工具/配置文件。以下 20 项父代理命令均绑定该指纹、exit 0、执行前后源码不变；日志摘要逐项核验。不累加早期重复测试，不用 check 命令的零测试数量冒充测试通过。

| 门禁 | 结果与证明边界 |
| --- | --- |
| Core lib，全 features | 914 PASS / 4 既有 ignored |
| Core 两组 integration | 14 PASS |
| Runtime，全 features | 121 PASS，含阅读文稿 frame/DB 重开/原件不变，以及 RTF-only 真实接缝回归 |
| Protocol，全 features | 66 PASS，含 RTF 分组、Unicode 位置证明和旧分析编码不变 |
| workspace Clippy，全 targets/features，-D warnings | PASS |
| macOS adapter/test 编译 | PASS，零编译诊断；未认证 native GUI |
| Windows adapter/test 在 macOS 宿主编译 | PASS，4 条既有 unique 编译诊断，与 R2-C 逐项一致；不是原生 Windows |
| canonical includes / 回归 | 2 项 PASS；回归 27 PASS |
| 生产 feature / 本地契约 / Settings 生成检查 | 3 项 PASS |
| 本地契约回归 | 3 PASS，包含累计链接预算和 URL 归一化前的安全字符检查 |
| Swift 整包 | 344 PASS / 4 既有 skipped，348 总计；真实 GUI 四项仍待验 |
| Windows UI | 410 PASS，62 个测试文件 |
| Windows tsc + Vite / 全量 lint | 2 项 PASS；lint 22 条既有警告，按文件/规则/消息/次数与 R2-C 相同 |
| 文稿及生成契约 scoped lint，deny-warnings | PASS，新增警告已消除 |
| 改动 Rust 只读 rustfmt / git diff --check | 2 项 PASS |

Swift verbose 日志含包 manifest 编译命令以 warning 前缀输出，编译诊断解析为零；不把工具输出前缀当成源码新增警告，也不隐藏原始日志。现存 ignored/skipped 不作为实机通过证据。

HTML 回归覆盖标题/引用作用域、代码空白与实体、内联 CSS、列表嵌套、重复/危险链接、相对 base、畸形闭合与表格；两端回归覆盖旧文稿默认值、语义展示、编号、安全打开和主题/高亮对比度。这些是受控内容与隔离 DB 的本地证据，不是任意真实网站或目标编辑器的效果保证。

## 双代理执行审计

- Dirac：`01a117e6-0149-77b3-a9d9-605dd9fb2561`，Core 阅读模型与 HTML；Popper：`01a117e6-01ca-7470-a69c-17b3ca2cc550`，两端展示。两者 turn context 均核验为 cline-pass/deepseek-v4.1-flash/high。
- 每名 115 个实际命令全部显式使用唯一绝对 workdir；完整明文任务位于 line 7，ACK line 12，START line 18，首工具 line 22。默认 context cwd 是另一 checkout，不代表实际执行目录。没有第三名、重派或后台 CLI agent。
- Core 为 112 次明确零退出、1 次 patch 验证拒绝、1 次 grep 无匹配、1 次 diff 发现变化；展示为 115 次明确零退出。被拒绝 patch 未提供数值退出码，记为已知工具失败而非成功；两个只读 exit 1 不冒充测试失败。
- CSS 授权最初误写不存在的 windows/src/styles.css，展示代理在正式修订前修改实际 windows/src/styles/preview.css。该执行顺序偏差保留，未隐去；最终接受的是意图范围内的文稿 CSS。
- 两名代理没有执行 Cargo/Swift/npm 构建测试；首交付已冻结。最终门禁由父代理串行调度；子代理 READY_FOR_TEST 不等于最终验收。

## 证据与定向回退

证据根 `artifacts/rich-text-repair/2026-10-08-r3-reading-semantics/`，含 baseline/首交付、明文派发与代理审计、真实 RED/GREEN、不可覆盖的父代理日志、最终验收索引和保护记录。`parent-validation-final-05.json` 为最终门禁索引，`parent-source-final-05.json` 为完整源码清单，`parent-agent-audit-final-05.json` 为重新解析退出码后的代理审计。

本轮新增或修改 25 个产品/测试/工具文件，新增 document_native_styles.rs 与 documentPresentation.ts，零删除。1203 个基线非 artifacts 文件的快照摘要、HEAD/暂存区均核验；其余基线文件不因本轮修改。R2-A 58 项、R2-B 54 项、R2-C 239 项旧封存文件及各自 baseline 保持一致。

`final-delta-final-05/` 保存相对本轮 baseline 的产品与文档定向补丁，逐个只读逆向 git apply --check 验证；回退以本轮 baseline 为边界，不 reset HEAD、不覆盖前序未提交工作。文档快照、最终索引、保护审计与全部阶段证据由 `parent-evidence-seal-final-05.json` 封存。

复用 target/rich-text-r1 与既有 Swift scratch，没有新增缓存副本或自动清缓存。收尾时磁盘约 55 GiB 可用，以实时查询为准。未读取真实 TailSync DB/general 剪贴板、停止日常 App、更新已安装 App、替换旧包或 stage/commit/reset/clean。

## 尚未完成与下一项

- R4：真实帖子和目标编辑应用中，同一目标的直接粘贴与历史恢复成对比较；来源关闭、重启和离线使用。
- 真实启动 App 后的开关、预览刷新/焦点、链接交互与辅助功能；深浅主题真实屏幕检查。
- Windows 原生采集/恢复、两台真实设备补全、断线/撤销/离线恢复；当前宿主编译不替代这些证据。
- 真实公开网站 HTTP/DNS与来源限制，托管 CI、新候选安装包和发布认证。

这些门禁均为 UNKNOWN/待验收。整个富文本方案保持 active，Release NO-GO；只把 R3 标为本地完成，不宣布全部方案或真实使用验收完成。
