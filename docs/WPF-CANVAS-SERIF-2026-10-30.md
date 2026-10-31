# Canvas 宋体排版与分类筛选 · 2026-10-30

后续清晰度调整采用正文 500 字重，并配合 Noto Sans SC 界面字体与 13px 小字。
当前方案见 [文字清晰度调整](WPF-TEXT-CLARITY-2026-10-30.md)；以下保留初轮选型依据。

Canvas 正文改为主题提供的宋体候选链：Noto Serif SC、Source Han Serif SC、
Songti SC、STSong、STZhongsong、SimSun、Georgia、serif。正文为 16px、常规字重，
标题优先 Noto Serif SC。搜索、按钮及说明继续使用 UI 字体。Flux、Ledger、Aura、
Mono 的排版不改动。

比较依据为同一 WPF Display / ClearType 路径的合成中英文：SimSun、STZhongsong、
Noto Serif SC 400 和 500，15px 与 16px。Noto Serif SC 400 保留宋体的笔画对比，
比传统系统宋体的小字号点阵表现更均匀；16px 作为当前 Canvas 历史列表正文选择。
这属于当前屏幕的视觉判断，不是对所有显示器的字体质量承诺。

字体是共享 Theme V2 的 reading/display 配置，不是在 WPF 全局替换字体。
WPF 启动回退链同步更新。macOS 和 WebView 现有适配器使用相同主题数据；本次
在 Windows 验证运行，macOS 不声称已运行验收。

本机已有 `C:/Windows/Fonts/NotoSerifSC-VF.ttf`，实际 WPF GlyphRun 确认选到该文件
的 Regular 400 字形，且没有缺字。本次没有安装、修改或分发字体文件；其他机器
没有 Noto Serif SC 时会按候选链回退，发行包若要求完全一致需另行携带授权字体。

历史分类去掉 `command` 与 `structured_data` 两个旧别名。它们已并入后端 code
分类。能力响应中未支持的分类直接从下拉框移除，保留全部分类及实际可用分类，
不再给用户展示无法选择的灰色项目。

复现字体样张：`TailSync.Wpf.exe --serif-specimen-dir=<目录>`，只使用合成文本，
不访问后台、系统剪贴板或用户历史。`--text-render-check-dir` 会记录实际字形文件；
`--migration-check-dir` 检查下拉项与真实 Core 能力一致。
