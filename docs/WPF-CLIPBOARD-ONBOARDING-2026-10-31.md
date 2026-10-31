# WPF 剪贴板与引导分支集成（2026-10-31）

源分支：`origin/codex/clipboard-onboarding-fixes-2026-10-31`，提交 `35c7234b568c29143bb74618762f7a92771fdea5`。

集成分支：`codex/wpf-clipboard-onboarding-windows-2026-10-31`。合并提交为 `86e2fb5`，保留此前 WPF 的界面、主题字体、预览、真实后台及连接进展。完整源代码位于 `C:/Users/monet/.codex/worktrees/ui-stack-benchmark/TailSync-2.2.2`；原先未提交的工作在集成前备份为 Git stash `8f79ec7f156b08f1643dfa63ec596a9c7f3c733a`，备份仍保留。

## 实现

- 合入图片归档的累计原件字节、派生字节及逐引用像素限制；保留成功下载的图片，缺失图片允许后续补齐。采用源分支的系统代理与 Fake-IP 支持、浏览器图片识别及网页图文顺序修复。
- WPF 设置增加网络图片自动保存开关，经现有字段 patch 写入 Core。部分恢复消费 `complete` / `missing_images` 报告，显示缺图信息；复合条目恢复携带当前条目的内容身份和原始 ID。
- 历史窗口首次打开聚焦条目；`1–6` / 小键盘 `1–6` 对应本页实际可见的前六条，完整恢复后关闭历史及所属预览，失败或部分恢复保留窗口。日期和批次标题不占编号；展开批次后重排编号。
- `←` / `→` 翻页，`F` 搜索，搜索内 `↓` 返回条目；上下跨页保持边缘选择。输入框、按钮、组合输入及模态窗口不触发列表取用。查询、恢复及删除期间拦截旧行操作，翻页失败保留提交过的页码、记录和选择。
- WPF 原生引导复用 `shared/onboarding/guide.json` 和 `demo.json`，涵盖中英文 15 步。高亮跟随真实 WPF 控件、滚动和布局，提示卡占独立区域。最小窗口压缩提示卡、把永久关闭放入 `⋯` 菜单，保留历史条目与导航；完整文案保留在标题提示中。
- 引导历史、收藏、清空、回溯与模拟配对仅操作内存。进入和退出取消旧查询、清除历史快照、缩略图和预览；晚到结果不能覆盖当前模式。虚拟记录与批次在 C# 和 Rust 命名管道两层均被禁止送入真实历史操作。
- 图片、SVG、Markdown、PDF、Word 样本使用已有原生预览；PowerPoint 与富文档样本使用共享结构化文档。引导中主题可以本地预览，结束后恢复已保存外观。
- 正常首次前台启动自动引导，配对链接和显式测试启动优先。结束后可从设置重开；永久关闭沿用 Rust 中绑定安装身份的状态，设置入口随之移除。
- 打包强制携带匹配架构的 clipboard helper，检查 PE 架构、协议版本和发布目录实际解析结果。默认构建启用复合剪贴板。ARM64 交叉构建只检查架构，运行检查仍需 ARM64 Windows。
- Windows 回归修正：进程监督夹具使用系统 PowerShell 的原始字节流；SQLite 测试释放实际所有权后再删除目录；临时目录不再使用包含 Windows 非法字符的 `SystemTime` 调试文本。引导 Swift 生成器规范化换行，生成文件固定 LF。

## 本机验证

环境：Windows x64、.NET 10.0.403 SDK、Rust MSVC，日期为 2026-10-31。以下检查存在覆盖重叠，不合计为独立功能数量。

| 检查 | 结果 |
|---|---|
| WPF Release、自包含发布 | 通过，0 警告、0 错误 |
| Core 完整单元测试 | 1005 通过，3 项原有忽略 |
| Runtime 单元与集成测试 | 142 通过，2 项验收用例忽略 |
| 原生命名管道单元测试 | 6 通过 |
| 真实隔离后台 IPC | 180 项检查通过 |
| 后台父进程与早期校验 | 3 项检查通过 |
| .NET 展示、分页与竞争检查 | 27 项通过 |
| WPF 基础运行 | 12 项通过 |
| WPF 原有真实后台集成 | 139 项通过 |
| 新增 WPF 引导、快捷键、隔离与原生预览 | 96 项通过 |
| 跨平台契约、引导生成一致性、格式检查 | 通过 |
| 发布目录 helper 实际自检 | 协议 2，复合功能与分析能力启用 |

证据目录：`F:/TailSync/wpf-clipboard-onboarding-qa-20261031`。最终界面报告与截图在 `native-verified/`，历史集成报告在 `migration-final/`，基础检查为 `smoke-final.json`；IPC 与生命周期报告分别位于 `backend/`、`lifecycle/`。PNG 是实际 WPF 渲染树导出，已检查中英文、深色、300×360 最小历史窗口及 Word/PowerPoint 样本。

可重复执行的主要命令：

```powershell
# 从仓库根目录运行；本机 SDK 路径可替换为 dotnet
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
node shared/onboarding/generate-swift.mjs --check
node scripts/run-isolated-tests.mjs -- cargo test --release --locked -p tailsync-core --features protocol-composite-clipboard --lib
node scripts/run-isolated-tests.mjs -- cargo test --release --locked -p tailsync-runtime --features tailsync-core/protocol-composite-clipboard --tests --no-fail-fast
node scripts/run-isolated-tests.mjs -- cargo test --release --locked --manifest-path windows/src-tauri/Cargo.toml --features protocol-composite-clipboard --lib wpf_bridge
./windows/native-ui/build-wpf.ps1 -Publish

# 从完整的发布目录运行，所有数据位于独立测试配置
./TailSync.Wpf.exe --clipboard-onboarding-check-dir=F:/TailSync/new-wpf-onboarding-check
./TailSync.Wpf.exe --migration-check-dir=F:/TailSync/new-wpf-migration-check
```

## 交付与边界

便携目录：`F:/TailSync/wpf-clipboard-onboarding-20261031`，入口为 `TailSync.Wpf.exe`。需要保留旁边的 `TailSync.Backend.exe`、`tailsync-clipboard-helper.exe`、运行库和许可证。退出原先运行的 TailSync 后再启动新版，避免系统现有单实例机制把启动请求交给旧程序。

本次没有用用户的正式剪贴板、历史或信任状态执行写入验收。真实 Mac/Windows 双设备同步、目标应用粘贴、系统代理下的公网图片、ARM64 运行、多显示器 DPI 切换与长时运行仍需目标环境验收。现有 WPF 更新检查继续保留；自动安装仍需要包含 WPF 宿主及后台的对应更新包。
