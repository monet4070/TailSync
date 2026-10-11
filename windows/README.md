# TailSync windows

复合剪贴板的版本分别管理：identity/base **4**；data frame **5**（认证连接可协商回退到 4）；main history **条件 v11/v12/v13**；effect journal **1**；local contract **2**；helper protocol **macOS 4 / Windows 2**；origin fingerprint **便携 2 / macOS 原件独立域 1**。v11→v12/v13 受构建开关、运行时开关和 helper 健康控制；已有 v12/v13 在关闭态可读取，迁移目的库保持至少来源版本。v13 增加历史身份及 Manifest 2；旧 CNS1/helper protocol 2 只提供 LegacyProjection；macOS CNA1/protocol 4 分开原件与派生表示，完整枚举且依赖已知闭合的原件才提供安全去重键。pre-v12 与 pre-v13 回退包分别保留。版本变更须有独立决策，不能把这些编号统称为“协议版本”。

本地修复与设备验收分开记录。当前证据见 [R6 核验](../docs/COMPOSITE-CLIPBOARD-R6-VERIFICATION.md)，长期行为见 [复合剪贴板规格](../docs/features/composite-clipboard.md)。

前端入口：`npm --prefix windows test`、`npm --prefix windows run build`、`npm --prefix windows run lint`。Rust 默认/开启态需要分别 test/clippy。Mac 上的源码 host 测试或 MSVC `cargo check --features portable-check` 不证明 Windows 链接与原生剪贴板行为。

Windows 本机使用 `windows/scripts/build-composite-helper.ps1` 和 `windows/scripts/package-windows.ps1 -CompositeClipboard -BuildDirectory <new> -OutputDirectory <new>`，默认态另跑一轮；正式门禁不传 SkipChecks/SkipSmokeTest。portable 和 NSIS installer 分别记录包路径、架构、系统要求、helper 协议 2、签名与 SHA256，并验证 dev/安装后的实际 resolver。release helper 位于可执行文件旁，debug 还可使用构建 sidecar 目录。

当前 macOS 工作环境未执行 Windows 链接、打包、安装或原生剪贴板；portable/installer 在 package-manifest.json 各自标 UNKNOWN。Windows、目标 App 与双设备未通过前，发布 NO-GO。
