# TailSync 无 WebView 预览对照 Demo

本目录提供三个独立的 Windows x64 程序，用于比较原生文档引擎。共用同一 WPF 窗口、输入文件、像素宽度和自动操作流程，**不连接 TailSync 后台，不读取历史数据库，不修改系统剪贴板**。

| Demo | 引擎 | 格式 | 本轮能力 |
|---|---|---|---|
| `TailSync.Preview.Pdfium.exe` | PDFium，NuGet `bblanchon.PDFium.Win32 158.0.8111` | PDF | 页面、翻页、缩放、提取当前页文字 |
| `TailSync.Preview.MuPdf.exe` | `MuPDF.NET 3.28.2.4` / native `1.28.2` | PDF、DOCX；XLSX/PPTX 为额外探索入口 | 页面、翻页、缩放、提取当前页文字；Office 重排 |
| `TailSync.Preview.WindowsPdf.exe` | Windows.Data.Pdf | PDF | 页面、翻页、缩放；明确不提供文字提取 |

这是**预览引擎对照**，不是 Slint/WPF UI 框架性能对照。MuPDF.NET 还带有托管封装、字体及其他依赖，结果不能直接代表将来 Rust/C API 的最小实现。不同引擎适配均计入复制/解码耗时：PDFium 输出 BGRA；MuPDF RGB 转 BGRA；Windows API 输出编码图像再解码。三个程序均显示单页、不做页面缓存、不预取邻页，不运行文档 JavaScript 或主动打开链接。

本轮不含页面上的文字选区、全文件搜索、缩略图导航、密码输入或生产沙箱。PDFium/MuPDF 的“文字”按钮打开当前页提取文本，可选择、复制、查找；这不等于原页面文字选择层。图片缩放只保留当前像素结果。Office 页数和分页可能与 Word 不同；实际差异应留作评估结果，不能当作已等价移植。

## Windows 运行

