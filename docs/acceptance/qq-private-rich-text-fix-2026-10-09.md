# QQ 私有富文本消息识别修复（2026-10-09）

## 真实记录与根因

只读定位用户报告的历史记录 3121。QQ 消息按顺序包含截图、截图说明和“依旧”、企鹅图片、图片说明。剪贴板提供 QQ 私有 binary plist、第一张图的 TIFF 和两个原生 item，没有标准 HTML、RTF 或纯文本。第二个 item 仅广告文件 URL；按现有策略延迟读取。

原转换器不解析 QQ 私有格式，多 item 的便携转换失败，导致记录 native-only。历史分类器又把多原生 item 统一判为 Unknown，即使已经解析出正文也无法显示为文档。资源提取会将内联图片 URL 替换为绑定资源，逐字节比对原始派生 HTML 还会错误地丢失来源证明。

## 修复

macOS helper 识别经实际记录确认的 QQ plist 类型，仅处理文字与图片元素，保留次序，原子保存 HTML 与纯文本派生对。未知类型、异常结构、数量和预算超限均拒绝语义转换并保留原件。

图片只从消息明确引用的 QQ 缓存路径读取；路径根及每个目录都用 openat/O_NOFOLLOW 检查，限制文件类型、所有者、大小和读前后状态，不扫描目录或下载网络资源。已捕获的 PNG 仅能作为第一张图的后备，缺失图片保留位置并如实标为不可用，避免重复第一张图或泄露缓存路径。

Core 只接受完整枚举、稳定原件、匹配来源和 converter 的完整派生对。其他 item 只允许延迟的 file-url 与控制格式；独立文本或已捕获的文件不能被折叠。原始 2 个 item 与字节仍完整保留。

Runtime 对 QQ 重建整个经过验证的便携投影，连同图片资源、绑定、文字和 completeness 与保存的 bundle 比较一次后建立派生来源证明。分类器仅对有此证明的选中 QQ HTML 放宽多 item 限制。分类器版本升至 1002，使旧缓存按新规则重建。

前一轮网络图片下载反馈修复包含在本次包中。

## 本地验证

- 新增历史列表回归：修复前 kind=unknown，修复后 kind=document；缩略图、中文摘要、两张图与原件不变均通过。错误 converter、其他来源及未证明的多 item 不会被标作文档。
- 来源回归：内联图片提取后仍可证明来源；替换图片、替换文字、不完整枚举均不能取得 QQ 来源证明。
- Core composite 串行 331/331；Runtime lib 和集成测试共 116/116；打包 helper 10/10；两 crate all-targets Clippy、workspace fmt、diff-check、11 个 canonical include 检查通过。
- 真实记录重放通过 Core、历史分页与文档分析接口。当前访问条件：保存 1 图、缺 1 图，显示 document；由 QQ“另存为”得到第二张图后重放：保存 2 图、缺 0 图，显示 document。文字和位置正确，原始 2 item 不变。
- 用真实 QQ plist/TIFF 在独立命名 pasteboard 重建原 item 结构，再经打包 helper 的 --read-archive 生产入口验证原子派生对。得到完整文字、1 张保存图、1 张缺图；一般剪贴板前后字节一致。

证据仅保存在本地受限目录 `artifacts/rich-text-repair/2026-10-09-multi-item-rich-text/`。测试探针归档为 .txt，不留临时 Cargo example。生产历史库未写入，运行中的旧 App 未终止。

## 交付与未完成项

候选 App：`/Users/monet/Downloads/TailSync构建/2026-10-09-qq-rich-text-01/TailSync.app`。arm64、macOS 13 最低版本、ad-hoc 签名、包内 helper/resolver、composite 功能和 helper protocol 4 均检查通过。详见 package-manifest.json 和 source-fingerprint.json。

识别与分类修复已本地完成。完整双图导入仍受实际 macOS QQ 容器访问拒绝限制：Swift URL 读取与普通文件读取均失败。QQ 自己另存出的第二张图可用于验证转换正确，但不赋予 TailSync 今后读取 QQ 缓存的权限。本轮未更改系统授权、未声称权限问题已修复。

当前运行的是前一轮 network-image-feedback 包；切换新 App 后须重新复制 QQ 消息，原历史记录没有新增的派生对，不会自动补出缺失内容。尚未进行切换后的 QQ→TailSync GUI 验收、真实双设备或 Windows 原生验收。
