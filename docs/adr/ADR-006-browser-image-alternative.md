# ADR-006：网页单图采用已保存的独立 PNG 表示

- 状态：已实施于当前工作区（2026-10-23）；本地验证完成，原生与双设备验收未完成。
- 范围：预览、手动回溯和实时 composite 接收的单图表示选择。
- 上游：[ADR-004](ADR-004-composite-clipboard-contracts.md)、[ADR-005](ADR-005-composite-content-identity.md)。

2026-10-29 后续：[ADR-007](ADR-007-web-article-sync.md) 扩展了网页图文的可用副本发布范围；本 ADR 对独立 PNG 不得猜测绑定到 HTML 节点的限制继续有效。

## 问题

浏览器复制图片可能同时提供独立 PNG 和含 `blob:` / `file:` 等引用的 HTML。PNG 已完整保存，HTML 的图片引用却不能跨设备读取。原实现优先读取 HTML 的第一张图片，导致预览不可用；实时接收又因原始 bundle 不完整而完全跳过剪贴板写入。现象可能进一步被 UI 误报成解密错误。

## 决策

输入满足以下条件时，将独立 PNG 作为可用的替代表示：存在 PNG、只有一个未绑定且非 inline 的 HTML 图片引用、没有正文或 RTF、没有其他不支持的 HTML 依赖，且 completeness 不是 MissingFormats。规则复用现有 HTML 解析和字节校验边界，不为内部每一步重复验证。

PNG 始终是独立表示，不是 HTML 节点的绑定证据。预览选用 PNG；手动回溯使用 `AvailableContent` 并报告一个未恢复的 HTML 图片引用；实时接收只发布 PNG，原件及历史仍保存整个原始 bundle。

这是对 ADR-004 K6“StoredIncomplete 时不执行 native effect”的**受限例外**：仅此种已验证的 PNG 替代计划可以发生一次受控系统写入。成功发布替代 PNG 后最终仍返回 `StoredIncomplete`，不得返回 `Applied` 冒充完整 HTML 恢复。该 ACK 已是发送端不可重试的终态，旧发送端会继续保守显示不完整提示，不需要新增 wire 值。

K1、K2、K4、K5、K7、K10 继续有效：WireDigest 与历史身份对应原始内容；先持久历史与 claim，再做副作用；generation、revision、授权和取消检查不变；副作用未知不重做；系统写入在 engine 锁外；完成回执按原路径持久化。新计划保留 NoEffect / Partial / Unknown / Superseded 的已有分支，不能把它们映射成成功。

## 边界与验证

- 不联网修复引用，不读取 file 路径，不解除 SSRF 限制，不把 PNG 注入任意 HTML 节点。
- 带正文、多图、RTF、已有 HTML 资源或 MissingFormats 的内容继续走原来的发布/保留规则。
- 正向回归覆盖预览、两端回溯输出、实时接收一次发布、重复事件不再发布、原始历史内容不变。
- 负向回归覆盖第一图缺失而第二图及独立 PNG 共存，以及带正文时不丢掉正文去选择 PNG。
- 结果和原生验收边界见[修复记录](../acceptance/notification-privacy-and-browser-image-2026-10-23.md)。
