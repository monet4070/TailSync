# 历史记录来源统一右上角 — 2026-10-10

用户要求将本机与设备来源统一移到历史记录右上角。实际工作树为 `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，分支 `codex/composite-clipboard`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。保留既有 dirty/untracked 工作，未提交。

旧包实际 History 窗口复现：普通文本来源在底部右侧，composite 来源在底部操作按钮旁。两种记录使用不同布局，导致来源位置不一致。

修改 `macos/swift-ui/Sources/TailSync/Views/HistoryRow.swift`：所有记录在同一顶部 HStack 中显示类别、时间、弹性间隔、来源和收藏星标。来源限制 90 点，中间截断，右侧留白 6 点；星标使用正常布局空间，避免浮层覆盖来源。移除旧 footer 和操作按钮旁的来源，底部操作按钮仍按实测宽度给状态文字预留空间。历史与收藏窗口使用同一个行组件。本机显示语义继续使用既有本地化。

验证：最终源的 62 个既有行交互、诊断、恢复菜单、分类兼容和键盘测试全部通过。另用临时 NSHosting 渲染探针生成 6 个样本（三主题 × 两语言），包括图文、本机、长远端设备名、单文件、纯文本和收藏记录；探针移到证据目录 `.swift.txt`，不留在生产测试扫描内。最终原生渲染复核来源均在顶部、长名称截断、星标与来源不重叠。实际最终 App GUI 确认纯文本、图片和 QQ 图文条目的来源均在右上角，QQ 首图缩略图仍显示。

最终包签名、架构、最低部署版本、helper 自检及启用态 resolver 均 PASS。最终源码指纹在构建、测试及切换后复核一致。正常退出旧 App 后启动最终 App，GUI/daemon PIDs 9298/9300 均来自最终包。切换前后 8 条生产历史和全部 9 表逐列摘要一致；配置、QQ bookmark、配对文件、两个身份文件及剪贴板 snapshot/archive 均一致。生产数据未修改；私有备份和真实窗口证据在 `private/`，目录 0700、文件 0600。

Windows 已有 `.item-meta` 顶部来源和 `.item-peer { margin-left: auto }` 布局，本次未修改 Windows；未进行 Windows 原生或双设备验收。

最终包：`/Users/monet/Downloads/TailSync构建/2026-10-10-history-source-top-right-02/TailSync.app`

源码 SHA-256：`ca06ae723462f3fc661643654b730066623954237bd46612efd8ef97f83c399c`

包 SHA-256：`861536ba5b1cc20c9c7e41d630fdea3ee7555796251f0273928b7ed28fdde65f`

证据：`artifacts/rich-text-repair/2026-10-10-history-source-top-right/`。初版 -01 包及初版日志保留，最终验收绑定 -02 包、final 指纹、clean-final 测试日志、visual-final 渲染样本及 preservation-result。
