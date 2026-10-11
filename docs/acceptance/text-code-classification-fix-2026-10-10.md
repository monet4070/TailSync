# 文本预览与代码分类修复 — 2026-10-10

用户授权修复：复制纯文本显示“文稿”，原文本预览工具消失，以及代码、命令、结构化数据分类重叠。实际工作树 /Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35，分支 codex/composite-clipboard，HEAD f719d581a1aa5198a1b3f2483222df5c69e3dfc0；保留既有 dirty/untracked 修改，未提交。

根因：composite 展示分类把任何粗体、斜体、下划线等样式都当作文稿。复制普通消息时附带的 HTML/RTF 样式因此切换到文稿 renderer，文本/代码/Markdown、搜索、换行、字号等文本工具不再显示。另外命令分类带 text 次标签，JSON 另设 structured_data；composite 列表又将存储用 category 强插进派生 categories，让已分类代码仍显示文本。

修复：有文本/HTML 表示且无图片、无表格的消息使用文本 renderer，附带简单样式不会使其成为文稿。图文、表格、仅 native/RTF 的内容继续使用文稿，以免只剩缺省摘要。原始 HTML、RTF、native archive、资源及恢复能力不因显示策略改变。文本分类版本升至 5、display 分类版本升至 1003，已有记录使用现有有界/可取消/CAS 分析自动更新索引。复合记录的存储 category 继续决定保留策略，列表主类别取派生标签，不改变清理配额。

命令、命令块、JSON 和代码都归 code，不再额外附 text 标签。Mac/Windows 分类菜单仅保留 text、website、code、path、image、file。兼容旧 command/structured_data 标签及请求：读入映射、去重、代码筛选别名；旧 legacy 文本通过已有 backfill 更新，旧端契约仍可接收旧名称。

验证：分类器 RED 3 项，display RED 2 项，真实 runtime 分页/筛选/preview RED 1 项，修复后全部 GREEN。最终共享 core 916/916、分类器 6/6（4 个已有忽略项，两个真实外网 Iroh 测试在前轮超时，最终跳过并保留未验收状态）；DB 独立 192/192；runtime history/QQ display 7/7；Mac 预览、分类兼容、行诊断及键盘 127/127；Windows History、Preview、TextPreview、分类兼容、缩略图 113/113；TypeScript/Vite build 通过。Canonical includes 11 项通过。

真实新版 Mac GUI 已确认：文本条目显示“文本”；预览显示文本、代码、MD、搜索、换行、字号、复制全部工具；分类菜单只有一个“代码”，没有命令/结构化数据。QQ 图文记录 3130 仍为文稿并有首图缩略图。所有 composite 索引自动变为 1003。Mac GUI/daemon PIDs 8220/8222 来自下列新包。Windows 原生与双设备未验收。

包签名、架构、最低部署版本、helper 自检、启用态 resolver 均 PASS；源码在构建后复核不变。切换前后 7 条现存记录的所有非分类字段一致，另外 8 表逐列一致；config-v2、QQ bookmark、两个身份文件、配对文件及剪贴板 snapshot/archive 一致。

## 界面误操作与修复限制

验收旧 GUI 时错误把右键当成菜单入口，但 HistoryRowInteraction 的 rightMouseDown 直接删除。这使记录 3136（15:10:42，上一次本聊天的修复回复）被误删。已即时向用户说明并停止进一步右键操作。当前程序启用 secure_delete，SQLite recover 和已有备份未能恢复其 composite 原件。

从本聊天的准确上一条回复提取界面可见的两段文字，使用已有应用密钥在内存中加密，作为纯文本恢复到原 id 和事件记录给出的原时间（2026-10-10T07:10:42.814000+00:00）。恢复文字 348 字节；其余 6 条数据库记录全字段不变；没有改写剪贴板、配置或密钥。原 HTML/native 格式及原 composite identity 无法恢复，此记录不算原件完整恢复。相关 raw/snapshot/recovery 文件均只存私有 0700 本地证据目录，密钥未输出或保存。

包：/Users/monet/Downloads/TailSync构建/2026-10-10-text-code-classification-02/TailSync.app

源码 SHA-256：b361fedfaff474865460d34930e4577c6730ad1467628d276512844c6c15691f

包 SHA-256：9de3bcae54e3aefa5f40d13b4be1b5b8630b4651278342afa25a2854a9e5643e

证据：artifacts/rich-text-repair/2026-10-10-text-code-classification/。此前测试失败及初版包保留；最终验收只绑定 -02 包和 final 日志。
