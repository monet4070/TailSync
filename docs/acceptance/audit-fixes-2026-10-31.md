# 审查修复计划与验证记录（2026-10-31）

基线：`620a963e683eb2bfffff5fb9cc7a00963b1fb6ff` 加本轮开始时已有的工作区修改及新增文件。保留这些修改，在当前工作区增量修复，不执行重置或清理。

## 已校准的事实与证据边界

- 网络原图容器为 `NIO1`，增量同步为 `ARU1`；不涉及新增 `ClipboardPayload` 尾部字段。原审查关于该尾部字段向后兼容的结论撤回，本轮不修改线协议或数据库版本。
- 演示 DOCX 为 `8000000011`，PPTX 为 `8000000012`；预览步骤标识 `history-7` 对应内容版本 3 的第 9 步。DOCX 已有前端渲染路径，补齐 PPTX 的结构化文档路径。
- 安全漏洞与 macOS 数据隔离原为静态路径确认；归档预算已有隔离复现，Windows 残留演示 ID 已有组件模拟复现。组件、主机测试与原生双设备验收分别记录，不混称端到端通过。
- 本轮是授权代码修复，不再声称只读。此前已跟踪 diff 摘要与保存副本一致，171 个保留的未跟踪文件内容一致；没有初始副本的 `artifacts/` 不属于完整只读证明范围。

## 执行顺序与验收

1. **网络图片可用性（用户确认优先）**：保留 Fake-IP 的系统代理兼容分支，DNS 命中 `198.18.0.0/15` 时通过域名获取，不主动降级为缺图。普通解析结果仍校验后固定 IP 直连。两条路径共用 URL、重定向、响应格式、时限和资源预算约束。此项按用户后续明确的产品取舍调整，不再将阻止 Fake-IP 作为验收条件，也不宣称代理分支具备公网 IP 绑定保证。
2. **macOS 引导隔离（高优先级）**：进入和退出引导都使旧查询失效、清除旧列表/选择/预览并重新加载。引导中阻断真实 ID 的写操作；演示 ID 在结束后仍不得进入后端。验证已加载真实历史时启动引导，以及未完成查询晚返回的情况。
3. **累计归档预算（P2，核心功能）**：归档器接收已有网络原图，独自计算剩余额度，调用方不预减 `ArchiveLimits`。原始字节按每条保留的 `NetworkImageOriginal` 计费；像素和派生 PNG 字节按图片引用出现次数计费，基础 PNG 表示和未引用资源至少计一次；协议的去重资源内存上限独立保留。增量推导采用同一引用记账规则。超限图片报告明确失败，保留本轮此前成功图片，数据库保留最终提交校验。回归重复大图、跨阶段原图字节、已有图片不重复扣减、部分成功入库和同步推导。
4. **Windows 引导生命周期（P2）**：历史/设置监听对应跨窗口 storage 事件；变更时清理列表、查询与预览。演示 ID 无条件隔离；引导中真实 ID 不执行历史写操作。演示预览在结束引导后清理关闭。验证跨窗口开始/结束及残留 ID 的恢复/删除/收藏保护。
5. **Windows PPTX 演示预览（P2）**：复用共享演示文档与现有文档预览契约，避免请求真实数据库；保留 DOCX 独立渲染路径。验证 PPTX 结构化载荷、会话身份和引导结束时的资源释放。

## 验证方式与进度

测试通过 `scripts/run-isolated-tests.mjs` 隔离应用数据目录，不使用日常历史、设置或剪贴板。按改动执行 Core/Runtime、SwiftUI、Windows 前端定向回归，再执行相关格式、类型、构建和跨平台契约检查。Windows 原生 GUI、代理实机和双设备验收如本机无法执行，明确列为尚未验证。

- [x] 网络图片可用性调整与回归
- [x] macOS 引导隔离与回归
- [x] 累计归档预算与持久化/同步回归
- [x] Windows 跨窗口生命周期与回归
- [x] PPTX 演示文档预览与回归
- [x] 相关门禁及最终结果记录

