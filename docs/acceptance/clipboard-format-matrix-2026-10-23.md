# 剪贴板格式入库、回溯与预览验收（2026-10-23）

**后续状态：RTF 发布缺陷已修复，85 个样本重新验证通过，修复包已安装。** 下文保留修复前版本的失败证据与结论；根因更正、修复范围和新包结果见 [RTF 修复验收](rtf-restore-fix-2026-10-23.md)。

结论：**不能判定全部通过**。本机 macOS 的 85 个样本全部可采集、持久化并从新数据库连接读回；83 个可默认回溯，2 个 RTF 相关样本默认回溯被 `originals_incomplete` 拒绝。两个失败样本的显式单格式回溯及预览均通过。

覆盖当前 Swift 支持的全部 59 种文件扩展名，另加 XLSX、ZIP 两个预期仅显示信息的负向对照，共 61 个文件样本，以及 24 种原生剪贴板形态。85 个由同一历史记录生成的真实预览帧均交给 Swift 消费；81 个具有内容/静态封面预览，4 个只有信息或摘要。

## 证据边界

- 原生内容：已安装 App 内的 helper → 私有命名粘贴板 → CNA1 → 生产 `process_composite_capture` → 隔离加密 SQLite → 新连接与实际历史分类查询 → 实际恢复计划/原生准备 → 同 helper 发布 → 回读字节比对。
- 文件：系统文件列表读取 → 共享 `prepare_file_batch` → 与生产相同的 `HistoryDB.add_file_batch` → 新连接 → 单条/整批回溯、预览数据与原字节比对。61 个文件按 20/20/20/1 分组；前三级批次均超过 composite 的 16-item 上限。文件测试没有调用带网络发送的完整轮询器。
- 预览：同一历史生成 TSPV/TSDC 帧 → 当前 Swift 解码/分类/内容物化；图片强制解码成 CGImage，PDF 验证两页及第二页文字，SVG 实际经 WKWebView 栅格化，文稿验证图片位置与缺图占位。
- Office：从历史帧提取 DOCX/PPTX/PPT 原字节，系统 Quick Look 成功生成缩略图，逐张检查三张缩略图，均有正确内容。
- 所有样本均为测试内容。使用独立数据库和 `tailsync-probe.*`，未把样本放入用户 general 剪贴板、生产历史或真实设备；本任务没有替换 App。
- 本轮不是每个样本在完整 App GUI 上的端到端操作验收，也不认证 Windows 原生、跨设备、真实来源/目标应用粘贴。微信/QQ 合成采集回归包含在 31 项平台测试中，不能当作实际应用验收。

## 阻断项：RTF 的系统声明格式不可读

使用系统 `textutil` 生成有效 RTF，再经实际打包 helper 写入。macOS 为 RTF-only 声明 `public.utf16-external-plain-text` 与 `public.utf8-plain-text`，两者的 item/board data 与 string 读取均返回 nil；RTF 本身能被 NSAttributedString 解析。混合 text+HTML+RTF 已显式提供 UTF-8，仍出现不可读的 UTF-16。

当前代码保留这些失败事实。RTF-only 经有界重试后保存；混合内容也保存。默认恢复被 `originals_incomplete` 拒绝，因而目前不能保证“支持 RTF 的每一种组合都能双击回溯”。显式 RTF（RTF-only）或 HTML（混合样本）可恢复且字节比对通过。RTF-only 的原生文稿预览正常，混合样本按既有策略显示文本预览。

试验向 helper 增加 200 ms RunLoop 等待没有改变结果；该试验没有进入产品代码。证据见 `rtf-lifetime-experiment.json`、`rtf-synthesis-probe.swift`、`rtf.archive`、`text-html-rtf.archive`。此轮保留 fail-closed 规则，没有通过删掉失败格式或放宽原件完整性门槛制造通过。

原生矩阵会检查完 24 个样本再报失败，失败 gate 未改成绿色或“预期失败”。后续修复应先确认源声明与系统合成类型的界限，再决定完整恢复和显式已保存内容恢复的行为；不应把未读到的原件标为已捕获。

## 能力限制

