# 富文本预览来源校验误报修复 — 2026-10-10

用户报告已有图文历史预览显示“部分内容未按原样显示：来源未验证”，授权修复。工作树 `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，分支 `codex/composite-clipboard`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`；保留既有 dirty/untracked 工作，未提交。

实际旧 App 中打开同一条 17:41:04、89 KB 图文记录，标题为“这个本机位置和富文本内容不一致，看着很难受，可以统一移动到右上角吗”。详情为 `unverified_provenance / representation derivation could not be proven`，正文显示 1 个段落、1 张图片、0 张不可用图片。

根因：图片资源提取将原 HTML 中内嵌 data URI 改为绑定资源引用，Windows CF_HTML 解码也会改写表示。runtime 来源判断仅为 QQ 使用完整投影验证；普通 HTML 和 RTFD 派生 HTML 仍依赖转换前后原始字节相等。真实正常转换因此被标为无法验证，文案又把无法证明转换关系解释为内容未显示。

修复 `shared/tailsync-runtime/src/content_analysis.rs`：原字节不能证明来源时，按已有准入流程重新从保留的 NativeArchive 生成 portable 投影，比较完整 bundle，包括文本、表示、图片字节、绑定和完整性状态。一次分析内惰性计算一次，原字节已匹配时不重复投影。仅接受确实保留且 Captured、item 对应的源格式；普通格式映射含 Mac 与 Windows 的现有别名。经此流程规范化的表示标为 Derived，不改变原件或内容身份。RTFD HTML 保留已有 converter/source 归属；QQ 继续要求完整投影证明。无法对应的内容仍报告 Unverified。

Mac 与 Windows 中英文提示同步改为“预览提示：格式转换关系未验证”，详情说明预览与原始剪贴板格式的对应关系无法验证，去除“未显示”的错误断言。真实无法验证仍保留警告级别；缺图、截断及结构损失提示保持现有含义。

回归：先记录普通 HTML、CF_HTML、RTFD 的 3 项真实 RED，以及生产历史保存→读取→文稿预览的 1 项 RED，修复后 GREEN。负向覆盖文字、图片、绑定被替换，源格式缺失及 item 不对应。生产历史回归验证图文正文、保存图片、NativeArchive 和内容身份不变。

最终验证：runtime lib 与历史/分析/QQ/新增 native HTML 集成 104 项，core 分析 22 项，Mac 文稿与文本预览/契约/本地化 76 项，Windows 文稿 renderer/review/cancel 44 项通过。runtime Clippy（lib/tests、deny warnings）与 Windows TypeScript/Vite build 通过。首轮并发 runtime 执行曾有 2 项失败，分别为 source_busy 与预览错误种类不匹配，日志保留；相同完整选择串行执行 104 项全部通过。早期测试代码编译错误与不存在的测试目标调用亦保留，不计为语义 RED 或通过门禁。

最终 Mac App 真实验收：打开上述同一条历史记录，无需重新复制；“来源未验证”横幅消失，图片和原文字继续显示，仍为 1 张图片、0 张不可用图片。GUI/daemon PIDs 11551/11553 均来自最终包。切换前后 11 条历史及全部 9 表逐列摘要相同；配置、QQ bookmark、配对文件、两个身份文件、剪贴板 snapshot/archive 一致。私有数据库备份和真实窗口证据保存在 private/，目录 0700、文件 0600。

包签名、架构、部署版本、helper 自检及启用态 resolver 均 PASS。源码指纹在构建、检查和 App 验收后复核一致。Windows 原生和两设备未验收；本次未提交、发布或更新身份/归档格式。

运行包：`/Users/monet/Downloads/TailSync构建/2026-10-10-preview-provenance-01/TailSync.app`

源码 SHA-256：`d7931b45a1672836c11062f490af8187e3ad6d2258f3205d4aee6dd9a05235e0`

包 SHA-256：`0431255e8179126dd68e17b1773e4132ad650d7b953e0d075ed9b7ec3ff26cb2`

证据：`artifacts/rich-text-repair/2026-10-10-preview-provenance/`，包含 RED/GREEN、最终测试、源码指纹、包清单、同记录 GUI 验收及 preservation-result。
