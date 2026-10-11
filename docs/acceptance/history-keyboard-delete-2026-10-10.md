# 历史记录键盘删除 — 2026-10-10

macOS 增加 Command + Backspace（⌘⌫），Windows 增加无修饰键的 Delete。仅删除列表当前选中的记录，复用现有历史/收藏删除 API；历史中的收藏记录继续受到保护，收藏窗口维持原有删除语义。搜索框保留原生删字行为，输入法组合、过期查询、恢复/删除进行中、重复按键均不触发删除。成功后选中下一条，尾部回退到上一条；失败保留当前记录和选中状态，可再次操作。

macOS 通过原生 NSWindow、HistoryView 和注入的内存服务验证 responder 路由、搜索删字、输入法保护、连续按键、失败重试及收藏 API。新增用例先复现快捷键无效果，修复后键盘、行交互与批次显示共 47/47 通过。Windows 使用真实 React History 页面和模拟 IPC 验证相同核心行为，页面/键盘测试 71/71 通过，TypeScript/Vite 构建通过。所有删除测试使用内存夹具，未删除任何生产记录。Windows 原生运行未验收。

新 Mac 包的签名、架构、部署版本、helper 自检、composite resolver 检查通过。旧包通过应用菜单正常退出，新包 GUI/daemon 已运行，PID 30387/30388。切换前后全部 8 个数据库表逐列一致，5 条生产历史均保留；剪贴板快照/归档、配置、图片目录 bookmark、设备身份和配对文件一致。新包为菜单栏后台应用，UI 工具无法绑定未打开窗口；真实新包窗口的删除操作未验收。用户实际记录未被用于删除测试。

包：/Users/monet/Downloads/TailSync构建/2026-10-10-history-keyboard-delete-01/TailSync.app

源码 SHA-256：a3b1178bb39fcde5df6a68fcb691f862eacf7e37470f01fcbb658407213db521

包 SHA-256：c114c744ed8fa9d79b97a6b3a5ba87c0f5705ee69137de51cc6aa87a30705f83

证据：artifacts/rich-text-repair/2026-10-10-history-keyboard-delete/，包含修复前失败、修复后回归、构建日志、source-fingerprint.json、package-manifest.json、switch-result.json 及数据备份。
