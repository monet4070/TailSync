# TailSync v2

TailSync 是一款跨设备剪贴板同步应用，支持文本、图片和文件。应用运行在系统托盘中，可在局域网或 Tailscale 网络上连接多台设备，并保留本地剪贴板历史。

## 开发环境运行

在仓库根目录执行：

```powershell
npm install
npm run tauri:dev
```

应用启动后会常驻系统托盘。通过 TailSync 托盘菜单打开“历史记录”或“设置”窗口。`npm run dev` 只会启动前端预览，无法提供剪贴板监听、系统托盘或数据库访问能力。

运行前请确保已安装：

- Node.js 和 npm
- Rust 工具链（包含 Cargo）
- Tauri v2 所需的系统开发依赖

Windows 用户还需要安装 Visual Studio Build Tools，并启用“使用 C++ 的桌面开发”工作负载。macOS 用户需要安装 Xcode Command Line Tools。

## 设备连接与安全配对

TailSync 支持两种连接方式，可在“设置 → 连接与设备”中选择：

- **局域网**：发现并连接同一局域网中的设备。
- **Tailscale**：通过 Tailscale 网络连接设备。使用此模式前，请确认设备已登录同一个 Tailnet。

发送剪贴板数据前，需要在设备之间完成一次安全配对：

1. 在设备 A 的“设置 → 连接与设备”中点击“允许配对”。
2. 在设备 B 的设备列表中找到设备 A，点击“配对”。
3. 确认两台设备显示相同的六位验证码，并分别点击“验证码一致”。
4. 配对完成后，双方会自动保存对方的身份公钥，无需手动复制或粘贴长公钥。

传输协议 v2 使用 Noise XX、固定的 X25519 设备身份以及 ChaCha20-Poly1305 加密。旧版明文协议 v1 客户端会被拒绝；升级后请在所有参与同步的设备上重新完成配对。应用不会自动信任首次出现的公钥，也不会在认证失败时降级到明文连接。

## 历史文件存储

剪贴板历史文件存储在应用数据目录下的 `file-history` 文件夹中，而不是 SQLite 数据库内。数据库只保存历史元数据和带版本的内容哈希引用。

首次启动新版本时，旧版数据库中的加密文件 BLOB 会迁移到该文件夹。正在传输的文件会写入独立的 `incoming` 文件夹，应用启动时会清理其中过期的临时文件。

## 构建 Windows 安装包

```powershell
npm run tauri:build:win
```

构建产物会根据 Tauri 配置输出到 `src-tauri/target/release/bundle/`。

## 前端技术说明

项目使用 React、TypeScript、Vite 和 Oxlint。Vite 的开发服务器支持热模块替换（HMR）。当前启用了以下官方 React 插件：

- [@vitejs/plugin-react](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react)：使用 [Oxc](https://oxc.rs) 处理 React 文件。
- [@vitejs/plugin-react-swc](https://github.com/vitejs/vite-plugin-react-swc)：使用 [SWC](https://swc.rs/) 编译 React 文件。

### React 编译器

当前模板未启用 React Compiler，因为它可能影响开发和构建性能。需要启用时，请参考 [React Compiler 安装文档](https://react.dev/learn/react-compiler/installation)。

### 扩展 Oxlint 配置

如果要开发生产环境应用，建议安装 `oxlint-tsgolint` 并编辑 `.oxlintrc.json`，启用类型感知规则：

```json
{
  "$schema": "./node_modules/oxlint/configuration_schema.json",
  "plugins": ["react", "typescript", "oxc"],
  "options": {
    "typeAware": true
  },
  "rules": {
    "react/rules-of-hooks": "error",
    "react/only-export-components": ["warn", { "allowConstantExport": true }]
  }
}
```

完整规则和分类请参阅 [Oxlint 规则文档](https://oxc.rs/docs/guide/usage/linter/rules)。
