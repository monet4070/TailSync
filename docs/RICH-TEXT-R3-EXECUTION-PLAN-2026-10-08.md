# R3：常用阅读语义执行方案

状态：**R3 LOCAL_COMPLETE（2026-10-09）；真实 GUI/目标应用/设备 UNKNOWN；Release NO-GO**。双子代理交付、父代理独立二修及 20 项同指纹最终本地门禁完成，详见 [R3 验收](RICH-TEXT-R3-ACCEPTANCE-2026-10-09.md)。不代表已安装 App、原生 GUI、Windows 或双设备验收完成。工作树为 `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。沿用总体范围方案与执行记录，R2-C 封存不变。

## 目标与范围

围绕帖子阅读与编辑器复制保留格式，补齐三级统一标题、安全超链接、引用、行内代码、保留空白的代码块、实色高亮及有界列表层级。维持简单表格、已有六种强调与图片位置。预览不是完整网页/CSS/Word 引擎，不从简化预览重建原件。

HTML 语义标签和常用内联 CSS 映射到统一阅读模型；不运行脚本、样式表、自动链接探测或外部请求。链接只允许无凭据、无控制字符的明确 HTTP(S) 地址，经用户点击才交给系统打开；相对地址只有存在可信 HTML base 才解析，不猜来源。未知装饰仍只进入折叠诊断。

适量原生样式映射限定于已有有界 RTF scanner 能证明的逐位置基础强调：必须与选定正文逐字对应才应用，组作用域与开关保持；没有位置证明时继续诚实显示统一正文。不把 style_bits 的整篇汇总应用到全部正文，不新增 RTFD 通用解析器，不改变原始 RTA1、manifest、wire、数据库或恢复身份。

## 模型与兼容

保留 DisplayDocument v1 和既有 paragraph/table/image 类型。DocumentParagraph 增加可缺省的 heading、quote_depth、code_block、list_depth；DocumentRun 增加可缺省的 link；DocumentRunStyle 增加可缺省的 monospace 和 highlight。旧文稿缺字段可读，旧消费者忽略新增字段；由生产 Rust 导出生成 Swift/TypeScript/schema/fixtures，禁止手改 generated 文件。新增字符串与内存计入既有文稿预算，深度不超过八级，标题映射到三级。

## 双 DeepSeek/high 与父代理分工

1. 阅读模型代理：只修改 Core display_document/document_build 及其内置回归，实现模型、HTML 解析、验证与旧数据默认值；先 ACK，收到 START 再实施。
2. 两端展示代理：只修改 Swift 文稿 material/view/tests 和 Windows 文稿 renderer/grouping/CSS/tests；使用冻结字段实现一致样式、显式安全链接打开、可读颜色与高亮，不增加模式或格式按钮。
3. 父代理：保存基线、实现有限 RTF 位置证明、更新生产契约及其他构造器接线；不重做两代理写集。完整交付后冻结首版、逐文件独立审查、补真实 RED 回归、二修与最终门禁。

模型为 cline-pass/deepseek-v4.1-flash，effort high，完整明文且 fork_context=false。不再恢复已关闭代理，不使用第三名或 CLI 后台替代。Rust 构建全部由父代理串行调度，两名代理未收到 GO_TEST 不运行 Cargo/Swift/npm 门禁，避免 file-lock 和缓存膨胀。

## 验证与保护

专项覆盖嵌套/畸形作用域、代码空白和实体、链接危险协议/重复属性/实体/缺失 base、原图顺序、旧模型解码、预算、RTF 组与 Unicode 位置。两端覆盖语义、深色/浅色对比度、安全打开只能由点击触发，真实 GUI/辅助功能另验。

全部通过隔离 runner，复用 target/rich-text-r1 与既有 Swift scratch，不新增缓存副本。最终运行 Core/Runtime/Protocol、生成契约、Clippy、两 adapter、Windows UI/build/lint、Swift 整包与只读格式/diff；日志绑定不可变最终源码指纹，失败尝试保留。禁止读取用户 DB/general 剪贴板、停止日常 App、替换旧包、stage/commit/reset/clean、放宽预算或添加 allow/ignored。

真实目标应用的直接/历史粘贴对照、来源关闭/重启/离线、Windows 原生、辅助功能与两设备属于 R4。未获得这些证据前，整个目标保持 active、Release NO-GO。
