# 通知隐私与网页图片修复（2026-10-23）

状态：N1a 与网页单图的源码修复、本地回归完成；macOS 验收包已替换至 `/Applications/TailSync.app`。**钥匙串授权、本地服务和网页图片历史复制已通过；通知、Windows 原生及双设备验收仍未完成**。尚未正式发布。后续复制与首屏配色修复见 [补充验收记录](browser-image-copy-and-startup-colors-2026-10-23.md)。

## 变更

- macOS 普通到达只使用来源设备和类型，不再把 `HistoryEntry.description` 放入系统通知；使用 `.passive`、无声音。runtime 问题使用固定本地化文案及 `.active`，其他状态更新使用 `.passive`。
- Windows 到达通知不再包含剪贴板正文或文件名；文件完成、剪贴板应用失败、传输失败、存储状态与迁移提示使用共享 Rust 本地化文案。原始诊断不进入这些系统通知。
- 先沿用现有发射路径，不引入通知 V2、预览开关、聚合预算或 `files_received` port 签名改造。原生投递失败明确记日志，不重试可能已被系统接受的请求，不撤销已经完成的传输或存储迁移。
- 网页单图同时有已保存 PNG 和一个未保存 HTML 图片引用，且没有正文/RTF 时，预览和缩略图选择独立 PNG；回溯已保存内容可以复制该 PNG，同时保留“网页引用尚未恢复”的部分结果。实时接收也采用同一 PNG 替代计划，成功后回 `StoredIncomplete`，重复投递不再写入。原件、绑定和历史身份不修改；ADR-006 记录 K6 的受限例外。
- 带正文、多图、RTF、已有 HTML 绑定或 inline 图片的内容不使用这条替代规则。保留“第一张图片缺失时不拿第二张顶替”的约束，并加强了独立 PNG 共存时的回归。
- macOS 的泛化 `payload_unavailable` 现在显示“不可用”，不再一律宣称“无法解密”。确有旧版解密错误文本时，兼容分类仍保留。

## 截图对应记录与根因

只读 SQLite 元数据确认：截图中的远端记录为本机历史 ID 56，UTC `2026-10-23T05:27:03`，本地时间 13:27:03；类型为 composite，逻辑大小 96,501 字节。记录包含一份 196 字节的 HTML 和一份 96,305 字节的 PNG 表示，PNG 由加密外部载荷保存。

这证明不是“记录里只有网页地址”。源码中 HTML 优先级高于 PNG，第一张 HTML 图片不可读取时不会选到独立 PNG；通用载荷错误又被 Swift 分类成解密错误。实时接收也会因原始内容不完整跳过系统写入。这些链路已用与该记录相同表示组合的合成用例复现；本轮未解密生产 HTML，因此不声称已确认其具体引用地址。修复选用真实已保存的独立表示，不把它冒充 HTML 节点的资源，也不修改下载器的公共网络限制。

修复中对截图记录的检查只查询了生产 SQLite 元数据；图片正向回归使用临时数据库和合成图片，不能把它当作截图记录已在新 App 中实际打开的证据。安装期间另外发现一个 `archive_sync` 集成测试进程请求了真实钥匙串，说明临时数据库本身不足以证明密钥隔离，具体修复见下方记录。后续按用户要求替换了运行包，并通过原生界面确认历史列表能够加载；没有手动改写用户数据库、重建历史或改写系统剪贴板。

## macOS 验收包安装记录