要求 Windows 10 2004 / build 19041 或更新、**x64**。便携包自带 .NET，不需安装 SDK；MuPDF 另需 Microsoft Visual C++ x64 Runtime。启动脚本会检查常见缺失 DLL；缺失时从 [Microsoft 官方地址](https://aka.ms/vs/17/release/vc_redist.x64.exe) 安装，不会自动修改系统。

解压整份便携包，不要只复制 exe。先通过脚本启动：

```powershell
.\run-demo.ps1 -Engine Pdfium -File .\fixtures\reading-3-pages.pdf
.\run-demo.ps1 -Engine MuPdf -File .\fixtures\reading.docx
.\run-demo.ps1 -Engine WindowsPdf -File .\fixtures\reading-3-pages.pdf
```

若 PowerShell 阻止未签名的本地脚本，可仅对本次进程运行：`powershell -NoProfile -ExecutionPolicy Bypass -File .\run-demo.ps1 -Engine Pdfium`，不需修改系统执行策略。

也可直接运行各目录的 exe，再点击“打开文件”。`Ctrl+O` 打开，左右键翻页，`Ctrl+滚轮` 缩放，`Esc` 释放当前文档。关闭文档保留进程，以观察释放后的内存；关闭窗口退出进程。后台渲染操作串行执行，忙时不积压新操作；原生调用正在运行时关窗需要等待本次调用结束，进程外测试脚本另有超时上限。

程序仅处理用户选择的文件；64 MiB 输入上限、16 MP / 8192 px 单页位图上限。使用已知的合成夹具做第一轮比较。进程隔离和输入上限不构成完整安全沙箱。

## 一键对比

关闭其他 demo，保持测试窗口可见、不最小化，固定电源模式、显示器缩放和后台负载：

```powershell
# 默认使用包内 2 个 PDF 和 2 个 DOCX；PDF 对比三个引擎，DOCX 只跑 MuPDF。
.\compare.ps1

# 快速确认三个 PDF demo 都能打开并结束，不用它得出性能结论。
.\compare.ps1 -Files .\fixtures\reading-3-pages.pdf -Rounds 1 -Cycles 1

# 自选文件与足够样本；100 次开关用来观察长期资源趋势。
.\compare.ps1 -Files 'D:\samples\中文 文档.pdf' -Rounds 3 -Cycles 30
.\compare.ps1 -Files 'D:\samples\中文 文档.docx' -Rounds 1 -Cycles 100
# 多页文档指定远距离跳页，不默认渲染全部 60 页。
.\compare.ps1 -Files .\fixtures\mixed-60-pages.pdf -Pages 1,15,30,60 -Rounds 3 -Cycles 10
```

默认每文件 3 次独立进程启动，每进程 3 次打开/关闭；空窗等待 5 秒 → 打开首屏 → 依次渲染第 1/2/3 页 → 双倍像素宽度 → 恢复 → 关闭文档 → 等待 5 秒。实际不存在的页会记录 skipped。每轮轮换引擎顺序，串行执行，不并发抢资源。DOCX 不会伪装成三引擎可比。默认逐页间隔 500 ms，正常宽度 1200 px，双倍 2400 px。

日志位于 `results/compare-时间/`，包含机器信息、输入 SHA256、进程退出码和每次运行的 `events.jsonl`。失败/超时保留日志并返回非零；日志不记录正文或提取文本，但环境参数包含用户选择的文件路径。

可将整个结果目录发回分析；本地有 Python 3 时也可以：

```powershell
python .\summarize.py .\results\compare-时间
```

生成 `summary.json` / `summary.csv`，区分失败与完整运行，缺失计数保留空值。默认样本少，仅用于初筛，不能视为可靠的发布 p95。

## 指标含义

- `engine_render`：引擎页面加载/渲染及该适配的像素转换耗时，不含 WPF 显示。
- `frame_assigned`：渲染请求到像素交给 WPF 的耗时；首屏另外包含读文件、SHA256、加载和首屏渲染。
- `render_tick`：赋值后的 WPF 渲染周期事件。**不是物理呈现时间，也不是输入到屏幕延迟**；可见性/最小化导致无事件时会超时报错。
- 资源每 250 ms 采样：Private Bytes、Working Set、系统支持时的 Private Working Set、CPU 累计时间、线程、句柄、托管堆。Working Set 不冒充私有驻留；不支持的 Private Working Set 为 null。采样可能遗漏短暂峰值。
- 空窗基线、文件打开、释放后静置分别有事件标记。没有强制 GC 或 working-set trim；MuPDF native caches、CLR 和 WinRT 对象回收行为会如实体现在结果中。
- 首轮是**进程冷启动后的首次打开**，不是保证磁盘/系统缓存已清空。GPU 资源、系统共享组件、进程外组件没有被本进程计数覆盖，后续正式测量使用 WPR/WPA 补齐。
- 用相同输入 SHA256、像素尺寸和操作比结果；Office 重排和 PDF 固定版式不是等价任务。仅看到内存更低、文字丢失或页面为空的结果不算胜出。

## 合成夹具

`fixtures/manifest.json` 记录文件大小与 SHA256：

- `reading-3-pages.pdf`：嵌入中文字体、可搜索文字、表格、图片。
- `mixed-60-pages.pdf`：同类 60 页，12 种不同图片循环；并非 60 张独立超大扫描页。
- `reading.docx`：3 个内容分区，中文、样式、列表、基础表格和行内图片。
- `layout.docx`：额外包含合并单元格、页眉、页脚页码，检查重排差异。

DOCX 指定 Arial / Microsoft YaHei，Windows 字体可用性影响结果。生成器 `make-fixtures.py` 接受可嵌入的中文 TTF；本次 PDF 内嵌字体子集，不分发独立字体文件。夹具只含合成内容，没有个人剪贴板数据、外部资源或宏。

## 源码构建与检查

从源码目录运行（便携包内为 `source/`），需要 .NET SDK 10.0.402（同 feature band 最新补丁可用）。固定 NuGet 版本及 `packages.lock.json`：

```powershell
.\build.ps1 -Publish
# 只构建一个候选
.\build.ps1 -Engine MuPdf -Publish
```

产物位于 `publish/`。发布包不使用 trimming、single-file 或 ReadyToRun，避免构建模式改变对照。三者均为同一 self-contained 配置；发布大小不是内存测量。

三个候选均发布后，可用 `python package.py --output <新的zip路径>` 打包便携程序、夹具、demo 源码、许可和 SHA256。包内每个候选自带运行时，所以测试包比最终单一方案的客户端大；不要拿三份运行时之和当作预计正式包体。

```powershell
dotnet run --project Checks/Checks.csproj -c Release
dotnet run --project MuPdf.Probe/MuPdf.Probe.csproj -c Release -- fixtures/reading.docx
dotnet run --project Pdfium.Probe/Pdfium.Probe.csproj -c Release -- fixtures/reading-3-pages.pdf
python -m unittest discover -s tests
```

探针只测试引擎打开/渲染/提取/关闭，不覆盖窗口、输入法、DPI 或 Windows 性能。MuPDF.NET 当前固定 native 包缺少 macOS 库，MuPdf.Probe 需要 Windows 或该包支持的 Linux 环境；PDFium 探针支持 macOS 和 Windows。

## 当前验证状态

- 三个 Windows x64 Release 项目已在 macOS 交叉编译。
- 16 个资源上限、参数和输入读取检查通过。
- PDFium macOS 原生探针：3 页、60 页 PDF 各 3 次打开/关闭，首尾页渲染及文字提取通过，重复像素 hash 一致。
- 夹具 PDF 和 DOCX 已通过独立渲染检查；此检查验证夹具本身，不代表 MuPDF 的 Office 兼容性。
- **Windows 控件实例化、所有引擎的实机加载、MuPDF DOCX 版式、内存与响应时间尚待实机验收。** 不提供虚构性能排名。

## 许可和依赖

MuPDF C 核心提供 AGPL/商业许可路径，但**本 demo 实际固定的 MuPDF.NET 和 NativeAssets 包使用 Artifex Community License，限非商业使用，商业用途需另行授权**。两者不能混称。它们保留在独立评估目录，未接入正式 TailSync MIT 客户端；本 demo 不代表已完成生产授权决策。

This software is based on MuPDF.NET, developed by Artifex Software, Inc. and licensed under the Artifex Community License.

PDFium 与其依赖许可、实际 MuPDF.NET/native 包许可和来源见 `licenses/`；托管依赖与版本见锁文件。便携包附 demo 源码及构建方式，源码中没有修改引擎。所有引擎仍保留各自版权和许可。
