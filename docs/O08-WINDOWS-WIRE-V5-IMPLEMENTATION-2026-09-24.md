# O08 跨平台实现与验收边界

日期：2026-09-24。该记录描述源码与本机回归，不代表发行包或真实双设备验收通过。

## 已实现

- Windows 74 个注册的 Tauri 命令中，普通命令使用 `StableErrorEnvelope`，二进制预览保留带固定错误码、固定公开文案和大小字段的 `PreviewErrorInfo`；`supports_stable_errors=true`。TypeScript 入口解码通用错误包、按中英文消息键呈现；无法分类的源错误安全映射为 `internal_error`。旧文本错误仍能由客户端兼容处理。
- macOS 45 个注册的 Tauri 命令已按相同策略统一错误边界，Tauri 与 Unix socket 的 `supports_stable_errors` 均为 `true`。Swift 普通 JSON 命令请求 v1 错误包，并通过生成的契约解码、按中英文消息键呈现；历史预览保留专用错误码路径。旧 daemon 返回的文本错误仍可兼容。
- 握手与配对继续使用 v4 帧，并在已认证的握手载荷中声明最高 wire 版本。缺少声明的旧设备选择 v4；双方声明 v5 才使用 v5 数据帧。能力按每次连接协商，断线后重新协商。
- Windows 与 macOS 默认构建均启用 `protocol-file-sliding-window` 与 `protocol-image-compressed-chunks`。运行前设置 `TAILSYNC_DISABLE_FILE_WINDOW` 或 `TAILSYNC_DISABLE_IMAGE_CHUNKS` 可分别禁止宣告；连接只有在双方均宣告后才启用相应传输路径。
- 文件以最多 4 个 1 MiB 分片组成一个滑窗，发送端一次写入后读取累计偏移 ACK。超时后保留已确认偏移，在新连接上通过 FileMeta 恢复未确认后缀；未协商滑窗的会话按原有逐片确认传输。
- 图片使用 zlib 压缩后的最多 64 个 512 KiB 分片。每片有消息 ID、时间戳、序号、总长度和原图摘要；接收端限制压缩与解压大小、分片数量、RGBA 尺寸及顺序，重组通过后才写剪贴板与历史。无压缩收益或未协商时回退原始 RGBA 事件。解码失败关闭当前会话的分片接收能力，并发送固定的 `invalid_image_chunk` 协议错误。

## 本机证据

- 验证脚本 `audit/stable-error-policy-2026-09-23/validate.ps1` 覆盖 Core、Runtime、Themes、Windows/macOS Rust test 与 clippy、Windows 前端 test/build/lint、本地 schema 契约和跨平台检查；日志及退出码保存在 `audit/o08-windows-2026-09-23/`。
- 本次 macOS 命令迁移后，macOS Rust 库 80 项测试通过、`cargo clippy --all-targets -- -D warnings` 通过；本地契约检查确认 Windows 74 个与 macOS 45 个注册命令均已分类，跨平台契约检查通过。Swift 工具链在本 Windows 主机不可用，因此 Swift 新增测试尚未实际运行。
- 后续针对预览错误和输入错误的修复固定了预览对外文案，修正 Windows 设置 JSON 与远程配对链接的参数错误分类，并将 socket 旧文本分类收紧为固定消息/明确前缀匹配，未知情况回退 `internal_error`。
- 修复后共享 Core 在默认与两项扩展均开启的构建下各有 403 项测试通过（各 1 项既有忽略）；Runtime 22 项、Windows Rust 100 项（1 项既有忽略）、macOS Rust 80 项、Windows 前端 230 项通过。两端 Clippy、前端构建与 lint、本地契约及跨平台检查通过。此处的 macOS Rust 结果来自 Windows 主机，仍需 macOS 原生构建与 Swift 测试。
- 共享 Core 测试覆盖 v4→v4、v4↔v5、v5→v5；滑窗与旧会话回退、部分 ACK 后超时恢复；图片压缩重组、伪造尺寸、解压炸弹、乱序、重复与摘要损坏。
- 4 MiB 同素材模拟 ACK 延迟：80 ms 时逐片等待 378 ms、滑窗 104 ms；150 ms 时逐片等待 639 ms、滑窗 168 ms。原始输出位于 `audit/o08-windows-2026-09-23/run-final-20260923/window-latency.log`。数据来自本机内存连接，仅证明实现减少确认轮次。

## 仍需外部验收

- 真实旧版 N-1 成品包与新版 Windows 成品包双向互通，覆盖文本、图片和文件、断线恢复及真实 LAN/Tailscale/Iroh 路径。
- 在真实 macOS 上运行 Swift 构建/测试及 Tauri 应用验收；Windows 主机上的 macOS Rust 编译不能替代该验收。
- 对同一签名产物执行干净安装、升级、长时间运行和多设备吞吐/内存检查。上述证据完成前，整体发行结论仍为 `NO-GO`。
