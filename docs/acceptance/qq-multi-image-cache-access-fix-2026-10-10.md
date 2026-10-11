# QQ 多图消息图片缓存授权修复 — 2026-10-10

## 当前结论

macOS 代码、回归、打包和切换已完成。真实 QQ 四图消息的完整保存仍待用户确认图片缓存访问权限；不能以合成测试代替真实授权后的验收。

## 原因与原始复现

当前复制的 QQ 消息含 4 个图片元素。QQ 第一项提供 TIFF，可转换为 PNG；另外 3 项只给出了受保护缓存的路径。现有 helper 读取四个缓存文件均返回 Operation not permitted，最终采集 HTML 为 1 张内嵌图、3 个缺图位置。数据库保留了采集结果，丢失发生于采集前，非历史列表只显示第一张。证据保留在 `artifacts/rich-text-repair/2026-10-09-qq-missing-images/`。

## 修复

- 设置 → 历史记录新增 QQ 消息图片授权入口。只有用户点击按钮才打开系统文件夹选择窗口；普通复制不弹权限窗口。
- helper 创建并重新解析只读 security-scoped bookmark，目标仅允许当前用户下一个 QQ 账号的 `nt_data/Pic` 目录。建议目录只来自当前复制消息，未扫描账号。
- 采集从获准目录打开描述符，避免先遍历被保护的祖先路径。每张图仍需匹配已知月份/Ori/散列文件名，并受原有文件大小、图像、数量和总预算限制；不放宽任意文件、网络、软链接读取。
- 授权文件为有界、0600 原子写入；损坏、过期、错误账号、软链接及不可读状态均不宣称已授权。未授权仍保留图片位置并使用第一图的剪贴板回退。
- 授权后需重新复制消息；未修改旧历史记录。

## 验证

- 生产缓存读取函数和 QQ 转换函数在自有临时目录测试：4 张图全部内嵌、文字和图片顺序保持、缺图为 0。
- 独立进程重启后重新解析已保存 bookmark；目录范围、错账号、路径穿越、文件与目录软链接、超限/空图片、损坏/超限/软链接 bookmark、错误选择不覆盖旧授权、测试数据目录隔离均通过。
- 原 QQ helper 转换回归 10/10；Swift 设置和 AppBehavior 回归 35/35。canonical include 和 capture-failure 路由检查通过。
- 新包签名、macOS 部署版本、arm64 架构、helper protocol 4 和 composite resolver 检查 PASS。
- 正常退出旧包，启动新包；生产库 5 条记录逐列一致，配置与剪贴板 snapshot 字节一致。备份与 `switch-result.json` 均在本证据目录。
- 新包实际 PID 路径已确认。后台启动后 UI 工具未获得窗口，设置页视觉验收仍未验证。
- 未授予 QQ 缓存权限、未启用完全磁盘访问。真实 QQ 4/4 保存、实际授权窗口与跨设备行为仍未验证。

## 候选绑定

包：`/Users/monet/Downloads/TailSync构建/2026-10-10-qq-multi-image-01/TailSync.app`
源码工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`
源码 SHA-256：`e461bd3f8247d55eecc3a5d92553de461507a4b7068d4f836abbd4ac72bc183f`
包 SHA-256：`379dfb3428a8bd005b52e63bc24a2d4331ef76d3ec2fa5ff935a189d690b3d44`