- 2026-10-23 14:14（Asia/Shanghai），正常退出旧版界面及其后台进程后，安装至 `/Applications/TailSync.app`，再从该路径启动。版本仍为 `2.3.2`，bundle identifier 保持 `com.tailsync.app`。
- SwiftUI 界面、Rust 后台和 clipboard helper 均由当前工作区重新构建；启用 `protocol-composite-clipboard`。相关 23 个修复文件的哈希与回归证据一致。
- 使用仓库 `macos/build-mac.sh` 打包，ARM64、macOS 13.0 deployment guard、三个可执行文件及整包的 ad hoc 签名验证通过。动态库依赖均指向系统库。
- 验收包和构建日志：`/Users/monet/Downloads/TailSync构建/2026-10-23-通知与网页图片/`；旧版完整包保留为其中的 `TailSync.previous.app`。构建、二进制哈希和替换记录保存在 `build-install-manifest.json`。
- 首次启动时后台采样确认主线程停在 Security 的 `SecItemCopyMatching`，API socket 尚未创建。系统日志确认用户在 14:34:21 对 `tailsyncd` 授予“始终允许”，随后连接页重试加载成功。工具不操作系统安全窗口，也不读取或转存密钥来绕过授权。
- 后续观察到整个 App 重新启动过一次；本轮检查时只有一组安装路径下的进程：界面 PID 93198、子后台 PID 93199。安装二进制哈希与验收包一致，API socket 存在，历史窗口能加载原有记录。不能将进程重新启动归因于后台 watchdog；本轮工具没有发起这次重启。
- 没有手动改写用户数据库、设置或剪贴板。此处证明安装、本地服务和历史列表加载，不能作为网页图片、通知或双设备验收通过的证据。

### 连续钥匙串弹窗的补充诊断

系统 `securityd` 日志确认，主程序获准后，14:34:30–14:34:35 的额外请求来自 PID 92526 的 `target/debug/deps/archive_sync-34405f0b6a24df9e` 集成测试。该进程现已退出。此前把后续弹窗全部归因于新版 App 授权并不完整。

`test-support` 的进程识别原先只覆盖 `tailsync_lib` 和 `tailsync_runtime` 库测试名，漏掉 Cargo 集成测试。因此即使测试数据库在临时目录，依赖库的加密路径仍会请求 OS 钥匙串。修复将 Core / Runtime 现有集成测试目标纳入明确名单，同时保留 `deps` 目录与 Cargo 哈希约束。正常 App 和 `--all-features` 的产品可执行文件仍不使用测试密钥。新增回归从实际测试源目录枚举目标，避免以后新增测试时再次遗漏。

这项修复只改变 `test-support` 的测试选择，不需要再次替换当前验收 App，不修改生产钥匙串的访问控制。两项测试目标识别回归和四项 `archive_sync` 集成测试通过，格式检查及 diff-check 通过。验证日志单独记录于 `artifacts/keychain-isolation/2026-10-23/`。

## 边界处理

- 通知到 OS 是外部展示边界：入口只接收类型、来源、计数或有限错误类别，不接收剪贴板描述、原始异常与路径。
- 网页图像选择是内部业务规则，输入沿用现有 bundle 解码/校验契约，没有重复校验类型或格式。HTML 解析错误仍按类型化错误传播。
- blob/file/内网引用继续拒绝下载；没有吞掉拒绝或报告完整归档成功。已保存图片的替代回溯使用现有 `AvailableContent` / `missing_images` 结果。
- OS 投递失败有日志；它发生在业务提交之后，因此不会被报告成文件接收或存储迁移失败。新增边界处理均在代码旁说明原因。

## 验证结果

| 检查 | 结果 |
|---|---|
| Core 全量 lib tests | 986 通过，3 项原有 ignored |
| Runtime 全量 lib tests | 84 通过；新增通知文案在最终修改后再次定向验证 |
| `o06_restore_plan` | 15 通过，含 macOS / Windows PNG 回溯、无 HTML 发布及部分结果断言，带正文仍保留正文 |
| 实时 PNG 替代接收回归 | 通过，受控发布一次、重复事件不再写、仅发布 PNG、原始历史保留，返回不完整终态 |
| 加强的第一图缺失回归 | 通过，第二图与独立 PNG 均不能替代缺失的第一图 |
| Swift `AppBehaviorTests` | 29 通过，含脱敏消息与 passive/active |
| Swift Preview model / view-model | 31 通过，含通用错误分类和重试流程 |
| Core / Runtime clippy `--all-targets -- -D warnings` | 通过 |
| macOS Rust composite-enabled all-targets check | 通过 |
| Windows Rust composite-enabled host library check | 通过；不能代替 Windows 条件代码的原生编译 |
| Rust 格式检查、跨平台漂移检查、diff-check | 通过 |
| Windows host all-targets（默认及 composite-enabled） | **复查通过**；初次失败由这批未提交改动新增的 macOS 归档测试误置共享 include 引入，已移入 macOS 私有测试文件；此前“既有阻断”的归因有误 |
| Windows host 测试程序编译 / composite capture | 默认配置 `test --lib --no-run` 通过；composite-enabled 捕获测试 30 项通过 |
| macOS composite capture（测试拆分后） | 31 项通过，包含两条 QQ 和一条 WeChat 原生归档用例 |
| 实际 OS 横幅、通知中心、Windows 原生、双设备与生产记录复测 | 未执行 |

