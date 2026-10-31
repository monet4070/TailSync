# Windows 文字清晰度调整 · 2026-10-30

本轮以用户提供的 macOS 截图为观感参考。目标为更稳定的笔画、小字对比度和阅读
层级；不把更换字体或软件参数等同于已经达到另一台 Retina 显示器的物理清晰度。

前一轮已有 Display 排版、ClearType、Fixed hinting、PerMonitorV2、像素对齐滚动，
文字祖先没有 Effect 或位图缓存。本轮保留这些路径，未改系统字体平滑或显示缩放。

同一合成文案比较了微软雅黑 UI 的 400/500、Noto Sans SC 500、Noto Serif SC
400/500，以及 Display / Ideal 排版。GlyphRun 记录显示：本机微软雅黑 UI 请求
500 时实际为 400；Segoe UI Variable Text 请求 500 时选到的命名字形为 600；
Noto Sans SC 与 Noto Serif SC 可选到真正的 500 字形。

Canvas 的 UI 角色优先 Noto Sans SC / Source Han Sans SC，正文仍用 Noto Serif SC
/ Source Han Serif SC。Canvas 的 history、panel 组件使用 500 字重。WPF 的正文和
设置标题现在消费主题组件字重，作者在自定义主题中指定的字重保持优先权。
其他内置主题的字体族和组件字重不变。

说明文字、分类控件和历史元数据采用从主题 UI 字号派生的 DetailFontSize
（默认 13px，即 UI 字号减一）。文件大小由过浅的 tertiary 改为 secondary，时间、
来源与其统一。正文仍是 Canvas 16px，不通过位图缩放或描边加粗文字。

字体均使用本机已有安装，不在该构建中安装或分发字体文件；未安装 Noto 字体的
其他机器会使用主题的回退链。本轮在 Windows 验证字形选择、浅深色对比度、窄
窗口布局、滚动及原有功能。Mac 截图是参考，不能替代同屏同缩放的双平台验收。

对比样张可通过 `--text-clarity-dir=<目录>` 生成。文案完全合成，不读取剪贴板；
`--clarity-review` 可保留样例窗口。渲染树 PNG 不衡量显示面板的物理亚像素表现。
