# Windows WPF 前端迁移记录 · 2026-10-30

## 基准与交付

前端依据为实际 fetch 的 GitHub main：`b61a0a04b88f5828707f7ef64781389944471be8`。迁移保留工作区较新 Rust 后台，没有把共享业务代码降回旧 main。WPF 使用原生控件与原生预览库，经认证的当前用户命名管道调用同一组业务命令。Tauri 仅保留后台宿主、托盘和系统集成，WPF 模式不创建 WebView。

构建入口：`windows/native-ui/build-wpf.ps1`；运行入口：`TailSync.Wpf.exe`。自包含 Windows x64 便携目录包含 WPF 宿主、`TailSync.Backend.exe`、可用的剪贴板 helper、.NET、PDFium/Skia 及许可证；不需要安装 .NET。

## 功能对照

| main 前端功能 | WPF 对应实现 | 验证与边界 |
| --- | --- | --- |
| 历史/收藏窗口、30 条分页、搜索取消、8 分类、日期范围 | `MainWindow`、`ShellViewModel`、`NativeHistoryClient` | 真实加密 DB、65 条种子、30/30/5 分页及搜索竞态通过 |
| 日期与文件批次分组、批次展开、恢复、取消传输 | 原生虚拟列表、批次导航、进度与停止按钮 | 分组/选中边界检查通过；真实文件传输需双设备验收 |
| 收藏、保护、明确删除收藏、清空保留收藏 | 复用原 Rust mutations | 真实后端往返通过，未用本地假状态替代 |
| 键盘、长按收藏、右键删除、错误提示、同步警告 | 原生输入处理、全局恢复互斥、warning ACK coordinator | 查询/恢复并发和警告确认回归通过；IME/触摸板待验收 |
| 缩略图、按需预览、独立预览所有者生命周期 | 原始 TSPV、原生图像、有限 PNG 缓存 | 无 base64 大预览复制；原生解码与错误帧检查通过 |
| 文本、代码、Markdown | AvalonEdit、Markdig 到 FlowDocument | 选择、搜索、换行、字号、虚拟化文本、高亮阈值、Markdown 表格 |
| 图片、SVG | WPF 图像/SharpVectors、安全图片解码与显式来源授权 | HTTPS 图片授权策略通过；公网授权下载正路径待验收；外链 CSS/字体/use 不加载 |
| PDF、DOCX | PDFium 分页画面 + PdfPig 独立文本视图；DOCX ZIP/XML 到 FlowDocument | 合成 PDF/DOCX 通过；PDF 非对齐文字层，复杂 DOCX 非 Word 等价排版 |
| 通用设置、历史上限、配额、语言 | 串行 field-intent 保存、canonical reload | 持久化、失败恢复、稳定错误本地化和标题栏语言检查通过 |
| 快捷键录制/暂停/恢复、开机启动 | 原 Rust shortcut 服务、WPF 录制弹窗、Windows 用户启动项 | 保留冲突回滚；迁移已开启的旧 TailSync 注册；实际系统操作待验收 |
| 本机/远端身份、路由、配对、远程邀请 | `ConnectionsView`、`PairingWindow` 及真实后台 | 命令可达性与状态边界审查通过；网络默认隔离验收，不声称双端成功 |
| 存储迁移、旧存储一次清理授权 | 原 Rust storage 命令、系统目录选择 | UI 接入并保留确认；未移动或删除用户真实数据 |
| 托盘、全局唤起、单实例、tailsync:// | Rust 原集成 + WPF 事件队列、用户会话激活管道 | 父进程回收、认证、事件及重复启动路径已检查；外部焦点恢复按 main 保持 |
| Theme V2 与主题包 | `ThemeService`、`ThemeManagerView`、Core theme commands | 五内置主题、四模式验证、真实安装/升级/回滚/删除、字体/图片资源与重启持久化通过 |
| 更新检查/安装 | 检查复用原后台，WPF 禁止安装 Tauri-only 包 | 需提供包含 WPF 宿主和后台的签名更新发行包后启用自动安装 |

main `tailsyncClient.ts` 的 67 个唯一命令已全部映射：62 个复用真实 Rust 命令、4 个预览窗口命令由原生生命周期替代、1 个不兼容的更新安装入口明确拒绝。静态对照表位于 `windows/ui-benchmark/results/wpf-main-migration/backend/main-command-coverage.md`；可达性不等于外部设备验收。

## 主题与 Windows 质感

保留用户认可的暖白、圆角卡片及主题色，右上角使用 Windows 窗口控制，滚动条为 4px 可见滑块、10px 命中宽度。列表采用像素滚动与 Recycling 虚拟化，静止时解除动画回调；窄设置行把动作移到下一行，按钮按实际内容布局。

字体遵循 main 的语义角色及 token，不再把所有主题覆盖成同一字体。Canvas/Ledger 的历史与搜索保留其 display 衬线链；Flux/Aura/Mono 的历史使用 reading 链，Aura 搜索使用 display。Display 标题分别遵循主题的宋体/Georgia、Bahnschrift、Candara、Cascadia 等 Windows 回退。缺少某字体时按有序列表回退，不捆绑 macOS 专有字体。ClearType、Display 排版、像素对齐继续启用；截图是视觉树导出，不能代替屏幕上 ClearType 的实际效果。

## 本机验证证据

- Release build / self-contained publish：0 warnings、0 errors。
- Core presentation：19 项检查通过。
- 自包含 WPF smoke：12 项通过。
- WPF 布局/滚轮：147 项断言通过，包含默认/最小尺寸、内容边界与停止订阅。
- 最终自包含 WPF → 真实隔离 Rust：127 项断言通过，位于 `delivery/migration.json`。
- Rust real IPC：143 项通过；父进程与无认证生命周期：3 项通过；桥接专项 unit tests：6 项通过。
- 证据目录：`windows/ui-benchmark/results/wpf-main-migration/`。不同测试集有重叠，不把断言数量当成独立功能数量。

全部自动集成数据来自新建的专用 F: 测试目录。仓库只保存报告与截图，不保存测试 DB、密钥或设备身份文件。IPC 与生命周期测试确认后台可以正常关闭，并在宿主退出时回收。预览测试仅使用合成内存文档。

## 验收范围

这是一套已接入真实服务、可构建运行的 WPF 迁移版。未声称通过的项目包括：真实双设备同步/安全配对、系统剪贴板恢复、全局快捷键、系统通知、用户存储迁移、复杂真实文档、多屏 DPI、IME、触摸板和长时间运行。需要相应实际环境才能完成这些验收。

较新分支 composite 富文本的端到端兼容性不属于 main 前端迁移证明；若启用该后端 feature，须携带对应 helper 并独立验收。旧原型 WPF 85 MiB 基准发生在真实后台接入之前，本次没有重新跑五技术栈等价性能矩阵，不能用旧值宣称现在的产品总内存。

第三方许可随便携目录分发；NuGet 依赖锁定于 `packages.lock.json`，Rust 后端保持原 Cargo.lock。
