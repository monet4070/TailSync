# Finder 文件复制分流修复 — 2026-10-13

用户报告 Finder 复制文件无法触发远端同步，本地历史只显示文件名称，随后明确授权“直接修复并生成测试包”。

修复工作树：`/Users/monet/.codex/worktrees/finder-file-copy-fix-2026-10-13/TailSync-rc-phase35`，基于 `7759a232fc7657df01fd4e482bcb9f7f26f1c930`。保留原工作树的所有既有未提交工作，未提交或发布此次改动。

## 实际输入与根因

当前运行的旧包为 `/Users/monet/Downloads/TailSync构建/2026-10-10-preview-provenance-01/TailSync.app`，GUI/daemon PID 859/869。首次只读采集时剪贴板 revision 17，真实复制的一个 Markdown 文件同时提供四种格式：

| 格式 | 旧 CNA1 采集状态 | 字节数 |
| --- | --- | --- |
| public.file-url | Absent（延后读取） | 0 |
| public.utf16-external-plain-text | Captured | 64 |
| public.utf8-plain-text | Captured（文件名） | 31 |
| com.apple.icns | Captured（图标） | 851946 |

旧 helper 的普通文件读取入口可以读取真实文件路径。CNS1/CNA1 却都报告 Stable，未识别为 FileSelection。本地最新历史 id 3209 为 composite/self，没有 batch_id。

`isPureFileSelection` 允许 UTF-8 文件名，但不允许 Finder 的 UTF-16 文件名与图标；这使文件选择被视为混合内容。Core `from_archive → from_native` 返回 NativeOnly，监听器保存本机归档后结束本轮，不进入文件批次发送。该分流问题不依赖远端连接成功与否。

## 修改

`macos/src-tauri/clipboard-helper.swift` 的文件选择辅助格式精确增加 `public.utf16-external-plain-text` 与 `com.apple.icns`，继续要求每个 item 都包含 public.file-url。HTML、QQ 私有格式和未知混合格式仍按文稿/原生内容采集；敏感格式标记的整板抑制仍先于文件判断。文件名/图标辅助格式不进入文件内容读取与传输。

现有 FileSelection 状态通过 Core 的 CaptureError::Files 回到原文件发送链路；未改动 Rust、协议、数据库或 Windows 代码。新增真实 AppKit 命名剪贴板夹具及生产 helper 回归测试，不写用户 general 剪贴板。

## 验证

- RED：针对旧运行包，7 项测试中 4 个失败子场景确认单文件、2 文件、17 文件及带来源标记文件的误分流；17 文件原来还会触发 helper exit 65。
- GREEN：新编译 helper 的 7 项文件分流测试全部通过；对最终包内 helper 再执行同组测试及 10 项 QQ 原生转换测试，17/17 通过。
- 文件分流测试覆盖单文件、2 文件、17 文件、纯 URL、来源标记、没有 URL 的文件名/图标、HTML/QQ/未知混合项，以及 1/17 项敏感标记抑制。FileSelection 输出均为 32 字节元数据帧。
- 生成策略一致性检查及 git diff --check 通过。
- 最终包 deep/strict 签名验证、三个二进制 arm64/macOS 13.0 部署检查、helper protocol 4 自检、daemon 启用态 resolver 全部通过。resolver 解析到新包内 helper。

后续只读重查时，用户 general 剪贴板已由 revision 17 变为 19，旧/新 helper 对当时的纯文件 URL 输入均报告 FileSelection，因此后续观察不能作为“同一个真实 Finder 输入修复前后对比”。本轮没有重写 general 剪贴板；真实四格式形状的 RED/GREEN 使用 private 命名剪贴板完成。

## 测试包与保留证明

新包：`/Users/monet/Downloads/TailSync构建/2026-10-13-finder-file-sync-01/TailSync.app`。

采用当前运行包的定点修复打包：复制完整旧包，只编译替换 helper，然后重新签名。旧包全文件 SHA-256 与 10 月 10 日已保存的包清单完全一致；修复起点的 helper 源码 SHA-256 与同日源码清单一致（`89fe2c141d22d7e83f8dd31a0e6839ac50ca363dbd5fc9321d55b3b3b267846a`）。

新包 daemon 字节与旧包一致；SwiftUI 的一次性去签名副本 SHA-256 一致，证明界面可执行代码未改。重新签名会更新主程序资源签名，所以不能声称签名后的整个 TailSync 文件逐字节一致。资源文件与其他包内容均保留；旧包再次复核完全一致。

此次仅完成本机识别修复、原生接缝回归及包检查。没有切换正在运行的 App，没有修改旧历史记录、配对或设置；Windows 原生及真实双端文件传输尚待验收。使用时退出旧 App，打开新包，再在 Finder 复制文件。

证据在本工作树 `artifacts/finder-file-copy-fix/`：red.log、green.log、package-tests.log、package-manifest.json、live-clipboard-comparison.json。产品修复只有辅助格式识别，不保留临时调试日志。
