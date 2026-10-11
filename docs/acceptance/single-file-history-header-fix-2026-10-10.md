# 单文件历史记录组标题修复 — 2026-10-10

旧包真实 History 窗口与用户截图一致：单文件显示“1 个文件”和“全部复制”。生产记录 2809 的 batch_total=1、batch_status=complete，保存格式正常；macOS isBatchStart 和 Windows computeBatchInfos 仅按 batch_id 判断是否显示组标题，造成错误展示。

修复仅调整显示条件，不移除批次身份或修改历史数据。完整的单文件批次不显示组标题；多个文件、筛选后只显示一个成员的多文件批次、未完成状态保留组标题与提醒。缺少计数的旧记录按当前载入的批次成员数回退。

证据：Mac 6 个批次策略回归先出现 2 个预期失败，修复后与既有原生键盘、行交互回归合计 24/24 通过。临时原生 HistoryView 渲染探针另通过（总 25/25），截图 native-groups.png 已复核：单文件无组头、两文件有全部复制、1/3 未完成保留提示；探针源已移至证据目录 .swift.txt。Windows 单文件历史/收藏真实 React 页面测试与纯分组测试先复现 3 个失败，修复后 74/74 通过；TypeScript 与 Vite 构建通过。Windows 原生与真实设备未验收。

新 Mac 包签名、架构、部署版本、helper 和 composite resolver 均 PASS，已正常退出旧包并启动新包。切换前后 5 条历史逐列一致，剪贴板快照、配置和已有图片目录 bookmark 一致。后台新包未打开窗口，UI 工具未获得新包真实窗口，因此此项仍待用户打开窗口确认。

源码 SHA-256：b1e2b7c8e502b6dababa3f4c039832eba7e4dd97bc3e8e8ed757877ef83a8276
包 SHA-256：34b877b836c82a5c2c873b9d9bdba16a4e50a6ed006c2513ecefd2436aee217d
包：/Users/monet/Downloads/TailSync构建/2026-10-10-single-file-history-01/TailSync.app
证据目录：artifacts/rich-text-repair/2026-10-10-single-file-header
