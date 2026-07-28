# TailSync v2 项目状态

最后核对：2026-07-27

本文是当前项目进度的唯一状态说明。日期化测试报告、会话记忆、已完成的实施方案和 UI 样稿已移除，避免把历史快照误当成当前事实。

## 当前基线

- 后端版本：`2.0.0`
- 线协议：v2
- 目标平台：macOS、Windows
- macOS 形态：SwiftUI 菜单栏外壳 + Rust 守护进程
- Windows 形态：React/Tauri + 共享 Rust 后端
- 对端端口：TCP `19890`
- macOS 本地 API：`127.0.0.1:19889`
- 数据库 schema：v4

## 已实现

- 文本、图片和文件同步及历史记录
- LAN、Tailscale 和自动路由策略
- UDP、mDNS 和 Tailscale 候选发现，自动模式优先可用 LAN
- Noise XX 加密、固定 X25519 身份和已配对公钥校验
- 限时六位码配对、双向确认、失败锁闭、取消和撤销信任
- 文本/图片事件 ACK、重试、时间戳检查和消息 ID 重放抑制
- 1 MiB 文件块、Blake3 块校验、offset ACK 和连接中断续传
- 文件名清理、1 GiB 接收上限和入站连接数限制
- 文件历史内部哈希命名与剪贴板展示路径分离；接收文件保持原名，兼容旧版哈希前缀，并通过文件 shadow filter 阻止回环传输
- SQLite 历史元数据、加密文本、加密图片外部存储和文件外部存储
- React 和 SwiftUI 的历史、设置、设备管理、配对、主题与中英文 UI
- macOS 应用构建、ad-hoc 签名、Bonjour 权限检查和打包后启动验证脚本
- Windows/macOS 共享源码漂移检查和双向线协议探针
- macOS 休眠/唤醒恢复：刷新可靠事件时间戳、取消旧连接 worker、重置剪贴板监听并将监听活性纳入守护进程健康检查
- macOS 文件剪贴板辅助进程超时保护，避免异常 pasteboard 请求永久阻塞本地监听

## 当前限制与发布前事项

1. macOS 本地 JSON-lines API 只依赖 loopback 绑定，没有能力令牌，也没有明确的请求长度/读取超时；端口 `19889` 不应被转发或暴露。
2. 文件续传状态能覆盖应用运行期间的断线重连，但启动时会清理 `incoming/`，不能跨应用重启继续。
3. `file-history/` 中的文件历史副本按原始字节保存；文本和图片历史使用应用数据密钥加密。
4. `build-mac.sh` 仅执行 ad-hoc 签名。公开分发前仍需 Developer ID 签名、公证和真实升级路径验证。
5. Android 客户端不在当前代码库和已实现范围内。
6. 自动化检查通过后，仍应在真实 Mac/Windows 设备上完成双向剪贴板、路由切换、大文件中断续传和撤销配对验收。

## 2026-07-27 验证结果

| 检查 | 结果 |
|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | 73 通过，0 失败，1 个真实 Tailscale 环境测试忽略 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | 通过 |
| `npx tsc -b` | 通过 |
| `npm run lint` | 通过 |
| `npm run build` | 通过 |
| `swift build --package-path swift-ui` | 通过 |
| `node scripts/check_cross_platform_sync.mjs` | 未运行；本机缺少预期的 `/Users/monet/tailsync-v2-win` 项目目录 |

本次验证使用当前锁定依赖和仓库默认构建目录完成。跨平台源码漂移检查仍需在 Windows 项目目录可用后补跑。

## 文档维护规则

- 当前行为以 `README.md`、本文和 `CROSS_PLATFORM_SYNC.md` 为准。
- 阶段性测试结果应更新本文，不再新增无维护期限的根目录测试报告。
- 已完成的实施计划不保留为“待办”；仍未解决的风险应写入“当前限制与发布前事项”。
- 协议、端口、配对流程或跨平台共享文件变化时，必须同步更新跨平台契约并运行漂移检查。
