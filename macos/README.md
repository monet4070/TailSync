# TailSync macos

复合剪贴板的版本分别管理：identity/base **4**；data frame **5**（认证连接可协商回退到 4）；main history **条件 v11/v12/v13**；effect journal **1**；local contract **2**；helper protocol **macOS 4 / Windows 2**；origin fingerprint **便携 2 / macOS 原件独立域 1**。v11→v12/v13 受构建开关、运行时开关和 helper 健康控制；已有 v12/v13 在关闭态可读取，迁移目的库保持至少来源版本。v13 增加历史身份及 Manifest 2；旧 CNS1/helper protocol 2 只提供 LegacyProjection；macOS CNA1/protocol 4 分开原件与派生表示，完整枚举且依赖已知闭合的原件才提供安全去重键。pre-v12 与 pre-v13 回退包分别保留。版本变更须有独立决策，不能把这些编号统称为“协议版本”。

本地修复与设备验收分开记录。当前证据见 [R6 核验](../docs/COMPOSITE-CLIPBOARD-R6-VERIFICATION.md)，长期行为见 [复合剪贴板规格](../docs/features/composite-clipboard.md)。

新的候选目录通过 `TAILSYNC_APP_BUNDLE=/absolute/new-run/TailSync.app /bin/bash macos/build-mac.sh` 指定。`TAILSYNC_COMPOSITE_CLIPBOARD=0/1` 控制构建开关，`TAILSYNC_MACOS_TARGET=native/universal-apple-darwin` 分别验证架构。最低系统为 macOS 13.0；签名、每个 Mach-O slice 与包内 helper 的 self-check 必须通过。

`Contents/MacOS/tailsyncd --tailsync-clipboard-helper-check` 使用生产 resolver，检查同目录 helper 协议 4 后退出，不初始化存储、网络或剪贴板。debug 通过 `scripts/build-composite-debug.sh` 单独构建并核验；release 只接受同目录 helper，不搜索 PATH。关闭态 3 项测试涵盖协商和条件迁移。

完整 App、目标 App、双设备属于 R7。启动候选前确认 TCP 19890 空闲及包 hash，使用隔离数据；保留正在运行的实例和现有包。私有 `tailsync-probe.*` board 故障探针不能替代完整 GUI 验收。
