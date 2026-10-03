# 验收环境与待验分类

> 建立日期:2026-09-30。基线 `origin/main` = `b61a0a04b88f5828707f7ef64781389944471be8`。
> 目的:把“需要什么才能关闭”写清楚,避免待验项被读成遗漏。

## 当前环境约束

- 执行环境仅 macOS 单机,**无法进行任何跨设备实机验收**。
- 无 Windows 设备/虚拟机可用。
- 因此下列第 3 类条目在环境改变前**不可能关闭**;台账中它们保持 `待验`,并且**发布门禁保持关闭**(CI 全绿不等于验收通过)。

## 三类待验

### 第 1 类:单机可自动化(不需要设备,应做成门禁并据此关闭条目)

| 条目 | 需要的证据 | 可行性 |
|---|---|---|
| `S3-P1-2` 配对 pending/active | 冻结的旧版握手/持久化格式夹具,按旧→新、新→旧双向运行;故障注入覆盖每个持久化与发送边界;重启/重复确认 | ✅ 单机可做 |
| `S5-P2-2` WAL 忙时降级 | 双连接 + 持有中的读事务制造 `busy=1`,验证警告与删除成功 | ✅ 单机可做 |
| `S6-P0-1` 成品端口门禁 | Windows CI 启动打包后的 exe，断言未监听旧 API 端口；历史故障注入已报红，本次修改版本仍待 CI | ✅ 托管 CI 可做 |
| `S5-P2-1` 孤儿文件对账 GC | 提交成功但删除失败、进程重启、再次清理 | ✅ 单机可做 |
| `S4-P1-3` 配额扫描与锁等待 | 扫描次数、物理字节账目、并发双批准入、失败注入 | ✅ 单机可做 |
| `S6-P2-1` 通知 gap/实例标识 | 队列覆盖、断线重连、daemon 重启 | ✅ 单机可做 |
| `S6-P2-4` Windows 后台警告 | 消费/确认契约可用前端夹具本机测试，原生后台通知与窗口集成由 Windows CI/设备验证 | ⚠️ 按测试层次分别取证 |
| `S2-F5` / `S4-P1-4` 性能预算 | 单机 loopback 可采基线,但**不等同**双机真实网络 | ⚠️ 部分 |

### 第 2 类:单机手动(只需这台 Mac)

| 项目 | 需要的证据 |
|---|---|
| ~~VoiceOver 验收~~ | **已由项目所有者移出必验范围（2026-10-03）**。清单保留在 `docs/acceptance/voiceover-connections-checklist.md` 供将来需要时使用，但不再作为发布门禁项，也不阻塞任何条目关闭。 |
| macOS 托盘/安装包行为 | 本地构建 + 打包后的托盘与包清单核对（`tray-helper` 孤儿组件已在本轮移除：仓库内无任何内建或调用引用，其源码会用明文 TCP 连 19889，而 macOS 守护进程只监听 Unix socket；移除后 macOS Rust 与 Swift 构建通过）。**2026-10-03 结果**：包清单核对 PASS；打包后的 app 启动成功并在 WindowServer 注册为 `type="UIElement"`（版本 2.3.2、bundleID `com.tailsync.app`、Arch ARM64），退出后无残留进程。状态项**行为**已有 3 项自动化测试覆盖并全部通过（`testSettingsRedirectKeepsTheStatusItemWindowOpen`、`testStatusMenuOpensHistoryForAppKitDefinedLeftClick`、`testStatusItemLogoDoesNotMutateTheCachedThemeImage`）；**图标在菜单栏中的视觉呈现**未由自动化确认——需要辅助功能或屏幕录制权限，见下方限制。 |
| 键盘取用历史真机验收 | 热键唤起 → 首次 ↓ → 第 0 条选中框：**2026-10-03 已通过**，记录见 `docs/acceptance/keyboard-macos-native-2026-10-03.md`。T-01 及其余条目**仍未确认** |
| macOS 打包与发布门禁 | **2026-10-03 全部通过**：2.3.2 universal（x86_64+arm64）、minos 13.0、ad-hoc 签名、`check_macos_deployment.py` PASS、`hdiutil verify` VALID、SHA-256 已生成。包内容清单 8 个文件无孤儿组件；`plutil -lint`、`NSLocalNetworkUsageDescription`、`NSBonjourServices=_tailsync._tcp`、URL scheme `tailsync`、`codesign --verify --deep --strict` 全部 PASS；打包后的 `clipboard-helper` 读写往返 PASS；**`verify_macos_bundle.sh` 三阶段全部通过**（含 `[2/3]` 打包 daemon 启动与 `[3/3]` JSON-lines API 校验，输出 `macOS community bundle verification passed.`） |

> **工具链限制（2026-10-03 实测）**：`TAILSYNC_GUI_FOCUS_TESTS=1` 门控的原生焦点测试在 `swift test` 下**无法运行**——该进程不是 `.app` bundle，WindowServer 拒绝让其成为前台应用，窗口拿不到 key 状态，测试在自身前置断言处失败。它需要由真实 `.app` 承载的 GUI 测试宿主。因此"窗口是否真的拿到键盘"只能由真人验收回答，不能用该测试替代。

> **权限限制（2026-10-03 实测）**：自动化代理无法验证菜单栏状态项。`osascript` 查询 `System Events` 返回 `不允许辅助访问 (-1728/-1719)`，`screencapture` 返回 `could not create image from rect`——辅助功能与屏幕录制权限都需要用户手动授予。这类"菜单栏/托盘是否可见"的检查在获得授权前只能由人工完成。

### 第 3 类:硬件所限,当前不可达(环境改变前不关闭)

| 项目 | 为什么需要设备 |
|---|---|
| Windows 原生断网/虚拟网卡/VPN 混合场景（`S1-P1-5` 的 `native_acceptance`） | 需要真实 Windows 与物理网卡状态变化 |
| 双机 LAN/Tailscale 配对、双方重启、断线续传、网络切换、跨版本互通 | 需要两台真实设备 |
| Windows 原生设置窗口跨窗口不回退（PR #68 的验收项） | 需要 Windows 会话 |

## 结论与操作含义

1. **可以直接推进的**:第 1 类其余条目(阶段 4/5/6 的代码与门禁)与第 2 类的 macOS 部分。
2. **可以让条目真正关闭的**:第 1 类做成门禁之后。
3. **不能关闭的**:第 3 类。它们在台账里长期 `待验`,发布前必须由具备设备的人补做;`docs/remediation/PHASE-0-3-REPORT.md` 的“5. 阻塞项”与此一致。
4. 台账每条的 `native_acceptance` 字段就是第 2/3 类证据的落点;本文件是它们的环境说明。
