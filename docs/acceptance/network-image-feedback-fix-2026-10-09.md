# 网络图片补全误报修复（2026-10-09）

## 问题与根因

用户报告“售后客服群，有事优先群里沟通 / QQ图片20260908140227 / 1284×2283 168 KB”点击补全网络图片仍提示“部分图片未能保存，原始引用已保留”。

只读核验正在运行的 `2026-10-09-readable-restore-01/TailSync.app` 及生产数据库，定位历史 ID 3115：archive_revision=2，已有一个 PNG 资源和网络原始 JPEG。资源与 img/src 正确绑定，图片尺寸为网页 img/src 指向的 281×500 预览图；正文显示的 1284×2283 是 lightbox 原图链接信息。本次没有改变原图链接选取规则。

原始 HTML 同时保留响应式 srcset 候选、SVG/use 图标以及样式依赖。原来的下载结果 `complete` 使用整个原始 HTML 的离线依赖闭包判定，因而即使 `archived=1; failures=[]` 仍返回 false。重试已经绑定的图片不发网络请求，依然因网页依赖未闭合返回 false。

## 修复范围

- Core 新增正文 img/src 资源完备性判断，复用现有解析、精确绑定、内联 PNG 与预算检查。
- 下载操作单独判断图片是否补全；原始文档的 completeness、完整恢复门禁和所有外部引用继续保留。
- Runtime 的 archive_entry_images 使用图片下载完成状态。无 API schema 或协议变更，两端共用这一生产入口。
- 回归覆盖首次下载、重复点击、原始内容和归档身份保持、真实缺图、无 src、重复 src、截断和无效内联图片。

## 已完成验证

新增 Runtime 回归在修复前复现：已经保存图片、failures 为空，但 report.complete=false。修复后通过。

从真实记录只读提取的归档 bundle 重放结果：download_complete=true; original_document_complete=false; images=1; missing_images=0; network_calls=0; unchanged=true。规范化富文本恢复仍保留正文图片。

Core composite 测试串行 329/329；Runtime 全部 lib 测试串行 74/74；两 crate 的 all-targets Clippy -D warnings；改动文件 rustfmt；git diff --check；11 个 canonical include 双平台检查均通过。初次默认并行测试发生全局 preview/admission 额度争用，串行运行全部通过，保留原始失败日志。

证据目录：`artifacts/rich-text-repair/2026-10-09-network-image-feedback/`。实际记录提取文件仅保存在该本地目录，不作为公开测试 fixture。原生产数据库没有写入。

## 交付与边界

修复 App 输出到 `/Users/monet/Downloads/TailSync构建/2026-10-09-network-image-feedback-01/TailSync.app`；包检查结果见 `package-manifest.json` 和 `results.json`。保留旧 App、运行进程和历史数据，切换运行新 App 后生效。

本轮没有 Windows 原生、真实双设备同步或目标应用粘贴验收。Computer Use 读取现有窗口连续两次超时，因此没有声称 GUI 操作验收通过；真实记录验证通过生产 Core 及 Runtime 测试入口完成。
