# Windows 富文本历史显示正文（2026-10-11）

用户要求富文本历史显示里面的文本，以便辨认条目。实存的一条 Mac 富文本包含 lightbox 图片查看器；原 text/plain 摘要先出现 `image`、文件名和 `2557×1320 154 KB`，正文排在后面。HTML 也保留了 lightbox 的 `.meta` 元信息，因此直接去标签不能解决问题。

增加独立的历史正文投影：优先从 HTML 提取正文，按结构忽略 lightbox/lightbox-wrapper 内的 `.meta`，不采集图片 alt/src/title 属性。保留普通 caption、普通 `.meta` 正文、实体、中文和正文中的尺寸字符串。提取限制输入、节点、嵌套深度及文本长度，不解码图片或发起网络请求；剪贴板恢复所用的原 text_summary 保持不变。

新保存的摘要优先使用此正文。已有记录的同步查询、后台读取及关键词搜索复用正在读取并校验哈希的 HTML，读取时投影正文，不迁移或重写 manifest、payload 或同步身份。归档 HTML 使用相同规则；缺失或损坏 HTML 保留保存的摘要。纯 PNG 继续显示图片格式、尺寸和帧数。

自动验证：复合数据库 80 项、HTML 9 项、通用历史数据库 84 项通过（1 项既有忽略），Windows Rust 136 项通过（3 项既有忽略）。数据库回归覆盖新记录、旧摘要、HTML-only、归档层、正文搜索、缺失/损坏 HTML，以及读取前后存储内容和身份一致。此前前端 64 文件、444 项结果复用，本次未更改前端；打包重新构建前端。

交付目录：`windows/release-dual-device-7759a23-rich-text-summary-fixed`。启动烟测、本机数据保留和实际 WebView 历史观察以此目录的 `validation-summary.json`、`runtime-data-preservation.json`、`native-review-result.json` 为准。Mac 双机联调由用户手动完成；此前预览关闭的实机验证状态仍见上一轮验收文档，本次没有重新验收预览关闭延迟。

保留此前启动、重复捕获、紧凑列表、悬浮菜单、纯图片摘要、焦点与原生预览立即关闭修复。完整补修 patch 包含所有未提交修改；源码基线为 `7759a232fc7657df01fd4e482bcb9f7f26f1c930`。