## 执行结果

| 范围 | 结果 | 关键证据 |
|---|---|---|
| Core 全量单元及默认集成测试（初轮） | 单元 1007 通过、3 跳过；默认集成通过 | 初轮严格地址策略和真实固定连接夹具；重复 3MP 图片按引用计费；已有像素只扣一次；已有派生字节不重置；原图字节超限保留此前成功结果；同步推导使用已有资源额度。Fake-IP 策略已按下文后续调整取代 |
| Runtime 全量单元及默认集成测试 | 合计 142 通过，2 项原有可选用例跳过 | 实际 SQLite 归档重试保持有效部分结果；约 18 MiB 累计 JPEG 原图在合并前返回 `archive_total_budget`，已有原图逐字节不变 |
| Windows 前端全量 | 63 个测试文件、522 项通过 | 模拟 Settings 跨窗口结束引导，历史恢复真实列表；通知到达前残留演示行的恢复/删除也不调用后端；演示预览结束时清理关闭；PPTX 准备与释放不发文档/取消 IPC；DOCX 保持原渲染器 |
| macOS SwiftUI 定向集成 | 41 项通过 | 真实 `NSHostingView` 加载真实记录后启动隔离引导，数字恢复/删除不调用注入后端；不配合取消的晚到查询不能替换演示列表；结束后恢复真实记录 |
| 构建与检查 | 通过 | Core/Runtime 全 target Clippy（`-D warnings`）、Rust fmt、Windows TypeScript/Vite 构建、前端 lint、共享引导生成漂移、跨平台契约与 `git diff --check` |

前端 lint 仍输出 Hook/ref 和 effect 状态更新警告，退出状态成功；Swift 测试包含既有测试辅助代码与环境访问警告，未将这些警告记作零警告通过。最后一处引导 storage-clear 兼容调整后，另行重跑该模块 17 项测试和前端构建/lint，均通过。

### 复现命令

```sh
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-core -p tailsync-runtime --quiet
cargo fmt --all -- --check
cargo clippy --locked -p tailsync-core -p tailsync-runtime --all-targets -- -D warnings
node scripts/run-isolated-tests.mjs -- swift test --package-path macos/swift-ui --filter 'HistoryKeyboardIntegrationTests|OnboardingTests'
npm --prefix windows test -- --reporter=dot --silent
npm --prefix windows run build
npm --prefix windows run lint
node shared/onboarding/generate-swift.mjs --check
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
git diff --check
```

### 尚未进行的目标环境验收

- Windows 原生 WebView2 跨窗口 storage 传播、焦点与关闭交互，以及双设备图文粘贴。
- 代理客户端组合与真实网络图片的端到端验证。普通直连路径保留原有公网策略和固定连接夹具；Fake-IP 路径按可用性优先的兼容策略处理。

## 后续调整：图片可用性优先

用户明确要求不因 Fake-IP 问题降低图片可用性，因此恢复原来的 Fake-IP 系统代理/系统网络兼容分支，取消此前对 Fake-IP 的主动缺图降级。URL 字面量的既有校验和普通公网地址固定连接保留；原图累计预算、macOS/Windows 引导隔离及 PPTX 预览修复保留。

代理分支不要求响应连接地址等于源站地址；两条分支仍共用 MIME、编码、体积、截止时间与取消检查。新增本地 HTTP 代理夹具，验证请求携带原始域名且能取得图片字节，无需修改系统代理或 DNS 配置。该夹具验证代理传输能力，不代替 Clash/Surge 等真实客户端验收。

调整后复验：Core 单元 1008 项通过、3 项原有用例跳过；Runtime `network_originals` 五项持久化/重试回归通过；Core/Runtime 全 target Clippy（`-D warnings`）、Rust fmt 和差异格式检查通过。引导与 PPTX 相关实现保持上一轮已验证的状态。

改动保留在工作区，未提交、未发布或替换用户已安装的应用。
