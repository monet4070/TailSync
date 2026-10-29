# 网页图文同步：2026-10-29 本机验收

用户目标是同步网页正文、图片及图片在文字中的位置；字体、颜色和复杂排版可以简化。此次按用户确认完成本机原生与浏览器验收，明确保留双设备缺口。

## 实施结果

- 实时接收和历史默认恢复共用文稿选择。已取得的图片保持原节点与重复出现顺序，缺图在原位置占位；缺失事实不因生成可用副本而抹去。
- 沿用已有网络图片设置，在首次事件冻结之前最多花 2 秒取得公开 PNG／JPEG／静态 WebP；取消、新复制和存储失败走受控降级。原始来源与首发便携内容原子入库，后补图片只更新历史。
- 修复 HTML 与纯文字 RTF 竞争造成丢图：默认图文保留原 HTML 和绑定，macOS 补充原生 RTFD。显式格式选择、历史原件及协议版本不变。
- WebKit 普通选区的字距、描边属性允许简化，不再因此拒绝整段正文和附件。

## 原生与浏览器结果

构造选区为 `Alpha → 红图 → Beta → 绿图 → Gamma → 红图 → Omega`。WebKit 中真实选择正文，经 macOS Services 导出到私有 pasteboard，再由产品 helper 采集 CNA1、隔离历史入库、默认恢复、产品原生准备与发布，最后由真实 AppKit `NSTextView` 粘贴。GUI 同时显示来源与接收文稿。

HTML 单独存在、HTML 附带纯文字 RTF、WebKit 真实选区三种情况全部保留 3 张图片，附件 UTF-16 位置均为 `[5, 10, 16]`，颜色顺序为红／绿／红，第一次与第三次图片相同。HTML 的行内图片位置没有被重建成文末附图或独立段落。结构化结果见 [web-article-native-2026-10-29.json](web-article-native-2026-10-29.json)。

测试前复现：HTML 单独存在时为 3 张图片，附带纯文字 RTF 时为 0 张图片；仅合成带 `\\pict` 的 RTF，在 AppKit 中仍为 0 张。故采用原生 RTFD。

这里验证的是 WKWebView 引擎的原生选区导出与 AppKit 原生接收器，未把它冒充为 Safari／Chrome 完整应用的系统复制快捷键验收。全部测试操作 `tailsync-probe.*` 私有 pasteboard，没有修改 general pasteboard、真实历史、设置或已安装 App。

## 自动化回归

| 范围 | 结果 |
|---|---|
| Core | 997 项单元测试通过，3 项跳过；默认集成测试通过 |
| Runtime | 92 项单元、48 项集成通过；公网与原生导出测试按需执行 |
| macOS Rust 平台 | 177 项通过，7 项可选／原生测试跳过 |
| Windows Rust，macOS host | 189 项通过，4 项跳过；保留 3 条既有 cfg 裁剪相关警告 |
| macOS Swift UI | 417 项执行，5 项跳过，0 失败 |
| Windows 前端 | 62 个测试文件、507 项通过；TypeScript／Vite 构建通过 |
| 脚本与原生 UI 设计参考 | 69 项 Node 脚本、8 项 Native 参考页、9 项 WPF 参考页通过 |
| Python | QQ 目录授权 1 项、预览实验结果汇总 4 项通过 |

另外执行 Core 接收器故障与来源指纹回归、Core／Runtime 全 target Clippy、根 workspace 与两端格式、canonical include、跨平台契约与 onboarding 生成一致性检查。

前端测试入口限定为 `src/`，原生 UI 实验中的 `node:test` 通过独立 Node 命令执行。首次高负载并行运行有一项历史键盘断言未观察到瞬时提示；单文件完整重跑 69 项及最终前端全量 507 项均通过，保留该次失败日志于本机。

## 公开网络限制与待验收项

以实际 `ReqwestTransport` 执行公网图片前置获取时，当前系统 DNS 将 `www.w3.org`、`www.gstatic.com` 返回为 `198.18.*` 代理地址，触发公开地址校验并拒绝获取。此项保留为环境未通过，没有绕过地址校验。真实 HTTP 客户端的有界 PNG／JPEG／WebP 响应、完整首发、超时、取消、部分成功与离线历史恢复均有本地回归，但不能替代真实 CDN 验收。

尚未完成：真实双设备传输与双向粘贴、Windows 原生目标应用、Safari／Chrome 完整应用、登录／blob／来源未交付的懒加载图片，以及复杂 CSS 造成的视觉顺序差异。Windows C# 实验原生工程没有在本机 macOS 编译。Windows host 测试或设计参考页通过不等同于这些原生环境通过。

## 重现入口

常规 Rust 测试经 `node scripts/run-isolated-tests.mjs -- cargo test …` 隔离真实应用数据。原生工具源码在 `tools/clipboard-probe/`：

1. 用当前 helper 编译产物运行 `probe.py --helper … --self-test`，包括带 WebKit 装饰属性的 RTFD 转换回归。
2. `o06_restore_plan::export_native_web_article_consumer_fixtures` 导出 CBP；平台 `export_prepared_web_article_native_fixtures` 执行真实原生准备。两者按需测试均使用 `TAILSYNC_WEB_ARTICLE_NATIVE_DIR` 指定临时输出目录。
3. `web_article_app.swift` 接收输出目录、helper 路径、`tailsync-probe.*` 接收板名。输出目录中放置自有测试 `article.html`。选择正文并等待可见选区稳定后导出，保存 `browser-source.cna`；把接收板名写入 `target-board.txt`。
4. 平台按需测试 `prepare_browser_selection_for_native_consumer` 处理 WebKit 原件，再点击工具的接收按钮观察实际粘贴。
5. `web_article_native_test.py --helper … --consumer … --fixtures … --browser-board-file …` 对两种构造文稿与浏览器输出逐一断言文字、图片数量、位置、颜色及重复资源。

可选公网测试 `first_publish_public_network_images` 使用 `TAILSYNC_PUBLIC_IMAGE_URLS` JSON 数组提供自选公开图片地址。普通离线回归不隐式访问公网。
