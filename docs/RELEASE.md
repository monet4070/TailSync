# TailSync 发布手册

tag 发布由 `.github/workflows/release.yml` 执行。当前默认是无需购买平台证书的
`community` 层级；以后具备预算时，可切换到 `trusted`，而不改变 updater 密钥、
客户端更新端点或发布物命名。

## 两层签名的边界

| 层级 | Community（当前默认） | Trusted（以后可选） |
|---|---|---|
| updater 包 | TailSync 私钥签名，客户端固定公钥校验 | 相同 |
| 完整性 | SHA-256 清单、签名包版本校验、防降级 | 相同 |
| macOS | ad-hoc 签名，未公证 | Developer ID 签名、公证并 stapled |
| Windows | 无 Authenticode | Authenticode 签名与时间戳 |
| 首次启动 | 可能出现 Gatekeeper / SmartScreen 警告 | 正常的平台信任体验 |

updater 签名保护“更新服务器不能给客户端替换恶意包”；平台代码签名和 Apple
公证保护“操作系统能确认发行者”。它们是两套独立信任链。没有付费平台证书时，
仍可安全地提供签名自动更新，但不能承诺没有系统警告。

## 一次性设置：免费 Community Release

在可信机器上生成专用 Tauri updater 密钥：

```powershell
cd windows
npm ci
npx tauri signer generate --write-keys ..\tailsync-updater.key
```

私钥及密码必须保存在仓库之外，并备份到受控的离线位置。将以下三个值加入
GitHub Actions secrets：

| Secret | 内容 |
|---|---|
| `TAILSYNC_UPDATER_PUBLIC_KEY` | Tauri updater 公钥 |
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater 私钥文件的完整内容 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 私钥密码 |

不设置仓库变量 `TAILSYNC_RELEASE_TIER` 时，工作流默认使用 `community`。也可在
GitHub 仓库的 Actions variables 中显式设置：

```text
TAILSYNC_RELEASE_TIER=community
```

发布构建缺少 updater 公钥或私钥会直接失败，不会静默生成一个无法验证更新的包。
公钥会编译进两个客户端。轮换公钥需要先用旧私钥签一个过渡版本，不能直接覆盖。

## 发布通道

- `v2.2.0` 这类稳定 tag 会发布附件，并生成 `latest.json`，供客户端自动更新。
- `v2.2.0-rc.1` 这类预发布 tag 会发布附件，但不会生成或覆盖 `latest.json`。
- 当前客户端只订阅稳定通道；预发布包必须从 GitHub Release 页面手动安装。

## 发布步骤

1. 在两个 `tauri.conf.json` 和三个 Cargo package 中设置相同的语义版本。
2. 合并前确认 CI 全绿，并完成 macOS 与 Windows 真机验收。
3. 推送匹配的 tag，例如 `v2.2.0`。
4. 确认 Release workflow 的双平台打包、启动烟测、updater 签名和 manifest 校验通过。
5. 下载公开附件，根据 `.sha256` 文件复核哈希。
6. 在干净的 Mac 和 Windows 真机安装，再从上一个公开版本执行一次完整更新。
7. 真机更新通过后再对外公告；首次闭环未完成前，不把自动更新标记为已验证。

macOS 可使用：

```bash
shasum -a 256 -c TailSync-2.2.0-macOS-universal.sha256
```

Windows 可按清单中的每个文件执行：

```powershell
Get-FileHash .\TailSync-2.2.0-Windows-x86_64-setup.exe -Algorithm SHA256
```

## Community 首次启动说明

macOS 用户应先尝试打开应用，然后在“系统设置 → 隐私与安全性”中确认“仍要打开”。
Windows 用户可在 SmartScreen 对话框中选择“更多信息 → 仍要运行”。发布说明必须明确
展示这项限制，不应引导用户全局关闭 Gatekeeper、SIP、SmartScreen 或杀毒软件。

## 以后切换 Trusted Release

将 Actions variable 改为：

```text
TAILSYNC_RELEASE_TIER=trusted
```

同时补齐以下付费平台凭据：

| Secret | 内容 |
|---|---|
| `WINDOWS_CERTIFICATE_BASE64` | Authenticode PFX 的 Base64 |
| `WINDOWS_CERTIFICATE_PASSWORD` | PFX 密码 |
| `APPLE_SIGNING_IDENTITY` | 完整 Developer ID Application identity |
| `APPLE_CERTIFICATE_BASE64` | Developer ID PKCS#12 的 Base64 |
| `APPLE_CERTIFICATE_PASSWORD` | PKCS#12 密码 |
| `APPLE_ID` | `notarytool` 使用的 Apple 账号 |
| `APPLE_APP_SPECIFIC_PASSWORD` | Apple app-specific password |
| `APPLE_TEAM_ID` | Apple Developer Team ID |

Trusted 构建会额外校验 Authenticode、`codesign`、`spctl`、notarization 与 stapling；
任一凭据不完整或验证失败都会终止发布。切换层级不需要重新生成 updater 密钥。

## 数据迁移注意事项

v1 导入成功后，原 `~/TailSync_History/history.db` 和 `.fernet_key` 会保留为恢复输入。
旧数据库可能含明文 `desc` 预览。检查迁移报告并保留确实需要的备份后，应由用户自行
安全删除旧目录；TailSync 不会自动删除它。