- GIF、动画 WebP/APNG、多页 TIFF 原字节可恢复；预览是静态封面，不能据此宣称预览会播放全部帧/页面。
- 网页单图有已保存 PNG 时可回溯 PNG，但仍报告 1 张 HTML 引用图片未保存；含正文的缺图网页恢复已保存内容并保留缺图提示。两者不是完整网页恢复。
- 未知原生类型与多个原生 item 可按原字节保存和回放，当前只有摘要预览。
- XLSX、ZIP 可入库和回溯，当前只显示文件信息。纯 Markdown 字符串按文本预览；`.md`/`.markdown` 文件使用 Markdown 渲染。纯文字 HTML/多表示编辑器内容按既有产品策略显示文本，恢复时仍保留相应原格式（RTF 阻断情形见上）。

## 原生样本逐项结果

| 样本 | 入库/新连接读回 | 默认回溯 | 预览 | 说明 |
|---|---|---|---|---|
| plain-text | 通过 | 通过 | 文本 |  |
| url | 通过 | 通过 | 文本 |  |
| code | 通过 | 通过 | 代码 |  |
| command | 通过 | 通过 | 代码 |  |
| json | 通过 | 通过 | 代码 |  |
| path | 通过 | 通过 | 文本 |  |
| markdown-text | 通过 | 通过 | 文本 |  |
| html | 通过 | 通过 | 文本 |  |
| rtf | 通过 | **失败：originals_incomplete** | 图文文稿 | 显式 RTF 回溯通过；系统合成纯文本格式不可读 |
| text-html-rtf | 通过 | **失败：originals_incomplete** | 文本 | 显式 HTML 回溯通过；系统合成 UTF-16 不可读 |
| opaque-native | 通过 | 通过 | 仅摘要 |  |
| multiple-native-items | 通过 | 通过 | 仅摘要 |  |
| browser-image-missing-html | 通过 | 通过 | 图片/静态封面 | 回溯已保存内容；仍缺 1 张 HTML 引用图片 |
| html-missing-image-with-text | 通过 | 通过 | 图文文稿 | 回溯已保存内容；仍缺 1 张 HTML 引用图片 |
| png | 通过 | 通过 | 图片/静态封面 |  |
| jpeg | 通过 | 通过 | 图片/静态封面 |  |
| tiff | 通过 | 通过 | 图片/静态封面 |  |
| gif-animation | 通过 | 通过 | 图片/静态封面 | 原件字节保留；静态封面预览 |
| webp-static | 通过 | 通过 | 图片/静态封面 |  |
| webp-animation | 通过 | 通过 | 图片/静态封面 | 原件字节保留；静态封面预览 |
| apng-animation | 通过 | 通过 | 图片/静态封面 | 原件字节保留；静态封面预览 |
| tiff-multiple-pages | 通过 | 通过 | 图片/静态封面 | 原件字节保留；静态封面预览 |
| html-two-images | 通过 | 通过 | 图文文稿 |  |
| rtfd-attachment | 通过 | 通过 | 图文文稿 |  |

## 文件扩展名逐项结果

