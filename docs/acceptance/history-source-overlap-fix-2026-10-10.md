# macOS 历史记录来源标签重叠修复 — 2026-10-10

用户截图与旧包真实窗口均显示 `self` 与详情按钮重叠。原因：来源在行内部，操作按钮则位于 bottomTrailing 浮层；固定 52 点预留不能覆盖当前菜单、箭头与按钮的实际布局。

来源标签已和按钮放到同一 HStack，记录实际宽度给行的元数据区域留出间隔。长来源名称在 90 点内中间截断；来源标签不捕获点击，仍由原行 responder 处理。元数据使用末行基线对齐。本机内部标记仅在显示层翻译为 本机 / This device；存储与通知来源语义不变。

验证：原问题在旧包真实 History 窗口复现。三主题（Canvas、Forest、高对比）、两语言的 6 个原生 NSHosting 渲染样本均已生成；复核中文 Canvas 与英文 Forest 中来源和按钮不重叠、长名称截断。31 项检查通过（30 个既有交互、诊断和菜单测试，加 1 个临时原生渲染探针）。探针已从生产测试扫描中移到本证据目录 `.swift.txt`，截图位于 `visual-final/`。新包架构、签名、部署版本、helper 与 composite resolver 检查 PASS。

正常退出旧包并启动新包，实际 PID 路径已确认。切换前后 4 条生产历史逐列相同，剪贴板快照、配置及已有 QQ bookmark 保持相同。新包在后台启动时没有窗口，UI 工具因此未能完成新包真实窗口复核；不把原生渲染当作完整 App GUI 验收。

包：`/Users/monet/Downloads/TailSync构建/2026-10-10-history-source-layout-01/TailSync.app`
源码：`a6ec32557e7edfe1df4cfe0515f0ac017cce283d0075d5c69d3fd45ac8a32864`
包 SHA-256：`fd32100c0300871afcfec5c439d9640da86967c4371249198bfa8382069d1daf`
证据：`artifacts/rich-text-repair/2026-10-10-history-source-overlap`
