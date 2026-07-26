<div align="center">

<img src="assets/tailsync-icon.png" alt="TailSync" width="128" height="128">

# TailSync

### 让剪贴板跨设备自然流动

在 macOS 与 Windows 之间安全同步文本、图片和文件。支持局域网直连，也支持通过 Tailscale 跨网络连接。

[![Release](https://img.shields.io/github/v/release/monet4070/TailSync?display_name=tag&sort=semver)](https://github.com/monet4070/TailSync/releases/latest)
[![macOS](https://img.shields.io/badge/macOS-Apple%20Silicon-000000?logo=apple&logoColor=white)](#下载与安装)
[![Windows](https://img.shields.io/badge/Windows-x64-0078D4?logo=windows11&logoColor=white)](#下载与安装)
[![Tailscale](https://img.shields.io/badge/Network-Tailscale-242424?logo=tailscale&logoColor=white)](#使用-tailscale-跨网络连接)
[![License](https://img.shields.io/badge/License-MIT-22C55E)](LICENSE)

[下载 TailSync](https://github.com/monet4070/TailSync/releases/latest) · [Tailscale 官网](https://tailscale.com/) · [安装 Tailscale](https://tailscale.com/download)

</div>

## TailSync 能做什么

- 在 Mac 和 Windows 之间双向同步文本、图片与文件
- 自动保存本地剪贴板历史，可搜索、恢复和删除
- 同一局域网内直接连接，不经过 TailSync 云端
- 通过 Tailscale 在不同 Wi-Fi、不同城市或远程办公网络之间同步
- 使用限时配对、六位验证码、固定设备身份和 Noise 加密会话
- 主动检查对端是否真的在线，避免 mDNS 缓存造成“假在线”

## 下载与安装

前往 [最新 Release](https://github.com/monet4070/TailSync/releases/latest) 下载对应安装包。

| 平台 | 安装包 | 当前支持 |
|---|---|---|
| macOS | [`TailSync-2.0.0-macOS-arm64.dmg`](https://github.com/monet4070/TailSync/releases/download/v2.0.0/TailSync-2.0.0-macOS-arm64.dmg) | Apple Silicon（M1 / M2 / M3 / M4） |
| Windows | [`TailSync_2.0.0_x64-setup.exe`](https://github.com/monet4070/TailSync/releases/download/v2.0.0/TailSync_2.0.0_x64-setup.exe) | 64 位 Windows |

### macOS

1. 打开下载的 DMG。
2. 将 `TailSync.app` 拖入 `Applications` 文件夹。
3. 从“应用程序”启动 TailSync；启动后可在菜单栏找到图标。

当前 DMG 使用 ad-hoc 签名且未经过 Apple 公证。若 macOS 阻止首次启动：

1. 在 Finder 中右键 TailSync，选择“打开”；或
2. 打开“系统设置 → 隐私与安全性”，在安全提示旁点击“仍要打开”。

只应从本仓库的 Release 页面下载安装包。

### Windows

1. 运行下载的 `TailSync_2.0.0_x64-setup.exe`。
2. 根据安装向导完成安装并启动 TailSync。
3. 启动后可在系统托盘找到 TailSync。

如果 Microsoft Defender SmartScreen 提示未知发布者，请先确认文件来自本仓库 Release，再选择“更多信息 → 仍要运行”。

## 三分钟开始使用

### 1. 启动两台设备

在 Mac 和 Windows 上都启动 TailSync。Mac 端常驻菜单栏，Windows 端常驻系统托盘。

### 2. 选择连接方式

打开“设置 → 连接与设备”，在两端选择相同或兼容的连接方式。

| 模式 | 适用场景 | 行为 |
|---|---|---|
| **自动**（推荐） | 日常使用 | 同时检查 LAN 与 Tailscale，优先使用可达的局域网链路，必要时切换到 Tailscale |
| **局域网** | 两台设备在同一个可互通的局域网 | 只使用 LAN，不读取 Tailscale 设备 |
| **Tailscale** | 设备不在同一网络，或 LAN 被隔离 | 只使用 Tailscale 地址连接 |

### 3. 配对设备

1. 在两台设备上都点击“允许配对”，打开 120 秒配对窗口。
2. 在任意一端的设备列表中选择对端并发起配对。
3. 检查两端显示的六位验证码和设备指纹是否一致。
4. 两端分别确认。只有双方都确认后，设备才会被保存为可信设备。

如果窗口超时，重新点击“允许配对”即可。旧版 v1 与 v2 不兼容，升级到 v2 后需要重新配对。

### 4. 开始同步

配对完成后，在一台设备上复制文本、图片或文件，另一台设备会自动收到内容。历史页面可以搜索、重新复制或删除过去的记录。

## 使用 Tailscale 跨网络连接

[Tailscale](https://tailscale.com/) 会为你的设备建立一个私有加密网络（Tailnet）。两台设备不需要连接同一个 Wi-Fi，也不需要配置公网 IP、端口转发或路由器映射。

典型场景包括：

- Mac 在家、Windows 在办公室
- 一台设备连接有线网络，另一台连接不同网段的 Wi-Fi
- 酒店、校园网或公司网络禁止局域网设备互相发现
- 两台设备分别位于 `192.168.31.x` 和 `192.168.1.x` 等不同子网

### 第一步：安装 Tailscale

在 Mac 和 Windows 上分别安装 Tailscale：

- [Tailscale 官网](https://tailscale.com/)
- [官方下载页面](https://tailscale.com/download)
- [官方安装文档](https://tailscale.com/kb/1017/install)
- [Tailscale CLI 文档](https://tailscale.com/kb/1080/cli)

> [!IMPORTANT]
> TailSync 需要通过 Tailscale 命令行读取本机和对端地址。Mac 端请确认终端中的 `tailscale status` 可以正常执行；当前会检查 `/usr/local/bin/tailscale` 和 `/opt/homebrew/bin/tailscale`。Windows 端会自动查找 Tailscale 的标准安装目录。

### 第二步：登录同一个 Tailnet

在两台设备上使用同一个 Tailscale 账号登录，或确保两个账号属于同一个 Tailnet。可以在 [Tailscale 设备管理页面](https://login.tailscale.com/admin/machines) 检查两台设备是否都显示为在线。

Tailscale 通常会为每台设备分配一个 `100.x.x.x` 地址。这个地址与家庭或公司局域网中的 `192.168.x.x` 地址互相独立。

### 第三步：先检查 Tailscale 连通性

macOS 终端：

```bash
tailscale status
tailscale ip -4
tailscale ping <对端设备名或对端的 100.x.x.x 地址>
```

Windows PowerShell：

```powershell
tailscale status
tailscale ip -4
tailscale ping <对端设备名或对端的 100.x.x.x 地址>
```

如果 PowerShell 找不到 `tailscale` 命令，可以使用：

```powershell
& "$env:ProgramFiles\Tailscale\tailscale.exe" status
```

`tailscale ping` 成功后，再继续配置 TailSync。若它失败，应先解决 Tailscale 登录、Tailnet 或 ACL 问题。

### 第四步：在 TailSync 中启用 Tailscale

1. 保持两端的 Tailscale 客户端正在运行并已登录。
2. 启动两端 TailSync。
3. 打开“设置 → 连接与设备”。
4. 日常使用选择“自动”；需要排除 LAN 干扰时，两端都选择“Tailscale”。
5. 两端点击“允许配对”，然后按正常流程完成配对。

TailSync 会读取 `tailscale status` 提供的设备和地址，再向对端的 UDP `19889` 发送应用级心跳。Tailscale 显示 `Online` 只表示设备加入了 Tailnet；只有 TailSync 心跳或已认证连接有效时，TailSync 才会把对端视为真正在线。

### 第五步：自定义 ACL 时放行端口

Tailscale 默认策略通常允许同一 Tailnet 内的设备互通。如果你配置了自定义访问控制规则，需要允许两台设备之间访问：

| 端口 | 协议 | 用途 |
|---|---|---|
| `19889` | UDP | 设备发现与健康心跳 |
| `19890` | TCP | 配对、认证和剪贴板数据传输 |

参考 [Tailscale ACL 官方文档](https://tailscale.com/kb/1018/acls)。Windows 防火墙或第三方安全软件也必须允许 TailSync 使用这两个端口。

## 日常使用建议

- **一般情况选择“自动”**：在家或办公室优先走 LAN，离开局域网后仍可通过 Tailscale 连接。
- **跨网段时直接使用 Tailscale**：mDNS 通常不会跨越路由器，不必等待局域网发现。
- **两端都要运行 TailSync**：Tailscale 在线不代表 TailSync 应用正在运行。
- **首次同步前先完成配对**：发现设备并不代表已经建立信任。
- **敏感设备及时撤销配对**：可在设备列表中删除或撤销已信任设备。

## 设备状态说明

TailSync 的后台健康监控每 5 秒执行一轮探测：

| 状态 | 含义 |
|---|---|
| `已发现` | LAN 或 Tailscale 提供了设备地址，但尚未收到有效响应 |
| `正在确认` | 第一次健康检查失败，等待下一轮确认 |
| `在线` | 最近收到了 UDP 或 TCP 响应 |
| `已连接` | 当前存在通过 Noise 认证的会话 |
| `离线` | 连续两轮探测失败，且不存在认证会话 |

正常情况下，对端关闭 TailSync 或断网后约 **8–12 秒**会显示离线。

## 常见问题

### 两台设备在同一个路由器下，为什么发现不了？

确认两台设备位于同一子网，并检查路由器是否启用了 AP 隔离、访客网络隔离或 VLAN 隔离。例如 `192.168.31.247/24` 和 `192.168.1.2/24` 默认不属于同一子网，mDNS 通常无法直接跨网段。最简单的解决方式是两端安装 Tailscale 并选择“自动”或“Tailscale”。

### Tailscale 显示在线，但 TailSync 找不到对端

依次检查：

1. 两台设备是否登录同一个 Tailnet。
2. 两端 TailSync 是否都正在运行。
3. `tailscale ping <对端>` 是否成功。
4. 两端 TailSync 是否选择了“自动”或“Tailscale”。
5. 自定义 ACL、Windows 防火墙或安全软件是否拦截 UDP `19889` / TCP `19890`。
6. Tailscale 是否已安装在标准位置，命令行执行 `tailscale status` 是否正常。

### 设备已经出现，但无法同步

“已发现”不等于“已配对”。请打开两端配对窗口，核对验证码和指纹，并在两端分别确认。已配对但连接失败时，可点击“测试连接”查看当前 LAN 或 Tailscale 路由是否可达。

### Windows 从 Wi-Fi 换成有线网络后发现不了

有线网络可能分配了不同子网地址，或者 Windows 将新网络识别为“公用网络”并应用更严格的防火墙规则。检查网络配置和防火墙，或直接使用 Tailscale，避免依赖局域网广播和 mDNS。

### macOS 提示应用已损坏或无法验证开发者

确认 DMG 来自本仓库 Release，然后尝试右键应用选择“打开”，或前往“系统设置 → 隐私与安全性 → 仍要打开”。当前公开包未做 Apple Developer ID 签名与公证。

## 安全与隐私

- TailSync 没有用于转发剪贴板内容的云端服务。
- 每台设备拥有持久化的 X25519 身份密钥。
- 配对使用 120 秒窗口、六位验证码和双方确认。
- 后续连接通过 Noise XX 握手校验已配对身份，并使用加密会话传输。
- 使用 Tailscale 时，通信还受到 Tailnet 加密与访问控制保护。
- 文本历史和图片历史加密存储；文件历史目前保存原始文件字节，建议启用 FileVault 或 BitLocker。

## 数据保存位置

macOS 默认位于：

```text
~/Library/Application Support/com.tailsync.TailSync/
```

主要内容包括：

| 路径 | 内容 |
|---|---|
| `history-v2.db` | 历史元数据、加密文本和内容引用 |
| `image-history/` | 加密图片历史 |
| `file-history/` | 文件历史副本 |
| `incoming/` | 正在接收的临时文件 |
| `config-v2.json` | 设置、可信设备公钥和已知地址 |
| `identity-v1.bin` | 本机固定设备身份 |

Windows 使用系统应用数据目录保存同类数据。更新或重新安装应用本体不会主动删除历史、身份和配对信息。

## 从源码运行

### macOS

需要 Node.js、Rust、Swift 5.9+ 和 Xcode Command Line Tools。

```bash
cd macos
xcode-select --install
npm ci
./dev.sh
```

构建应用和 DMG：

```bash
./build-mac.sh
./build-dmg.sh
```

### Windows

需要 Node.js、Rust 和 Visual Studio Build Tools，并安装“使用 C++ 的桌面开发”工作负载。

```powershell
cd windows
npm ci
npm run tauri:dev
npm run tauri:build:win
```

## 当前限制

- 当前 macOS Release 仅提供 Apple Silicon 版本。
- macOS 安装包为 ad-hoc 签名且未经过 Apple 公证。
- Windows 安装包尚未使用公开代码签名证书。
- 文件支持应用运行期间断线续传，但应用重启后不会继续未完成传输。
- Android 客户端尚未纳入 v2 兼容范围。

## 许可证

TailSync 采用 [MIT License](LICENSE)。

---

<div align="center">

如果 TailSync 对你有帮助，欢迎点一个 ⭐

</div>