验证日志见 `artifacts/notification-web-image/2026-10-23/`。没有把初始失败的 SDK 构建或“没有匹配到测试”的 SwiftPM 输出算成通过。

### Windows 测试边界与归因更正

对比 HEAD 确认，三条新增 QQ / WeChat 用例依赖 macOS 的 CNA1 归档、`public.*` 格式及 `CaptureEnvironment.read_archive`。它们被放入两端都会包含的 canonical 测试文件，导致 Windows 工程产生 7 个编译错误。这是当前未提交 diff 引入的问题，先前文档和结果清单中的“既有阻断”归因不成立。

三条用例及其辅助函数现位于 `macos/src-tauri/tests/support/clipboard_capture_archives.rs`，仅由 macOS 工程的测试 fixture 包含。共享测试保留两端共用行为，没有给 Windows 添加仅为编译这些 macOS fixture 才需要的依赖或字段。没有使用单纯的 `target_os = "macos"` 门控，因为 Windows 工程也在 macOS 上做 host 编译。

修复后，`scripts/check-windows-host.sh`（格式、所有目标检查、测试程序编译）、Windows composite-enabled 所有目标检查及 30 项捕获测试通过；macOS 的 31 项捕获测试、canonical include 与跨平台契约检查通过。host 编译仍有该配置下的未使用符号警告，不能代替 Windows 原生 `clippy -D warnings` 和运行验收。

新证据保存在 `artifacts/windows-capture-test-boundary/2026-10-23/`。原始失败日志保留；原 `results.json` 已更正归因，更正前的结果快照另行保存，没有把原先的失败改写成当时通过。

### Swift 工具链说明

本机默认 SDK 27 的命令行构建缺少 SwiftUI 宏插件；使用已安装的 SDK 26.5 后源码编译成功。命令行工具目录也没有完整 XCTest 运行环境，因此显式使用已安装 Xcode 的框架、支持库与 `xctest` 执行已构建测试包，全程通过 `run-isolated-tests.mjs` 设置私有测试目录，没有接受许可协议或修改系统工具链配置。

构建测试包的关键参数：

```text
DEVELOPER_DIR=/Library/Developer/CommandLineTools
swift test --package-path macos/swift-ui --build-system native
  --sdk /Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
  -Xswiftc -F -Xswiftc <Xcode MacOSX.platform/Developer/Library/Frameworks>
  -Xswiftc -I -Xswiftc <Xcode MacOSX.platform/Developer/usr/lib>
  -Xlinker -F -Xlinker <same Frameworks>
  -Xlinker -L -Xlinker <same usr/lib>
  -Xlinker -rpath -Xlinker <same Frameworks>
  -Xlinker -rpath -Xlinker <same usr/lib>
```

实际测试使用：

```text
node scripts/run-isolated-tests.mjs -- <Xcode Developer/usr/bin/xctest>
  -XCTest TailSyncTests.AppBehaviorTests
  macos/swift-ui/.build/arm64-apple-macosx/debug/TailSyncPackageTests.xctest

node scripts/run-isolated-tests.mjs -- <Xcode Developer/usr/bin/xctest>
  -XCTest TailSyncTests.HistoryPreviewModelTests,TailSyncTests.HistoryPreviewViewModelTests
  macos/swift-ui/.build/arm64-apple-macosx/debug/TailSyncPackageTests.xctest
```

`<Xcode ...>` 指本机 `/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/...` 的对应绝对路径；`xctest` 为 `/Applications/Xcode.app/Contents/Developer/usr/bin/xctest`。这是验证环境记录，不是产品启动依赖。
