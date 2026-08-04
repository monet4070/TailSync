# TailSync v2

TailSync 是一款面向 macOS 和 Windows 的跨设备剪贴板同步应用。它支持文本、图片和文件，在局域网或 Tailscale 网络中发现已配对设备，并保留本地剪贴板历史。

当前后端版本为 `2.1.0`，线协议为 v2。项目进度、已知限制和最近验证结果见 [项目状态](docs/PROJECT_STATUS.md)。

## 当前功能

- 文本、图片和文件双向同步
- `auto`、`lan_only`、`tailscale_only` 三种连接策略
- UDP、mDNS/DNS-SD 和 Tailscale 设备发现
- Noise XX + 固定 X25519 设备身份的加密连接
- 120 秒配对窗口、六位验证码、双向确认和失败锁闭
- 文本/图片可靠事件 ACK、重试和网络重放抑制
- 1 MiB 文件块、Blake3 校验、offset ACK 和断线续传
- SQLite 历史记录、搜索、恢复、删除和数量限制
- 中英文、系统/浅色/深色主题、通知和设备启停

旧版明文协议 v1 不受支持，也不会自动降级。升级协议后需要在所有设备上重新配对。

## 架构

协议、加密、身份、配对、数据库和同步状态机位于仓库级共享 crate `../shared/rust-core/`。`src-tauri/src/` 只保留 macOS/Tauri 接入层，包括剪贴板、网络发现、连接管理和本地 API。

- macOS：SwiftUI 菜单栏外壳启动 Rust 守护进程，通过 `127.0.0.1:19889` 的 JSON-lines API 通信。
- Windows：React/Tauri UI 通过自己的平台接入层使用同一个 `tailsync-core` crate。
- 设备同步和配对使用 TCP `19890`。
- 局域网发现同时使用 `_tailsync._tcp.local.` 和 UDP 发现。

共享 crate 和协议边界见 [跨平台同步契约](docs/CROSS_PLATFORM_SYNC.md)。

## 开发环境

通用依赖：

- Rust 工具链（包含 Cargo）
- Tauri v2 所需的系统开发依赖

macOS 还需要 Swift 5.9 或更高版本和 Xcode Command Line Tools：

```bash
xcode-select --install
./dev.sh
```

`./dev.sh` 会构建并启动 SwiftUI 外壳、Rust 守护进程和剪贴板辅助程序。macOS 不再包含 React/Vite 前端，History 和 Settings 均由 SwiftUI 提供。

Windows 需要 Node.js、npm 和 Visual Studio Build Tools，并启用“使用 C++ 的桌面开发”工作负载。在 `../windows` 目录中执行：

```powershell
npm ci
npm run tauri:dev
```

如果 Windows 目录曾在不同操作系统间直接复制，请重新运行 `npm ci`，以安装当前平台对应的原生依赖。

## 配对设备

1. 在两台设备的“设置 → 连接与设备”中选择连接策略。
2. 两端都点击“允许配对”，打开 120 秒配对窗口。
3. 在设备列表中选择对端并发起配对。
4. 核对两端显示的六位验证码和设备指纹。
5. 两端分别确认；只有双方都确认后才会保存信任关系。

配对窗口默认关闭，连续五次失败、超时、取消或成功后都会自动关闭。应用不会自动信任首次出现的设备身份。

## 数据存储

应用数据由系统应用数据目录管理。macOS 通常位于：

```text
~/Library/Application Support/com.tailsync.TailSync/
```

主要内容：

- `history-v2.db`：历史元数据、加密文本和内容引用
- `image-history/`：加密的图片历史文件
- `file-history/`：使用 1 MiB 分块 AES-256-GCM 容器加密的文件历史副本
- `incoming/`：进行中的临时文件和续传状态
- `config-v2.json`：设置、信任公钥和已知地址
- `identity-v1.bin`：固定设备身份

进行中的文件可在网络断开后继续传输；应用重启会清理未完成的 `incoming/` 状态，因此当前不支持跨进程重启续传。

启动时会自动检测并导入旧版 TailSync v1 历史。导入失败的条目会记录到 `v1-migration-report.json` 并在旧数据库变化后重试；原 `history.db` 和 `.fernet_key` 始终保留。

## 构建

macOS 本地开发包：

```bash
./build-mac.sh
open TailSync.app
```

该脚本生成包含 SwiftUI 外壳、`tailsyncd` 和 `clipboard-helper` 的 `TailSync.app`，并进行 ad-hoc 签名验证。它不是用于公开分发的 Developer ID 签名或公证产物。

生成可上传到 GitHub Release 的 DMG：

```bash
./build-dmg.sh
```

产物写入 `release/TailSync-<version>-macOS-<arch>.dmg`，同时生成 SHA-256 校验文件。默认使用 ad-hoc 签名；正式发布可通过环境变量指定 Developer ID 和公证凭据：

```bash
TAILSYNC_CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
TAILSYNC_NOTARY_PROFILE="tailsync-notary" \
./build-dmg.sh
```

Windows 安装包应在 Windows 环境中构建：

```powershell
npm run tauri:build:win
```

## 验证

```bash
cargo fmt --manifest-path ../shared/rust-core/Cargo.toml --all -- --check
cargo test --locked --manifest-path ../shared/rust-core/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib
swift test --package-path swift-ui
node scripts/check_cross_platform_sync.mjs \
  --win-root ../windows \
  --mac-root . \
  --core-root ../shared/rust-core
```

如果项目位于会自动生成 `._*` AppleDouble 文件的非 APFS 卷上，Cargo/Tauri 可能把这些元数据误当成 UTF-8 配置。此时将构建目录放到本地临时卷：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  --target-dir /tmp/tailsync-target
```

macOS 完整发布验证：

```bash
bash scripts/verify_macos_release.sh /path/to/tailsync-v2-win
```

完整发布验证会运行共享 Rust core、macOS 接入层和 SwiftUI 检查，核对跨平台契约，构建并签名应用，启动打包产物，并检查 `19889`/`19890` 监听、本地 API 和文件剪贴板辅助程序。

## 文档

- [项目状态](docs/PROJECT_STATUS.md)
- [跨平台同步契约](docs/CROSS_PLATFORM_SYNC.md)