| 文件样本 | 入库/新连接读回 | 单条/整批回溯 | 预览 |
|---|---|---|---|
| sample.bash | 通过 | 通过 | 代码 |
| sample.bmp | 通过 | 通过 | 图片/静态封面 |
| sample.c | 通过 | 通过 | 代码 |
| sample.cc | 通过 | 通过 | 代码 |
| sample.cfg | 通过 | 通过 | 代码 |
| sample.conf | 通过 | 通过 | 代码 |
| sample.cpp | 通过 | 通过 | 代码 |
| sample.cs | 通过 | 通过 | 代码 |
| sample.css | 通过 | 通过 | 代码 |
| sample.csv | 通过 | 通过 | 文本 |
| sample.cxx | 通过 | 通过 | 代码 |
| sample.docx | 通过 | 通过 | Quick Look DOCX |
| sample.fish | 通过 | 通过 | 代码 |
| sample.gif | 通过 | 通过 | 图片/静态封面 |
| sample.go | 通过 | 通过 | 代码 |
| sample.h | 通过 | 通过 | 代码 |
| sample.heic | 通过 | 通过 | 图片/静态封面 |
| sample.heif | 通过 | 通过 | 图片/静态封面 |
| sample.hpp | 通过 | 通过 | 代码 |
| sample.htm | 通过 | 通过 | 代码 |
| sample.html | 通过 | 通过 | 代码 |
| sample.ini | 通过 | 通过 | 代码 |
| sample.java | 通过 | 通过 | 代码 |
| sample.jpeg | 通过 | 通过 | 图片/静态封面 |
| sample.jpg | 通过 | 通过 | 图片/静态封面 |
| sample.js | 通过 | 通过 | 代码 |
| sample.json | 通过 | 通过 | 代码 |
| sample.jsonl | 通过 | 通过 | 代码 |
| sample.jsx | 通过 | 通过 | 代码 |
| sample.kt | 通过 | 通过 | 代码 |
| sample.kts | 通过 | 通过 | 代码 |
| sample.log | 通过 | 通过 | 文本 |
| sample.markdown | 通过 | 通过 | Markdown |
| sample.md | 通过 | 通过 | Markdown |
| sample.pdf | 通过 | 通过 | PDF |
| sample.php | 通过 | 通过 | 代码 |
| sample.png | 通过 | 通过 | 图片/静态封面 |
| sample.pptx | 通过 | 通过 | Quick Look PPT/PPTX |
| sample.ps1 | 通过 | 通过 | 代码 |
| sample.py | 通过 | 通过 | 代码 |
| sample.rb | 通过 | 通过 | 代码 |
| sample.rs | 通过 | 通过 | 代码 |
| sample.sh | 通过 | 通过 | 代码 |
| sample.sql | 通过 | 通过 | 代码 |
| sample.svg | 通过 | 通过 | WebKit SVG |
| sample.swift | 通过 | 通过 | 代码 |
| sample.tif | 通过 | 通过 | 图片/静态封面 |
| sample.tiff | 通过 | 通过 | 图片/静态封面 |
| sample.toml | 通过 | 通过 | 代码 |
| sample.ts | 通过 | 通过 | 代码 |
| sample.tsv | 通过 | 通过 | 文本 |
| sample.tsx | 通过 | 通过 | 代码 |
| sample.txt | 通过 | 通过 | 文本 |
| sample.webp | 通过 | 通过 | 图片/静态封面 |
| sample.xlsx | 通过 | 通过 | 仅文件信息 |
| sample.xml | 通过 | 通过 | 代码 |
| sample.yaml | 通过 | 通过 | 代码 |
| sample.yml | 通过 | 通过 | 代码 |
| sample.zip | 通过 | 通过 | 仅文件信息 |
| sample.zsh | 通过 | 通过 | 代码 |
| slides.ppt | 通过 | 通过 | Quick Look PPT/PPTX |

## 校验与复现

- Core：990 项单元测试通过、3 项跳过；其余集成测试通过，原始输出见 `core.log`。
- Runtime：141 项通过。
- Swift 全套：420 项执行，5 项跳过，0 失败；跳过项含本次需显式启用的矩阵，以及原生焦点/WebKit 可选验收。其后单独启用 85 帧矩阵，通过 1 项测试（覆盖全部 85 个样本）。
- 平台采集回归：31 项通过，2 项可选矩阵默认跳过。
- 文件矩阵：61 行通过。原生矩阵：24 行全部检查，2 个默认恢复失败，最终非零退出。
- Canonical includes 与跨平台契约检查通过；它们不替代 Windows 原生测试。

测试入口与环境准备见 [clipboard-probe](../../tools/clipboard-probe/README.md)。设置 `TAILSYNC_FORMAT_MATRIX_DIR` 为仓库 `artifacts/format-roundtrip/2026-10-23` 的绝对路径。macOS 使用 CommandLineTools 与 MacOSX26.5 SDK；没有接受 Xcode 许可或更改全局工具链。

所有明细、原始日志、原始归档/预览帧、Office 缩略图、测试与已安装二进制摘要位于 [验收证据](../../artifacts/format-roundtrip/2026-10-23/results.json)。PPT 样本来源与摘要见同目录 `external-fixture-provenance.json`。
